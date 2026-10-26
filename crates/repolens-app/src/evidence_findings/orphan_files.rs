use super::*;

/// F-ORPHAN-FILES title when every orphan is loaded by name or isn't code
/// and the whole orphan set was checked (#20). Same phrase plain view
/// prints for a zero count.
pub(crate) const ORPHAN_NONE_TITLE: &str = "No unused files found (imports only)";
/// F-ORPHAN-FILES title when the listed sample has no unused source file
/// but the rest of the orphans were not listed (#20).
pub(crate) const ORPHAN_NONE_IN_SAMPLE_TITLE: &str =
    "No unused files in the sample; the rest were not checked (imports only)";

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 2. F-ORPHAN-FILES
    // -----------------------------------------------------------------------
    pub(super) fn orphan_files(&self) -> Option<EvidenceFinding> {
        let gh = self.graph_health?;
        if gh.orphan_count == 0 {
            return None;
        }
        // `gh.orphan_files` is a sample (truncated to 10 by
        // `build_graph_health_input`); `gh.orphan_count` is the full count.
        let sample_filtered: Vec<String> = gh
            .orphan_files
            .iter()
            .filter(|p| is_orphan_source_candidate(p))
            .filter(|p| !self.referenced_files.is_some_and(|r| r.contains(*p)))
            .cloned()
            .collect();
        let sample_is_complete = gh.orphan_files.len() >= gh.orphan_count;

        // When the full graph is available, recompute orphans and apply
        // the same root-config/policy filter so the ratio reflects only
        // source orphans. This avoids classifying a repo as Actionable
        // based on docs/manifests that the finding itself says are
        // filtered. Keep the full filtered list so the finding can cite
        // actual source-orphan paths even when the original truncated
        // sample contained only configs.
        let full_filtered: Option<Vec<String>> = self.graph.map(|g| {
            use std::collections::HashSet;
            let mut entry_set: HashSet<String> = self
                .profile
                .entry_points
                .iter()
                .map(|ep| ep.path.clone())
                .collect();
            if let Some(r) = self.referenced_files {
                entry_set.extend(r.iter().cloned());
            }
            let report = crate::graph_analysis::GraphAnalyzer::find_orphans(g, &entry_set);
            report
                .orphan_files
                .into_iter()
                .filter(|p| is_orphan_source_candidate(p))
                .collect::<Vec<String>>()
        });
        let full_filtered_count: Option<usize> = full_filtered.as_ref().map(|v| v.len());
        let loaded_by_name = self.loaded_by_name_count();

        // The "config-only" branch only applies when:
        //   - the sample is empty of source paths, AND
        //   - the full-graph recomputation also found zero source orphans
        //     (or the full graph is not available, in which case we fall
        //     back to the sample evidence we have).
        // Without the second guard, a repo whose first 10 sorted orphans
        // are docs/configs but whose later orphans are real source files
        // would be misreported as config-only.
        let full_recompute_says_no_source = full_filtered_count.is_some_and(|n| n == 0);
        let no_full_recompute = full_filtered_count.is_none();
        if full_recompute_says_no_source || (sample_filtered.is_empty() && no_full_recompute) {
            // No source orphans observed in our sample. Behaviour depends
            // on whether the sample covers the entire orphan set:
            //   - sample complete: safe to claim every orphan is a
            //     filtered non-source artefact (root configs/policy, Cargo
            //     test/bench/example targets, build scripts, fixture data,
            //     insta snapshots, editor/agent config dirs, or a crate-
            //     root lib.rs library entry point).
            //   - sample truncated: cannot claim that — emit an
            //     Informational finding noting the sample is all
            //     non-source while the unsampled remainder is unknown.
            //
            // Wording must match the broadened `is_non_orphan_source` set
            // (codex P3, 2026-05-14) so callers that surface this branch
            // describe the observed paths accurately for the new filter
            // classes.
            // A full-graph recompute that found no source orphans covers
            // every orphan, however short the graph-health sample was.
            let covers_all = sample_is_complete || full_recompute_says_no_source;
            let (title, statement, depends_on) = if covers_all {
                (
                    ORPHAN_NONE_TITLE.to_string(),
                    "Every file that no other file imports is one that tools load by name \
                     or that isn't code: tests, build scripts, configs, docs, examples, \
                     fixtures, snapshots, or a crate-root lib.rs. None looks unused."
                        .to_string(),
                    vec![
                        "filter:nonOrphanSource".into(),
                        "sampleComplete=true".into(),
                    ],
                )
            } else {
                let n = gh.orphan_count;
                let k = gh.orphan_files.len();
                let r = n.saturating_sub(k);
                (
                    ORPHAN_NONE_IN_SAMPLE_TITLE.to_string(),
                    format!(
                        "{n} files are not imported by any other file. This scan lists {k} \
                         of them, and all {k} are files that tools load by name or that \
                         aren't code (tests, build scripts, configs, docs, examples, \
                         fixtures, snapshots, crate-root lib.rs). The other {r} were not \
                         listed, so some of them may be unused source files."
                    ),
                    vec![
                        "filter:nonOrphanSource".into(),
                        "sampleComplete=false".into(),
                    ],
                )
            };
            return Some(EvidenceFinding {
                id: FindingId::new(F_ORPHAN_FILES),
                title,
                classification: FindingClassification::Informational,
                severity: FindingSeverity::None,
                knowledge_layer: KnowledgeLayer::Observation,
                // Phase 3 (2026-05-11): `graph.orphanCount` is backed by the
                // orphan-detection family (I-GRAPH-ORPHANS-* catalogue,
                // doc-06 §5 `confirmOnly`). Observation branch (no
                // heuristic threshold fired) → `Derived`.
                evidence_grade: EvidenceGrade::Derived,
                confidence: EpistemicConfidence::Codified,
                evidence: vec![EvidenceItem::Observation {
                    metric: metric_names::GRAPH_ORPHAN_COUNT.into(),
                    // Same shape as the source-orphan branch when the full
                    // graph was checked, so `loadedByNameCount` is there too.
                    observed_value: match loaded_by_name {
                        Some(k) if full_recompute_says_no_source => serde_json::json!({
                            "graphOrphanCount": gh.orphan_count,
                            "sourceOrphanCount": 0,
                            "loadedByNameCount": k,
                        }),
                        _ => serde_json::json!(gh.orphan_count),
                    },
                    source_paths: gh.orphan_files.clone(),
                    source_type: EvidenceSource::ImportGraph,
                    invariant_id: Some("I-GRAPH-ORPHANS-RESPECT-ENTRY-POINTS".into()),
                }],
                inference: ResponsibleInference {
                    statement,
                    strength: if covers_all {
                        InferenceStrength::Strong
                    } else {
                        InferenceStrength::Suggestive
                    },
                    depends_on,
                    does_not_imply: vec![
                        "unused source code is present".into(),
                        "unused source code is absent".into(),
                    ],
                },
                limitations: vec![Limitation {
                    code: limitation_codes::DOCS_AND_CONFIG_NOT_TREATED_AS_SOURCE.into(),
                    detail: "Root manifests, policy docs, READMEs, CI config files, \
                             editor / agent config dirs, Cargo integration tests / \
                             benches / examples / build scripts, test files found by a \
                             test runner's naming rules (pytest, unittest, JUnit, go \
                             test, Jest/Vitest), tool configs, example and benchmark \
                             folders, fixture data files, insta snapshots, and crate-root lib.rs library entry points \
                             are not treated as source files for orphan analysis."
                        .into(),
                }],
                action: RecommendedAction::NoAction,
                scope: self.scope(vec![], vec![]),
                not_checked: vec![],
                repo_context: self.repo_context.tags.clone(),
                false_positive_notes: self.indicator_notes(),
                provenance: None,
            });
        }

        // Compute the orphan ratio. Numerator: the post-filter source-
        // orphan count when the full graph is available (so the ratio
        // reflects what the finding actually claims); fall back to
        // gh.orphan_count when the graph is not available, which is a
        // conservative upper bound. Denominator: the included file
        // count — scan.file_count (total entries) would dilute the ratio
        // in repos with many ignored or vendor files.
        let numerator = full_filtered_count.unwrap_or(gh.orphan_count);
        let total_included = self
            .scan
            .map(|s| s.included_count.max(1))
            .unwrap_or_else(|| gh.orphan_count.max(1) as i32);
        let orphan_ratio = numerator as f64 / total_included as f64;
        let exceeds_threshold = orphan_ratio > ORPHAN_RATIO_THRESHOLD;

        let (classification, severity, action) = if exceeds_threshold {
            (
                FindingClassification::Actionable,
                FindingSeverity::Low,
                RecommendedAction::ConcreteAction {
                    steps: vec![
                        "For each listed file, check whether it is reached by a path the import \
                         graph does not capture — a CLI/subcommand dispatch, a test/example/bench \
                         harness, a build script, conditional compilation (cfg), or dynamic / \
                         reflective loading — before treating it as unused."
                            .into(),
                        "Remove a file only after confirming it is genuinely unreachable; if it \
                         is an intentional entry point or seam, leave it. This finding does not \
                         assert the files are dead code."
                            .into(),
                    ],
                },
            )
        } else {
            (
                FindingClassification::Informational,
                FindingSeverity::None,
                RecommendedAction::NoAction,
            )
        };

        // The GRAPH_ORPHAN_COUNT observation reports BOTH the raw graph
        // count and the post-filter source-orphan count when computable,
        // so consumers can see exactly which number is driving the ratio.
        let orphan_count_value = match full_filtered_count {
            Some(filtered) => serde_json::json!({
                "graphOrphanCount": gh.orphan_count,
                "sourceOrphanCount": filtered,
                "loadedByNameCount": loaded_by_name.unwrap_or(0),
            }),
            None => serde_json::json!({
                "graphOrphanCount": gh.orphan_count,
                "sourceOrphanCount": null,
            }),
        };
        // Prefer the recomputed-from-full-graph filtered list when
        // available — the truncated sample may have contained only
        // configs and we don't want to emit an "Orphan source files
        // detected" finding with empty source_paths. Cap the listed
        // paths to keep the response manageable.
        const ORPHAN_PATH_LIMIT: usize = 25;
        let (paths_for_emit, paths_complete): (Vec<String>, bool) = match full_filtered.as_ref() {
            Some(full) if !full.is_empty() => {
                let listed: Vec<String> = full.iter().take(ORPHAN_PATH_LIMIT).cloned().collect();
                (listed, full.len() <= ORPHAN_PATH_LIMIT)
            }
            _ => (sample_filtered.clone(), sample_is_complete),
        };
        let mut evidence: Vec<EvidenceItem> = vec![
            EvidenceItem::Observation {
                metric: metric_names::GRAPH_ORPHAN_COUNT.into(),
                observed_value: orphan_count_value,
                source_paths: vec![],
                source_type: EvidenceSource::ImportGraph,
                invariant_id: Some("I-GRAPH-ORPHANS-RESPECT-ENTRY-POINTS".into()),
            },
            EvidenceItem::Observation {
                metric: metric_names::GRAPH_ORPHAN_FILES.into(),
                observed_value: serde_json::json!({
                    "sample": paths_for_emit,
                    "sampleComplete": paths_complete,
                }),
                source_paths: paths_for_emit.clone(),
                source_type: EvidenceSource::ImportGraph,
                invariant_id: None,
            },
        ];
        if exceeds_threshold {
            evidence.push(EvidenceItem::HeuristicJudgment {
                rule: format!("graph.orphanRatio > {ORPHAN_RATIO_THRESHOLD}"),
                observed_metric: metric_names::GRAPH_ORPHAN_RATIO.into(),
                observed_value: serde_json::json!(orphan_ratio),
                threshold: serde_json::json!(ORPHAN_RATIO_THRESHOLD),
                rationale: Some(
                    "Heuristic threshold; not justified by an invariant. Above the \
                     threshold the orphan list may indicate dead code or seam files \
                     worth reviewing."
                        .into(),
                ),
            });
        }

        let inference_statement = match (full_filtered_count, sample_is_complete) {
            (Some(filtered), _) => {
                let (files, verb, pronoun) = if filtered == 1 {
                    ("file", "has", "It")
                } else {
                    ("files", "have", "They")
                };
                let orphans = if gh.orphan_count == 1 {
                    "orphan"
                } else {
                    "orphans"
                };
                format!(
                    "{filtered} source {files} (of {} graph {orphans}) {verb} no \
                     inbound import edges (entry points excluded), not counting \
                     files that aren't source code (root manifests, policy docs, \
                     editor/agent configs, Cargo test/bench/example/build-script \
                     targets, test files a test runner finds by name, tool \
                     configs, example and benchmark folders, fixture data, insta \
                     snapshots, crate-root lib.rs entry points). {pronoun} may \
                     still import other modules.",
                    gh.orphan_count
                )
            }
            (None, true) => format!(
                "{} graph orphans have zero inbound structural import edges (entry points \
                 excluded). They may still import other modules.",
                gh.orphan_count
            ),
            (None, false) => format!(
                "{} graph orphans have zero inbound structural import edges (entry points \
                 excluded). The reported source-paths list is a sample of {}; the full set \
                 is not enumerated.",
                gh.orphan_count,
                sample_filtered.len()
            ),
        };

        // Must agree with the `graph.orphanFiles` evidence above (#26):
        // when the full graph was recomputed, completeness is that of the
        // listed paths, not of the 10-path graph-health sample.
        let mut depends_on = vec![
            "filter:nonOrphanSource".into(),
            format!("sampleComplete={paths_complete}"),
        ];
        if exceeds_threshold {
            depends_on.push(format!(
                "threshold:graph.orphanRatio>{ORPHAN_RATIO_THRESHOLD}"
            ));
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_ORPHAN_FILES),
            title: crate::evidence_findings::ORPHAN_TITLE.into(),
            classification,
            severity,
            knowledge_layer: if exceeds_threshold {
                KnowledgeLayer::Heuristic
            } else {
                KnowledgeLayer::Observation
            },
            // Phase 3 (2026-05-11): `graph.orphanCount`/`graph.orphanFiles` are
            // backed by the orphan-detection family (I-GRAPH-ORPHANS-* catalogue,
            // doc-06 §5 `confirmOnly`). The `graph.orphanRatio` threshold is a
            // heuristic over verified inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence,
            inference: ResponsibleInference {
                statement: inference_statement,
                strength: if exceeds_threshold {
                    InferenceStrength::Suggestive
                } else {
                    InferenceStrength::Strong
                },
                depends_on,
                does_not_imply: vec![
                    "these files are dead code".into(),
                    "these files can be deleted safely".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::DOCS_AND_CONFIG_NOT_TREATED_AS_SOURCE.into(),
                detail: "Root manifests, policy docs, READMEs, CI config files, \
                         editor / agent config dirs, Cargo integration tests / \
                         benches / examples / build scripts, test files found by a \
                         test runner's naming rules (pytest, unittest, JUnit, go \
                         test, Jest/Vitest), tool configs, example and benchmark \
                         folders, fixture data files, insta snapshots, and crate-root lib.rs library entry points \
                         are filtered before the orphan list is reported."
                    .into(),
            }],
            action,
            scope: self.scope(paths_for_emit.clone(), vec![]),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}

impl EvidenceFindingsBuilder<'_> {
    /// Code files nothing imports that the orphan list leaves out because a
    /// test runner, Cargo or another tool loads them by name: test files,
    /// examples, benches, tool configs, build scripts, `#[path]` modules,
    /// crate-root lib.rs, and scripts CI or HTML runs. Manifests, docs and
    /// fixture trees are not counted. `None` without the full graph.
    fn loaded_by_name_count(&self) -> Option<usize> {
        use repolens_core::path_classification as pc;
        let g = self.graph?;
        let entries: std::collections::HashSet<String> = self
            .profile
            .entry_points
            .iter()
            .map(|ep| ep.path.clone())
            .collect();
        let report = crate::graph_analysis::GraphAnalyzer::find_orphans(g, &entries);
        let referenced = |p: &str| self.referenced_files.is_some_and(|r| r.contains(p));
        Some(
            report
                .orphan_files
                .iter()
                .filter(|p| pc::is_import_graph_source(p))
                .filter(|p| {
                    referenced(p)
                        || (is_non_orphan_source(p)
                            && !pc::is_root_config_or_policy(p)
                            && !pc::is_license_file(p)
                            && !pc::is_test_fixture_path(p))
                })
                .count(),
        )
    }
}

/// A graph orphan worth listing: a source file RepoLens extracts imports
/// from (so "nothing imports it" means something) that is not a manifest,
/// policy/license file, runner target or library entry point.
fn is_orphan_source_candidate(path: &str) -> bool {
    !is_non_orphan_source(path) && repolens_core::path_classification::is_import_graph_source(path)
}
