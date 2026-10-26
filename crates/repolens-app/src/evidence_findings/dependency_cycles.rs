use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 7. F-DEPENDENCY-CYCLES
    //
    // Replaces the retired `health.signals[id=dependency-cycles]` legacy
    // signal. Fires when the import graph contains at least one cycle.
    // -----------------------------------------------------------------------
    pub(super) fn dependency_cycles(&self) -> Option<EvidenceFinding> {
        let gh = self.graph_health?;
        if gh.cycle_count == 0 {
            return None;
        }

        let cycle_count = gh.cycle_count as i64;
        let files_in_cycles = gh.files_in_cycles_count as i64;
        let largest_cycle = gh.largest_cycle_size as i64;

        // Narrow scope.languages to the languages of files actually
        // participating in cycles. The default `languages_from_profile`
        // returns every language scanned (Markdown, JSON, YAML, …), which
        // makes the scope claim that cycles span text/data files — they
        // do not. When the full graph is available, recompute cycles to
        // collect the in-cycle file set and derive languages from those
        // paths via the standard extension-based detector. Fall back to
        // the profile-wide list when the graph isn't loaded.
        let mut cycle_groups: Vec<Vec<String>> = Vec::new();
        // Cycles that cross a crate (or aren't Rust): (count, largest size).
        let mut wider: Option<(usize, usize)> = None;
        let cycle_languages: Vec<String> = match self.graph {
            Some(graph) => {
                let cycle_report = crate::graph_analysis::GraphAnalyzer::find_cycles(graph);
                let crate_dirs: Vec<&str> = graph
                    .nodes
                    .iter()
                    .filter_map(|n| {
                        if n.id == "Cargo.toml" {
                            Some("")
                        } else {
                            n.id.strip_suffix("/Cargo.toml")
                        }
                    })
                    .collect();
                let (mut n, mut largest) = (0usize, 0usize);
                for scc in &cycle_report.cycles {
                    if !within_one_crate(&scc.files, &crate_dirs) {
                        n += 1;
                        largest = largest.max(scc.files.len());
                    }
                }
                wider = Some((n, largest));
                let mut langs: std::collections::BTreeSet<String> = Default::default();
                for scc in &cycle_report.cycles {
                    let mut files = scc.files.clone();
                    files.sort();
                    for file in &files {
                        let lang =
                            repolens_extract::lang::detect(std::path::Path::new(file)).to_string();
                        if lang != "Unknown" {
                            langs.insert(lang);
                        }
                    }
                    cycle_groups.push(files);
                }
                cycle_groups.sort();
                langs.into_iter().collect()
            }
            None => languages_from_profile(self.profile),
        };

        // Cycles inside a single Rust crate are ordinary module structure
        // (Cargo forbids crate-level cycles), so they rate Low on their own.
        // Severity comes from the remaining cycles via the usual thresholds.
        let all_within_crate = matches!(wider, Some((0, _)));
        let (sev_count, sev_largest) = wider.unwrap_or((gh.cycle_count, gh.largest_cycle_size));
        let severity = if all_within_crate {
            FindingSeverity::Low
        } else {
            match cycle_severity(sev_count, sev_largest) {
                "high" => FindingSeverity::High,
                "medium" => FindingSeverity::Medium,
                _ => FindingSeverity::Low,
            }
        };

        // Name the in-cycle files so an agent can act (which files? which cycle
        // to break?) without a separate find_cycles call. Cap the enumerated
        // cycles to keep the finding bounded on pathological graphs; disclose
        // truncation via `sampleComplete` (mirrors F-ORPHAN-FILES).
        const MAX_CYCLE_GROUPS: usize = 12;
        let groups_complete = cycle_groups.len() <= MAX_CYCLE_GROUPS;
        let shown_groups: Vec<Vec<String>> =
            cycle_groups.into_iter().take(MAX_CYCLE_GROUPS).collect();
        let mut cycle_files: Vec<String> = shown_groups.iter().flatten().cloned().collect();
        cycle_files.sort();
        cycle_files.dedup();

        // Concrete next step once members are known; stays calibrated (inspect
        // first; break only "if it causes maintainer pain") so it does not
        // contradict the "cycles must be removed" non-implication. Falls back to
        // the conservative review action when the graph (hence members) is absent.
        let action = if cycle_files.is_empty() {
            RecommendedAction::ReviewIfMaintainerPain
        } else {
            RecommendedAction::ConcreteAction {
                steps: vec![
                    "Inspect the file(s) listed in this finding's evidence/scope to judge \
                     whether each cycle is an intentional bidirectional relationship or \
                     accidental coupling."
                        .into(),
                    "If a cycle causes maintainer pain, break the smallest one first by \
                     inverting a single import — move the shared item into a leaf module both \
                     sides import, or depend on a trait/interface instead of a concrete type — \
                     then re-scan and re-check this finding to confirm the cycle count dropped."
                        .into(),
                ],
            }
        };

        Some(EvidenceFinding {
            id: FindingId::new(F_DEPENDENCY_CYCLES),
            title: format!(
                "Dependency cycles detected ({cycle_count} cycle(s), largest {largest_cycle} files)"
            ),
            classification: FindingClassification::Actionable,
            severity,
            // Severity is derived from the `cycle_count` / `largest_cycle_size`
            // heuristic in `cycle_severity` (>=6 / >6 → high, >=3 / >3 →
            // medium, otherwise low). Threshold-based, hence heuristic.
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 4B Step 2 (2026-05-12): `graph.cycleCount` is backed by
            // the graph-cycles family (I-GRAPH-CYCLES-* catalogue, doc-06
            // §5 `confirmOnly`). Heuristic severity tier over verified
            // inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: {
                let mut ev = vec![
                    EvidenceItem::Observation {
                        metric: metric_names::GRAPH_CYCLE_COUNT.into(),
                        observed_value: serde_json::json!({
                            "cycleCount": cycle_count,
                            "filesInCyclesCount": files_in_cycles,
                            "largestCycleSize": largest_cycle,
                        }),
                        source_paths: vec![],
                        source_type: EvidenceSource::ImportGraph,
                        invariant_id: Some("I-GRAPH-CYCLES-EXCLUDE-DECLARES".into()),
                    },
                    EvidenceItem::HeuristicJudgment {
                        rule: "severity = cycle_severity(cycleCount, largestCycleSize)".into(),
                        observed_metric: metric_names::GRAPH_CYCLE_COUNT.into(),
                        observed_value: serde_json::json!({
                            "cycleCount": cycle_count,
                            "largestCycleSize": largest_cycle,
                        }),
                        threshold: serde_json::json!({
                            "withinOneCrate": "low when every cycle stays inside one Rust crate",
                            "high": "cycleCount>=6 or largestCycleSize>6",
                            "medium": "cycleCount>=3 or largestCycleSize>3",
                            "low": "otherwise"
                        }),
                        rationale: Some(
                            "Heuristic threshold; not justified by an invariant. The severity \
                         tier reflects the cost of reasoning about and refactoring tangled \
                         imports, not a runtime property."
                                .into(),
                        ),
                    },
                ];
                if !shown_groups.is_empty() {
                    ev.push(EvidenceItem::Observation {
                        metric: metric_names::GRAPH_CYCLE_FILES.into(),
                        observed_value: serde_json::json!({
                            "cycles": shown_groups,
                            "sampleComplete": groups_complete,
                        }),
                        source_paths: cycle_files.clone(),
                        source_type: EvidenceSource::ImportGraph,
                        invariant_id: Some("I-GRAPH-CYCLES-EXCLUDE-DECLARES".into()),
                    });
                }
                ev
            },
            inference: ResponsibleInference {
                statement: format!(
                    "The import graph contains {cycle_count} cycle(s) involving \
                     {files_in_cycles} file(s); the largest cycle spans {largest_cycle} files. \
                     Cycles increase the cost of reasoning about, testing, and refactoring \
                     module boundaries."
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![],
                does_not_imply: vec![
                    "the code fails to compile".into(),
                    "cycles cause runtime problems".into(),
                    "cycles must be removed".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::IMPORT_GRAPH_STRUCTURAL_ONLY.into(),
                detail: "Cycle detection is structural: it reflects import edges in the \
                         scanned source. Build-time, type-level, or runtime cycles that the \
                         import graph does not represent are out of scope."
                    .into(),
            }],
            action,
            scope: self.scope(cycle_files, cycle_languages),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}

/// True when every file in `files` is Rust and owned by the same crate
/// (deepest `Cargo.toml` directory that contains it).
fn within_one_crate(files: &[String], crate_dirs: &[&str]) -> bool {
    let owner = |f: &str| -> Option<String> {
        crate_dirs
            .iter()
            .filter(|d| d.is_empty() || f.starts_with(&format!("{d}/")))
            .max_by_key(|d| d.len())
            .map(|d| d.to_string())
    };
    let mut first: Option<String> = None;
    for f in files {
        if !f.ends_with(".rs") {
            return false;
        }
        let Some(o) = owner(f) else { return false };
        match &first {
            None => first = Some(o),
            Some(x) if *x == o => {}
            _ => return false,
        }
    }
    first.is_some()
}
