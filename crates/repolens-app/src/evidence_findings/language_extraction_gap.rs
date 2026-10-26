use super::*;
use repolens_extract::{extraction_fidelity, ExtractionFidelity};

/// What RepoLens does and does not read for a regex-extracted language
/// (Python). Shown as evidence in F-LANGUAGE-EXTRACTION-GAP and as a line
/// under that finding in `init` and `findings`.
pub fn partial_extraction_note(language: &str) -> String {
    format!("{language}: imports read; cross-file calls and types not yet")
}

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // F-LANGUAGE-EXTRACTION-GAP
    // -----------------------------------------------------------------------
    /// Discloses the languages present in the scan that RepoLens extracts
    /// with reduced fidelity (regex) or not at all (no extractor), so an agent
    /// knows the symbol list and import graph are partial or absent there.
    ///
    /// - `Unsupported` languages (Shell, C, Ruby, ...): no extractor, so
    ///   imports aren't read at all.
    /// - `Heuristic` languages (Python): imports are read and resolved, but
    ///   calls and type references between files are not extracted. Listed
    ///   with [`partial_extraction_note`] (never as "imports aren't read"),
    ///   and only when the repo has at least two files in that language: a
    ///   single file has no cross-file calls or types to miss.
    ///
    /// A repo whose source is entirely AST-extracted (or only data/markup)
    /// emits nothing. This is an observation-grade disclosure of RepoLens's
    /// own capability, not a heuristic judgment about the repo.
    pub(super) fn language_extraction_gap(&self) -> Option<EvidenceFinding> {
        // `language_distribution` maps language name -> LOC (see
        // ProjectProfile::compute_language_distribution); it already excludes
        // "Unknown" and empty languages. BTreeMap iteration is key-sorted, so
        // each tier vec comes out alphabetical and deterministic.
        let mut ast: Vec<String> = Vec::new();
        let mut ast_loc: i64 = 0;
        let mut heuristic: Vec<String> = Vec::new();
        let mut heuristic_loc: i64 = 0;
        let mut unsupported: Vec<String> = Vec::new();
        let mut unsupported_loc: i64 = 0;
        for (lang, &loc) in &self.profile.language_distribution {
            match extraction_fidelity(lang) {
                ExtractionFidelity::Ast => {
                    ast.push(lang.clone());
                    ast_loc += loc as i64;
                }
                // Regex-extracted (Python): imports are read and resolved;
                // cross-file calls and types are not. Listed only when there
                // is more than one file for them to connect.
                ExtractionFidelity::Heuristic => {
                    if self.file_count_for_language(lang).is_none_or(|n| n >= 2) {
                        heuristic.push(lang.clone());
                        heuristic_loc += loc as i64;
                    }
                }
                ExtractionFidelity::Unsupported => {
                    unsupported.push(lang.clone());
                    unsupported_loc += loc as i64;
                }
                ExtractionFidelity::NotApplicable => {}
            }
        }

        // No gap: every source language is fully (AST) extracted, or the
        // only regex-extracted language has a single file.
        if heuristic.is_empty() && unsupported.is_empty() {
            return None;
        }

        let affected_loc = heuristic_loc + unsupported_loc;
        let affected_count = heuristic.len() + unsupported.len();
        let mut affected_langs: Vec<String> = heuristic
            .iter()
            .chain(unsupported.iter())
            .cloned()
            .collect();
        affected_langs.sort();
        let notes: Vec<String> = heuristic
            .iter()
            .map(|l| partial_extraction_note(l))
            .collect();

        // Evidence value: per-tier breakdown over the languages present in the
        // scan. AST is included for context (what *is* fully extracted).
        let tier =
            |langs: &[String], loc: i64| serde_json::json!({ "loc": loc, "languages": langs });
        let mut breakdown = serde_json::Map::new();
        if !ast.is_empty() {
            breakdown.insert("ast".into(), tier(&ast, ast_loc));
        }
        if !heuristic.is_empty() {
            let mut t = tier(&heuristic, heuristic_loc);
            t["notes"] = serde_json::json!(notes);
            breakdown.insert("heuristic".into(), t);
        }
        if !unsupported.is_empty() {
            breakdown.insert("unsupported".into(), tier(&unsupported, unsupported_loc));
        }

        // Inference statement, assembled from whichever tiers are non-empty.
        let mut sentences: Vec<String> = Vec::new();
        if !unsupported.is_empty() {
            sentences.push(format!(
                "RepoLens has no symbol extractor for {} language(s) ({} LOC: {}); symbols and \
                 import edges for those files are absent.",
                unsupported.len(),
                unsupported_loc,
                unsupported.join(", ")
            ));
        }
        if !heuristic.is_empty() {
            sentences.push(format!(
                "RepoLens extracts {} with regex only ({} LOC): imports are read and \
                 resolved; calls and type references between files are not extracted yet.",
                heuristic.join(", "),
                heuristic_loc
            ));
        }
        let statement = sentences.join(" ");

        let steps = vec![
            format!(
                "Treat RepoLens's symbol list and import graph as incomplete for these \
                 languages: {}. Unsupported languages contribute no symbols and no import \
                 edges; regex-extracted languages contribute imports and top-level symbols \
                 but no cross-file call or type edges.",
                affected_langs.join(", ")
            ),
            "Before relying on call or dependency structure for these files, read the source \
             directly (e.g. read_file_content) or use search_code instead of the import graph."
                .to_string(),
        ];

        Some(EvidenceFinding {
            id: FindingId::new(F_LANGUAGE_EXTRACTION_GAP),
            // Observation-shaped title; must avoid the e2e FORBIDDEN_TITLE_TOKENS
            // ("coverage", "fitness", "quality", "readiness", "risk level").
            title: format!(
                "Source files in languages without full symbol extraction \
                 ({affected_count} language(s), {affected_loc} LOC)"
            ),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            knowledge_layer: KnowledgeLayer::Observation,
            // The fidelity tiers are facts about RepoLens's extractor set; the
            // per-tier LOC are observed from the project profile → Derived.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::EXTRACTION_LANGUAGES_BY_FIDELITY.into(),
                observed_value: serde_json::Value::Object(breakdown),
                source_paths: vec![],
                source_type: EvidenceSource::ProjectProfile,
                invariant_id: None,
            }],
            inference: ResponsibleInference {
                statement,
                strength: InferenceStrength::Strong,
                depends_on: vec![
                    "extractorRegistry=Java,Rust,Go,JavaScript,TypeScript,Python".into(),
                    "fidelity:repolens_extract::extraction_fidelity".into(),
                ],
                does_not_imply: vec![
                    "these files are unimportant or dead".into(),
                    "these files lack structure".into(),
                    "RepoLens failed to scan these files".into(),
                    "the symbol list for supported languages is complete".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::SYMBOL_EXTRACTION_LANGUAGE_LIMITED.into(),
                detail: "RepoLens extracts symbols/relationships for Java, Rust, Go, JavaScript, \
                         and TypeScript via full parsing and Python via regex. Other recognized \
                         programming languages have no extractor; their files are inventoried and \
                         appear as graph nodes but contribute no symbols or edges. Counts are LOC, \
                         not file counts."
                    .into(),
            }],
            action: RecommendedAction::ConcreteAction { steps },
            scope: self.scope(vec![], affected_langs),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }

    /// Files in `language` among the graph nodes, or `None` without a graph.
    fn file_count_for_language(&self, language: &str) -> Option<usize> {
        let g = self.graph?;
        Some(
            g.nodes
                .iter()
                .filter(|n| repolens_extract::lang::detect(std::path::Path::new(&n.id)) == language)
                .count(),
        )
    }
}
