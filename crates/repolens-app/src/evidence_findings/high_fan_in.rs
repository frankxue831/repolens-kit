use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 4. F-HIGH-FAN-IN-FILE — aggregates all high-fan-in files into one
    // finding (matching the F-ORPHAN-FILES shape) so agents don't get N
    // duplicate findings that differ only by path. The 2026-05-12 dogfood
    // against gm-crypto-rs surfaced 10 separate F-HIGH-FAN-IN-FILE findings,
    // all severity=none/expected — pure noise in the response.
    // -----------------------------------------------------------------------
    pub(super) fn high_fan_in_files(&self) -> Option<EvidenceFinding> {
        let graph = self.graph?;
        if graph.nodes.is_empty() {
            return None;
        }
        let degree_map = compute_degree_map(graph);
        let in_degrees: Vec<(String, i32)> = degree_map
            .iter()
            .map(|(p, (i, _))| (p.clone(), *i))
            .collect();
        if in_degrees.len() < HIGH_FAN_IN_MIN_GRAPH_NODES {
            return None;
        }
        let total: i64 = in_degrees.iter().map(|(_, d)| *d as i64).sum();
        let avg = total as f64 / in_degrees.len() as f64;
        let raw_cutoff = (avg * HIGH_FAN_IN_MULTIPLIER) as i32;
        // Statistical threshold OR absolute floor — whichever is stricter — so
        // graphs where the mean inbound degree is well below 1 don't flag
        // every leaf-importer. See HIGH_FAN_IN_MIN_INBOUND_EDGES comment.
        let cutoff = raw_cutoff.max(HIGH_FAN_IN_MIN_INBOUND_EDGES - 1);
        // For whole edge counts, `d > cutoff` is the same test as
        // `d > avg × multiplier` (or the floor), so show that value.
        let shown_cutoff = ((avg * HIGH_FAN_IN_MULTIPLIER)
            .max(f64::from(HIGH_FAN_IN_MIN_INBOUND_EDGES - 1))
            * 100.0)
            .round()
            / 100.0;
        let mut all_high: Vec<(String, i32)> = in_degrees
            .into_iter()
            .filter(|(_, d)| *d > cutoff)
            .collect();
        if all_high.is_empty() {
            return None;
        }
        all_high.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let total_above_threshold = all_high.len();

        // Pre-filter: drop indicator-tagged hub modules (lib.rs, mod.rs,
        // prelude.rs, *-core/*-util crate paths) before the size cap.
        // Every Rust workspace has these — they legitimately have high
        // fan-in by design, the limitation note already calls them out,
        // and listing them dominates the response on real repos. The
        // 2026-05-14 dogfood on repolens-rs surfaced 9/10 high-fan-in
        // entries as indicator paths.
        //
        // If the entire kept set is indicator paths, the finding is kept as
        // a no-action note that says how many files were left out and why
        // (#31: nothing over the threshold is dropped silently).
        //
        // Exception (#31): files in the repo's main crate are never dropped
        // for its name. On gm-crypto-rs the main library is `gmcrypto-core`,
        // and the `-core` stem hid its most imported files. A `*-core` crate
        // that is not the main one is still left out, but the finding says
        // so (`sharedCrateFilesLeftOut`).
        let packages = package_dirs(graph);
        let main_package = largest_code_package(graph, &packages);
        let mut high: Vec<(String, i32)> = Vec::new();
        let mut shared_left_out: Vec<(String, i32)> = Vec::new();
        let mut hub_left_out: Vec<(String, i32)> = Vec::new();
        for (p, d) in &all_high {
            if is_hub_module_file(p) {
                hub_left_out.push((p.clone(), *d));
                continue;
            }
            let in_main = main_package
                .as_deref()
                .is_some_and(|m| package_of(p, &packages) == Some(m));
            if is_indicator_tagged_module_path(p) && !in_main {
                shared_left_out.push((p.clone(), *d));
                continue;
            }
            high.push((p.clone(), *d));
        }
        let indicator_filtered_count = total_above_threshold.saturating_sub(high.len());
        // Every file over the threshold was filtered: nothing to act on,
        // but say so rather than dropping the finding.
        let all_left_out = high.is_empty();
        let total_high = high.len();
        let was_capped = total_high > HIGH_FAN_IN_MAX_FINDINGS;
        let omitted_count = total_high.saturating_sub(HIGH_FAN_IN_MAX_FINDINGS);
        high.truncate(HIGH_FAN_IN_MAX_FINDINGS);

        // After the indicator filter, every remaining path is non-indicator
        // by construction. The repo-tag check still gates actionability:
        // compiler/infra workspaces are noisy by nature even after the
        // indicator filter.
        let core_or_compiler_repo = self.repo_context.has(&RepoContextTag::CompilerIndicators)
            || self
                .repo_context
                .has(&RepoContextTag::InfrastructureIndicators);
        let is_actionable = !core_or_compiler_repo && !all_left_out;

        let (classification, severity, action) = if is_actionable {
            (
                FindingClassification::Actionable,
                FindingSeverity::Medium,
                RecommendedAction::ConcreteAction {
                    steps: vec![
                        "For each listed file, check whether its broad inbound use is an \
                         intentional shared contract / utility (expected) or accidental coupling \
                         that makes the file risky to change."
                            .into(),
                        "Only if edits to it routinely ripple painfully across importers, \
                         consider narrowing its public API or splitting responsibilities. High \
                         fan-in alone is not a defect."
                            .into(),
                    ],
                },
            )
        } else {
            (
                FindingClassification::Expected,
                FindingSeverity::None,
                RecommendedAction::NoAction,
            )
        };

        let mut depends_on: Vec<String> = vec!["filter:nonIndicatorModule".into()];
        if core_or_compiler_repo {
            depends_on.push("match:repoIndicatorTag".into());
        }
        depends_on.push(format!(
            "threshold:graph.inboundEdges > {HIGH_FAN_IN_MULTIPLIER} * average"
        ));

        // Sample list mirrors the F-ORPHAN-FILES shape: an array of `{path,
        // inboundEdges}` records, plus a `sampleComplete` flag the agent can
        // check before assuming the list is exhaustive.
        let sample: Vec<serde_json::Value> = high
            .iter()
            .map(|(path, in_deg)| {
                let mut entry = serde_json::json!({
                    "path": path,
                    "inboundEdges": in_deg,
                });
                // #24: a Go import names a package, and its edges land on
                // one file standing for the package. Name the package.
                if let Some(pkg) = go_package_of(graph, path) {
                    entry["goPackage"] = serde_json::json!(pkg.name);
                    entry["importPath"] = serde_json::json!(pkg.import_path);
                    entry["importers"] = serde_json::json!(pkg.importers);
                }
                entry
            })
            .collect();
        let go_packages = sample
            .iter()
            .filter(|e| e.get("goPackage").is_some())
            .count();
        let all_go_packages = !high.is_empty() && go_packages == high.len();
        let mut inbound_value = serde_json::json!({
            "sample": sample,
            "sampleComplete": !was_capped,
        });
        if !shared_left_out.is_empty() {
            let left: Vec<serde_json::Value> = shared_left_out
                .iter()
                .take(HIGH_FAN_IN_MAX_FINDINGS)
                .map(|(path, in_deg)| serde_json::json!({ "path": path, "inboundEdges": in_deg }))
                .collect();
            inbound_value["sharedCrateFilesLeftOut"] = serde_json::json!({
                "count": shared_left_out.len(),
                "sample": left,
            });
        }
        if !hub_left_out.is_empty() {
            let left: Vec<serde_json::Value> = hub_left_out
                .iter()
                .take(HIGH_FAN_IN_MAX_FINDINGS)
                .map(|(path, in_deg)| serde_json::json!({ "path": path, "inboundEdges": in_deg }))
                .collect();
            inbound_value["moduleHubFilesLeftOut"] = serde_json::json!({
                "count": hub_left_out.len(),
                "sample": left,
            });
        }
        let scope_paths: Vec<String> = high.iter().map(|(p, _)| p.clone()).collect();
        let title = if all_left_out {
            format!(
                "Files with high inbound graph edges: none listed, {total_above_threshold} \
                 indicator-tagged file(s) left out"
            )
        } else if all_go_packages {
            format!(
                "Go packages with high inbound graph edges ({} package(s))",
                scope_paths.len()
            )
        } else {
            format!(
                "Files with high inbound graph edges ({} file(s))",
                scope_paths.len()
            )
        };
        let inference_statement = if all_left_out {
            format!(
                "{total_above_threshold} file(s) exceed the heuristic fan-in threshold \
                 ({HIGH_FAN_IN_MULTIPLIER}× the average inbound-edge count of {avg:.1}), and \
                 all of them are module hubs (lib.rs, mod.rs, prelude.rs) or files in shared \
                 *-core/*-util crates, so none is listed."
            )
        } else {
            format!(
                "{} file(s) exceed the heuristic fan-in threshold ({HIGH_FAN_IN_MULTIPLIER}× the \
                 average inbound-edge count of {avg:.1}).",
                scope_paths.len()
            )
        };

        // Threshold observation reports the worst (largest inbound) value
        // currently in the kept set so the HeuristicJudgment threshold
        // comparison has a meaningful `observedValue`.
        let kept_or_all = if all_left_out { &all_high } else { &high };
        let max_in_deg = kept_or_all.iter().map(|(_, d)| *d).max().unwrap_or(0);

        let mut limitations = vec![Limitation {
            code: limitation_codes::FAN_IN_THRESHOLD_HEURISTIC.into(),
            detail: format!(
                "The {HIGH_FAN_IN_MULTIPLIER}× average threshold is a heuristic over \
                 inbound-edge counts. Indicator-tagged hub modules (lib.rs, mod.rs, \
                 prelude, *-core/*-util workspace members) are filtered before \
                 emission because they legitimately have high fan-in by design."
            ),
        }];
        if go_packages > 0 {
            limitations.push(Limitation {
                code: limitation_codes::FAN_IN_THRESHOLD_HEURISTIC.into(),
                detail: "A Go import names a package, not a file: all of a package's import \
                         edges land on one file that stands for it, so a listed `.go` path \
                         stands for its whole package (`goPackage`, `importPath`, \
                         `importers`)."
                    .into(),
            });
        }
        if indicator_filtered_count > 0 {
            limitations.push(Limitation {
                code: limitation_codes::FAN_IN_THRESHOLD_HEURISTIC.into(),
                detail: format!(
                    "{indicator_filtered_count} indicator-tagged hub module(s) exceeded the \
                     threshold but were filtered out of this finding. They are not reported \
                     as fan-in concerns because their high inbound degree reflects \
                     intentional module-organisation patterns."
                ),
            });
        }
        if let Some((first, n)) = shared_left_out.first() {
            limitations.push(Limitation {
                code: limitation_codes::FAN_IN_THRESHOLD_HEURISTIC.into(),
                detail: format!(
                    "{} file(s) under shared *-core/*-util style crates exceeded the threshold \
                     but were left out of this list (most imported: {first}, {n}). Only the \
                     repo's largest crate is listed regardless of its name.",
                    shared_left_out.len()
                ),
            });
        }
        if was_capped {
            limitations.push(Limitation {
                code: limitation_codes::FAN_IN_FINDINGS_CAPPED.into(),
                detail: format!(
                    "Reported the top {HIGH_FAN_IN_MAX_FINDINGS} files by inbound degree; \
                     {omitted_count} additional non-indicator file(s) also exceeded the \
                     threshold but were not enumerated in the sample."
                ),
            });
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_HIGH_FAN_IN_FILE),
            title,
            classification,
            severity,
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 3 (2026-05-11): `graph.inboundEdges`/`graph.averageInboundEdges`
            // are backed by the graph-import-edges family (I-GRAPH-EDGE-CLOSURE,
            // I-GRAPH-NODE-ID-UNIQUE etc. catalogue, doc-06 §5 `confirmOnly`).
            // The 2× mean-multiplier threshold is a heuristic over verified
            // inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::GRAPH_INBOUND_EDGES.into(),
                    observed_value: inbound_value,
                    source_paths: scope_paths.clone(),
                    source_type: EvidenceSource::ImportGraph,
                    invariant_id: Some("I-GRAPH-EDGE-CLOSURE".into()),
                },
                EvidenceItem::Observation {
                    metric: metric_names::GRAPH_AVERAGE_INBOUND_EDGES.into(),
                    observed_value: serde_json::json!(avg),
                    source_paths: vec![],
                    source_type: EvidenceSource::ImportGraph,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "graph.inboundEdges > {HIGH_FAN_IN_MULTIPLIER} * graph.averageInboundEdges"
                    ),
                    observed_metric: metric_names::GRAPH_INBOUND_EDGES.into(),
                    observed_value: serde_json::json!(max_in_deg),
                    // The real cutoff (2 × 1.413 = 2.83), not the integer
                    // it is compared as: "> 2" reads as if 2 qualified (#26).
                    threshold: if shown_cutoff.fract() == 0.0 {
                        serde_json::json!(shown_cutoff as i64)
                    } else {
                        serde_json::json!(shown_cutoff)
                    },
                    rationale: Some(
                        "Heuristic threshold; not justified by an invariant. Indicator-tagged \
                         hub modules are filtered out before this judgment is applied so the \
                         reported paths are non-indicator."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: inference_statement,
                strength: InferenceStrength::Suggestive,
                depends_on,
                does_not_imply: vec![
                    "these files are poorly designed".into(),
                    "these files should be split".into(),
                ],
            },
            limitations,
            action,
            scope: self.scope(scope_paths, vec![]),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}

/// `lib.rs`, `mod.rs` and `prelude.rs`: module hubs whose fan-in reflects
/// how Rust modules are declared, not coupling.
fn is_hub_module_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "lib.rs"
        || lower.ends_with("/lib.rs")
        || lower.ends_with("/mod.rs")
        || lower.ends_with("/prelude.rs")
        || lower == "prelude.rs"
}

/// Package manifests that mark a crate / package directory.
const PACKAGE_MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "go.mod",
    "pyproject.toml",
    "setup.py",
];

/// Directories (repo-relative, "" for the root) holding a package manifest.
fn package_dirs(graph: &FileGraph) -> Vec<String> {
    let mut dirs: Vec<String> = graph
        .nodes
        .iter()
        .filter_map(|n| {
            let (dir, name) = n.id.rsplit_once('/').unwrap_or(("", n.id.as_str()));
            PACKAGE_MANIFESTS.contains(&name).then(|| dir.to_string())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// The innermost package directory containing `path`.
fn package_of<'d>(path: &str, dirs: &'d [String]) -> Option<&'d str> {
    dirs.iter()
        .filter(|d| d.is_empty() || path.starts_with(&format!("{d}/")))
        .max_by_key(|d| d.len())
        .map(String::as_str)
}

/// The package with the most code files, or `None` when there is no
/// single largest one.
fn largest_code_package(graph: &FileGraph, dirs: &[String]) -> Option<String> {
    use repolens_extract::{extraction_fidelity, ExtractionFidelity};
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for n in &graph.nodes {
        let lang = repolens_extract::lang::detect(std::path::Path::new(&n.id));
        if !matches!(
            extraction_fidelity(lang),
            ExtractionFidelity::Ast | ExtractionFidelity::Heuristic
        ) {
            continue;
        }
        if let Some(d) = package_of(&n.id, dirs) {
            *counts.entry(d).or_default() += 1;
        }
    }
    let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    match ranked.as_slice() {
        [(d, n), (_, m), ..] if n > m => Some(d.to_string()),
        [(d, _)] => Some(d.to_string()),
        _ => None,
    }
}

/// A Go package seen through the file its import edges land on.
struct GoPackage {
    name: String,
    import_path: String,
    importers: usize,
}

/// When `path` is a `.go` file that Go imports resolve to, the package it
/// stands for: its name (the import path's last segment, skipping a major
/// version suffix like `/v2`), its import path and how many files import it.
fn go_package_of(graph: &FileGraph, path: &str) -> Option<GoPackage> {
    if !path.ends_with(".go") {
        return None;
    }
    let mut import_path: Option<&str> = None;
    let mut importers: Vec<&str> = Vec::new();
    for e in &graph.edges {
        if e.target != path || e.edge_type != "imports" || !e.source.ends_with(".go") {
            continue;
        }
        let Some(detail) = e.detail.as_deref() else {
            continue;
        };
        import_path.get_or_insert(detail);
        importers.push(e.source.as_str());
    }
    let import_path = import_path?;
    importers.sort_unstable();
    importers.dedup();
    Some(GoPackage {
        name: go_package_name(import_path),
        import_path: import_path.to_string(),
        importers: importers.len(),
    })
}

/// The conventional package name for a Go import path.
fn go_package_name(import_path: &str) -> String {
    let mut segs = import_path.trim_end_matches('/').rsplit('/');
    let last = segs.next().unwrap_or(import_path);
    let is_major = |s: &str| {
        s.strip_prefix('v')
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    };
    let name = match segs.next() {
        Some(prev) if is_major(last) => prev,
        _ => last,
    };
    // gopkg.in/yaml.v3 → yaml
    let name = match name.rsplit_once(".v") {
        Some((stem, v)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => stem,
        _ => name,
    };
    name.to_string()
}

#[cfg(test)]
mod go_package_tests {
    use super::go_package_name;

    #[test]
    fn go_package_names_follow_the_import_path() {
        assert_eq!(go_package_name("github.com/spf13/cobra"), "cobra");
        assert_eq!(go_package_name("github.com/spf13/cobra/doc"), "doc");
        assert_eq!(go_package_name("github.com/a/b/v2"), "b");
        assert_eq!(go_package_name("gopkg.in/yaml.v3"), "yaml");
    }
}
