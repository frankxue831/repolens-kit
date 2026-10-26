//! Epistemic envelope types. Lifted from `repolens-mcp::epistemic` so that
//! `repolens-core` types (notably `EvidenceFinding`) can carry per-finding
//! grades without a dependency cycle.
//!
//! Wire format is unchanged: enums still serialize as camelCase strings
//! (`"observation"`, `"mechanical"`, `"verified"`, …) and `ToolResponse`
//! still emits the same JSON keys (`knowledgeLayer`, `confidence`,
//! `evidenceGrade`, `provenance`, `data`).
//!
//! The previous `Confidence` enum is renamed to `EpistemicConfidence` here
//! to keep the namespace clear of the planned `MemoryConfidence` concept
//! in `repolens-memory`. The serialized values are unchanged. The JSON
//! field name on `ToolResponse` stays `confidence`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Knowledge-layer enums
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeLayer {
    Observation,
    Heuristic,
    Interpretation,
}

/// Grade of confidence in the producer's track record, expressed as one of
/// three discrete buckets. Distinct from `EvidenceGrade` (which speaks to
/// invariant coverage) and from any future memory-confidence concept.
///
/// Renamed from `Confidence` during the move to `repolens-core` so that
/// `repolens-memory`'s planned per-memory confidence concept has unambiguous
/// space. The `confidence` field on `ToolResponse` keeps its name; only the
/// Rust type name changes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EpistemicConfidence {
    Mechanical,
    Codified,
    Variable,
}

/// Claim made by the invariant suite about the producer's track record.
/// Orthogonal to `KnowledgeLayer` and `EpistemicConfidence`. See
/// `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` §7
/// and the "Evidence Grade" section of the envelope spec amendment
/// (`docs/superpowers/specs/2026-04-11-epistemic-envelope-design.md`).
///
/// Defaults to `Unaudited` — the "no invariant suite cited" fallback that
/// implements doc 06 §3's "default posture = excluded" rule at response
/// construction time. Promoting a tool out of the default requires an
/// explicit `ok_epistemic_graded(..., EvidenceGrade::Verified)` call.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceGrade {
    Verified,
    Derived,
    #[default]
    Unaudited,
}

// ---------------------------------------------------------------------------
// Provenance — tagged union
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Provenance {
    PackExtraction {
        #[serde(rename = "packId")]
        pack_id: String,
        source: String,
    },
    RepositoryRead {
        source: String,
    },
    HeuristicRule {
        rule: String,
        threshold: String,
        #[serde(rename = "packId", skip_serializing_if = "Option::is_none", default)]
        pack_id: Option<String>,
    },
    MemoryNode {
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(
            rename = "decayImportance",
            skip_serializing_if = "Option::is_none",
            default
        )]
        decay_importance: Option<f32>,
        #[serde(
            rename = "groundingStatus",
            skip_serializing_if = "Option::is_none",
            default
        )]
        grounding_status: Option<String>,
    },
    MemoryStore {
        source: String,
        #[serde(rename = "nodeIds", default, skip_serializing_if = "Vec::is_empty")]
        node_ids: Vec<String>,
    },
    WikiArticle {
        #[serde(rename = "articleId")]
        article_id: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        staleness: Option<String>,
    },
    WikiIndex {
        source: String,
        #[serde(rename = "articleIds", default, skip_serializing_if = "Vec::is_empty")]
        article_ids: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Wrapper types — serialize as single value or array
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum KnowledgeLayerField {
    Single(KnowledgeLayer),
    Multiple(Vec<KnowledgeLayer>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ProvenanceField {
    Single(Provenance),
    Multiple(Vec<Provenance>),
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResponse {
    pub knowledge_layer: KnowledgeLayerField,
    pub confidence: EpistemicConfidence,
    pub evidence_grade: EvidenceGrade,
    pub provenance: ProvenanceField,
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Convenience helpers
// ---------------------------------------------------------------------------

/// Extract a pack identifier from a pack directory path (e.g. "pack-20260411-120000").
pub fn pack_id_from_dir(pack_dir: &std::path::Path) -> String {
    pack_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knowledge_layer_serializes_camel_case() {
        assert_eq!(
            serde_json::to_value(KnowledgeLayer::Observation).unwrap(),
            json!("observation")
        );
        assert_eq!(
            serde_json::to_value(KnowledgeLayer::Heuristic).unwrap(),
            json!("heuristic")
        );
        assert_eq!(
            serde_json::to_value(KnowledgeLayer::Interpretation).unwrap(),
            json!("interpretation")
        );
    }

    #[test]
    fn epistemic_confidence_serializes_camel_case() {
        assert_eq!(
            serde_json::to_value(EpistemicConfidence::Mechanical).unwrap(),
            json!("mechanical")
        );
        assert_eq!(
            serde_json::to_value(EpistemicConfidence::Codified).unwrap(),
            json!("codified")
        );
        assert_eq!(
            serde_json::to_value(EpistemicConfidence::Variable).unwrap(),
            json!("variable")
        );
    }

    #[test]
    fn provenance_pack_extraction_tagged() {
        let p = Provenance::PackExtraction {
            pack_id: "pack-20260411".into(),
            source: "graph.json".into(),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["type"], "packExtraction");
        assert_eq!(v["packId"], "pack-20260411");
        assert_eq!(v["source"], "graph.json");
    }

    #[test]
    fn provenance_repository_read_tagged() {
        let p = Provenance::RepositoryRead {
            source: ".repolens/".into(),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["type"], "repositoryRead");
        assert_eq!(v["source"], ".repolens/");
    }

    #[test]
    fn provenance_heuristic_rule_optional_pack_id() {
        let with = Provenance::HeuristicRule {
            rule: "healthAnalysis".into(),
            threshold: ">80%".into(),
            pack_id: Some("pack-123".into()),
        };
        let v = serde_json::to_value(&with).unwrap();
        assert_eq!(v["type"], "heuristicRule");
        assert_eq!(v["packId"], "pack-123");

        let without = Provenance::HeuristicRule {
            rule: "healthAnalysis".into(),
            threshold: ">80%".into(),
            pack_id: None,
        };
        let v = serde_json::to_value(&without).unwrap();
        assert!(v.get("packId").is_none());
    }

    #[test]
    fn provenance_memory_node_optional_fields() {
        let full = Provenance::MemoryNode {
            node_id: "abc-123".into(),
            decay_importance: Some(0.72),
            grounding_status: Some("confirmed".into()),
        };
        let v = serde_json::to_value(&full).unwrap();
        assert_eq!(v["type"], "memoryNode");
        assert_eq!(v["nodeId"], "abc-123");
        assert_eq!(v["decayImportance"], 0.72);
        assert_eq!(v["groundingStatus"], "confirmed");

        let minimal = Provenance::MemoryNode {
            node_id: "abc-123".into(),
            decay_importance: None,
            grounding_status: None,
        };
        let v = serde_json::to_value(&minimal).unwrap();
        assert!(v.get("decayImportance").is_none());
        assert!(v.get("groundingStatus").is_none());
    }

    #[test]
    fn provenance_memory_store_omits_empty_ids() {
        let empty = Provenance::MemoryStore {
            source: "memory.json".into(),
            node_ids: vec![],
        };
        let v = serde_json::to_value(&empty).unwrap();
        assert!(v.get("nodeIds").is_none());

        let with_ids = Provenance::MemoryStore {
            source: "memory.json".into(),
            node_ids: vec!["a".into(), "b".into()],
        };
        let v = serde_json::to_value(&with_ids).unwrap();
        assert_eq!(v["nodeIds"], json!(["a", "b"]));
    }

    #[test]
    fn provenance_wiki_article_optional_staleness() {
        let with = Provenance::WikiArticle {
            article_id: "def-456".into(),
            staleness: Some("fresh".into()),
        };
        let v = serde_json::to_value(&with).unwrap();
        assert_eq!(v["type"], "wikiArticle");
        assert_eq!(v["staleness"], "fresh");

        let without = Provenance::WikiArticle {
            article_id: "def-456".into(),
            staleness: None,
        };
        let v = serde_json::to_value(&without).unwrap();
        assert!(v.get("staleness").is_none());
    }

    #[test]
    fn provenance_wiki_index_omits_empty_ids() {
        let empty = Provenance::WikiIndex {
            source: "wiki-index.json".into(),
            article_ids: vec![],
        };
        let v = serde_json::to_value(&empty).unwrap();
        assert!(v.get("articleIds").is_none());

        let with_ids = Provenance::WikiIndex {
            source: "wiki-index.json".into(),
            article_ids: vec!["x".into()],
        };
        let v = serde_json::to_value(&with_ids).unwrap();
        assert_eq!(v["articleIds"], json!(["x"]));
    }

    #[test]
    fn layer_field_single_serializes_as_string() {
        let field = KnowledgeLayerField::Single(KnowledgeLayer::Observation);
        assert_eq!(serde_json::to_value(&field).unwrap(), json!("observation"));
    }

    #[test]
    fn layer_field_multiple_serializes_as_array() {
        let field = KnowledgeLayerField::Multiple(vec![
            KnowledgeLayer::Observation,
            KnowledgeLayer::Heuristic,
        ]);
        assert_eq!(
            serde_json::to_value(&field).unwrap(),
            json!(["observation", "heuristic"])
        );
    }

    #[test]
    fn provenance_field_single_serializes_as_object() {
        let field = ProvenanceField::Single(Provenance::RepositoryRead {
            source: ".repolens/".into(),
        });
        let v = serde_json::to_value(&field).unwrap();
        assert_eq!(v["type"], "repositoryRead");
    }

    #[test]
    fn provenance_field_multiple_serializes_as_array() {
        let field = ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: "p1".into(),
                source: "graph.json".into(),
            },
            Provenance::HeuristicRule {
                rule: "health".into(),
                threshold: "multiple".into(),
                pack_id: Some("p1".into()),
            },
        ]);
        let v = serde_json::to_value(&field).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[0]["type"], "packExtraction");
        assert_eq!(v[1]["type"], "heuristicRule");
    }

    #[test]
    fn tool_response_full_envelope() {
        let response = ToolResponse {
            knowledge_layer: KnowledgeLayerField::Multiple(vec![
                KnowledgeLayer::Observation,
                KnowledgeLayer::Heuristic,
            ]),
            confidence: EpistemicConfidence::Codified,
            evidence_grade: EvidenceGrade::Derived,
            provenance: ProvenanceField::Multiple(vec![
                Provenance::PackExtraction {
                    pack_id: "pack-20260411".into(),
                    source: "profile.json".into(),
                },
                Provenance::HeuristicRule {
                    rule: "projectProfile".into(),
                    threshold: "multiple".into(),
                    pack_id: Some("pack-20260411".into()),
                },
            ]),
            data: json!({"fileCount": 150}),
        };
        let v = serde_json::to_value(&response).unwrap();

        // Envelope JSON keys are unchanged.
        assert_eq!(v["knowledgeLayer"], json!(["observation", "heuristic"]));
        assert_eq!(v["confidence"], "codified");
        assert_eq!(v["evidenceGrade"], "derived");
        assert!(v["provenance"].is_array());
        assert_eq!(v["data"]["fileCount"], 150);
    }

    #[test]
    fn tool_response_single_layer_envelope() {
        let response = ToolResponse {
            knowledge_layer: KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
            confidence: EpistemicConfidence::Variable,
            evidence_grade: EvidenceGrade::Unaudited,
            provenance: ProvenanceField::Single(Provenance::MemoryNode {
                node_id: "abc".into(),
                decay_importance: Some(0.8),
                grounding_status: None,
            }),
            data: json!({"content": "test insight"}),
        };
        let v = serde_json::to_value(&response).unwrap();

        assert_eq!(v["knowledgeLayer"], "interpretation");
        assert_eq!(v["confidence"], "variable");
        assert_eq!(v["evidenceGrade"], "unaudited");
        assert_eq!(v["provenance"]["type"], "memoryNode");
        assert_eq!(v["provenance"]["nodeId"], "abc");
        assert!(v["provenance"].get("groundingStatus").is_none());
        assert_eq!(v["data"]["content"], "test insight");
    }

    #[test]
    fn evidence_grade_serializes_camel_case() {
        assert_eq!(
            serde_json::to_value(EvidenceGrade::Verified).unwrap(),
            json!("verified")
        );
        assert_eq!(
            serde_json::to_value(EvidenceGrade::Derived).unwrap(),
            json!("derived")
        );
        assert_eq!(
            serde_json::to_value(EvidenceGrade::Unaudited).unwrap(),
            json!("unaudited")
        );
    }

    #[test]
    fn evidence_grade_default_is_unaudited() {
        // Doc 06 §3 "default posture = excluded" is implemented at the type
        // level. `EvidenceGrade::default()` must be `Unaudited`, and any code
        // path that doesn't explicitly name a grade must land on `unaudited`.
        assert_eq!(EvidenceGrade::default(), EvidenceGrade::Unaudited);
    }

    #[test]
    fn pack_id_from_dir_extracts_filename() {
        use std::path::PathBuf;
        let p = PathBuf::from("/abs/path/.repolens/pack-20260411-120000");
        assert_eq!(pack_id_from_dir(&p), "pack-20260411-120000");
    }
}
