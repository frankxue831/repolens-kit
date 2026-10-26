//! MCP-side helpers that wrap the epistemic envelope types from
//! `repolens-core::model::epistemic`. The pure-serde envelope types live in
//! core so that core models (notably `EvidenceFinding`) can carry per-finding
//! grades without a dependency cycle. The helpers below stay here because
//! they return `rmcp::CallToolResult`, which is MCP-specific.
//!
//! Wire format and JSON keys are unchanged from the previous in-mcp module.

use rmcp::{model::CallToolResult, ErrorData as McpError};

pub use repolens_core::model::epistemic::{
    pack_id_from_dir, EpistemicConfidence, EvidenceGrade, KnowledgeLayer, KnowledgeLayerField,
    Provenance, ProvenanceField, ToolResponse,
};
use repolens_core::model::evidence::EvidenceFinding;

// ---------------------------------------------------------------------------
// Envelope grade computation for mixed-content tools
// ---------------------------------------------------------------------------

/// Numeric rank for `EvidenceGrade` ordering: lower number = lower grade.
/// `Unaudited < Derived < Verified`.
fn grade_rank(g: &EvidenceGrade) -> u8 {
    match g {
        EvidenceGrade::Unaudited => 0,
        EvidenceGrade::Derived => 1,
        EvidenceGrade::Verified => 2,
    }
}

/// Compute the envelope-level evidence grade for a tool whose response
/// embeds per-finding grades.
///
/// The contract — see `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md`
/// §"Phase 4B Step 4" — is that the envelope grade reflects the lowest
/// grade represented in the substantive payload. Concretely:
///
/// * `candidate` is the grade the tool would emit if everything in the
///   payload were catalogue-backed at that grade.
/// * Each per-finding `evidence_grade` in `findings` is folded in by `min`.
/// * If `has_ungraded_interpretation` is `true` (e.g., `get_summary` has
///   surfaced memory insights, which the doc 06 §5 catalogue does not
///   grade), the envelope falls back to `Unaudited` regardless of the
///   per-finding minimum — ungraded interpretation cannot be implicitly
///   promoted to `Derived`.
///
/// `find_dead_code` has a similar pattern hand-rolled at its call site
/// (gating on whether `graph.json` was loaded); this helper centralises the
/// pattern for tools that gate on the contents of `evidence_findings`.
pub fn min_envelope_grade(
    candidate: EvidenceGrade,
    findings: &[EvidenceFinding],
    has_ungraded_interpretation: bool,
) -> EvidenceGrade {
    if has_ungraded_interpretation {
        return EvidenceGrade::Unaudited;
    }
    let mut min = candidate;
    for f in findings {
        if grade_rank(&f.evidence_grade) < grade_rank(&min) {
            min = f.evidence_grade.clone();
        }
    }
    min
}

// ---------------------------------------------------------------------------
// Helper — replaces ok_json() for epistemic-aware responses
// ---------------------------------------------------------------------------

/// Default-grade response helper. Attaches `EvidenceGrade::Unaudited` —
/// the safe default when the tool's observation family has not yet been
/// cited against a rubric-conforming catalogue from doc 06 §5.
///
/// To emit `EvidenceGrade::Verified` or `::Derived`, use
/// [`ok_epistemic_graded`] and name the catalogued invariant IDs in a
/// code comment at the call site.
pub fn ok_epistemic(
    layer: KnowledgeLayerField,
    confidence: EpistemicConfidence,
    provenance: ProvenanceField,
    data: serde_json::Value,
) -> Result<CallToolResult, McpError> {
    ok_epistemic_graded(
        layer,
        confidence,
        EvidenceGrade::default(),
        provenance,
        data,
    )
}

/// Explicit-grade response helper. Use this to promote a tool out of the
/// unaudited default once its observation families have named invariant
/// catalogues. Adoption of this helper is the observable, greppable code
/// change that moves a tool from `evidenceGrade: unaudited` to
/// `verified` / `derived`.
pub fn ok_epistemic_graded(
    layer: KnowledgeLayerField,
    confidence: EpistemicConfidence,
    evidence_grade: EvidenceGrade,
    provenance: ProvenanceField,
    data: serde_json::Value,
) -> Result<CallToolResult, McpError> {
    let response = ToolResponse {
        knowledge_layer: layer,
        confidence,
        evidence_grade,
        provenance,
        data,
    };
    let text = serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string());
    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
        text,
    )]))
}

// ---------------------------------------------------------------------------
// Tests — only the rmcp-side helpers are exercised here. Pure-serde
// envelope tests live alongside the types in `repolens-core`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_epistemic_defaults_evidence_grade_to_unaudited() {
        // The 4-argument helper is the shim that preserves backwards
        // compatibility for tool handlers. Tools must not silently become
        // verified; they must opt in via `ok_epistemic_graded`.
        let result = ok_epistemic(
            KnowledgeLayerField::Single(KnowledgeLayer::Observation),
            EpistemicConfidence::Mechanical,
            ProvenanceField::Single(Provenance::RepositoryRead {
                source: ".repolens/".into(),
            }),
            json!({"test": "value"}),
        )
        .unwrap();

        let text = match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => t.text.as_str(),
            _ => panic!("expected text content"),
        };
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["evidenceGrade"], "unaudited");
    }

    fn finding_at_grade(id: &str, grade: EvidenceGrade) -> EvidenceFinding {
        use repolens_core::model::evidence::{
            EvidenceScope, FindingClassification, FindingId, FindingSeverity, InferenceStrength,
            RecommendedAction, ResponsibleInference, ScopeTime,
        };
        EvidenceFinding {
            id: FindingId::new(id),
            title: id.into(),
            classification: FindingClassification::Expected,
            severity: FindingSeverity::None,
            knowledge_layer: KnowledgeLayer::Observation,
            evidence_grade: grade,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![],
            inference: ResponsibleInference {
                statement: "n/a".into(),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![],
                does_not_imply: vec![],
            },
            limitations: vec![],
            action: RecommendedAction::NoAction,
            scope: EvidenceScope {
                paths: vec![],
                languages: vec![],
                time: ScopeTime {
                    pack_id: "pack-x".into(),
                    commit: None,
                },
            },
            not_checked: vec![],
            repo_context: vec![],
            false_positive_notes: vec![],
            provenance: None,
        }
    }

    #[test]
    fn min_envelope_grade_returns_candidate_when_no_findings() {
        let grade = min_envelope_grade(EvidenceGrade::Derived, &[], false);
        assert_eq!(grade, EvidenceGrade::Derived);
    }

    #[test]
    fn min_envelope_grade_falls_back_to_unaudited_finding() {
        // Mirrors the F-LOCKFILE-MISSING-NON-CARGO case: a single unaudited
        // finding inside an otherwise-derived response must drag the
        // envelope down to `unaudited` so the trust contract holds.
        let findings = vec![
            finding_at_grade("F-A", EvidenceGrade::Derived),
            finding_at_grade("F-B", EvidenceGrade::Unaudited),
        ];
        let grade = min_envelope_grade(EvidenceGrade::Derived, &findings, false);
        assert_eq!(grade, EvidenceGrade::Unaudited);
    }

    #[test]
    fn min_envelope_grade_keeps_candidate_when_all_findings_higher() {
        let findings = vec![
            finding_at_grade("F-A", EvidenceGrade::Derived),
            finding_at_grade("F-B", EvidenceGrade::Verified),
        ];
        let grade = min_envelope_grade(EvidenceGrade::Derived, &findings, false);
        assert_eq!(grade, EvidenceGrade::Derived);
    }

    #[test]
    fn min_envelope_grade_forces_unaudited_when_ungraded_interpretation_present() {
        // get_summary's memory-insights path: even if every per-finding
        // grade is Derived, surfacing ungraded interpretation content must
        // pin the envelope at Unaudited.
        let findings = vec![finding_at_grade("F-A", EvidenceGrade::Derived)];
        let grade = min_envelope_grade(EvidenceGrade::Derived, &findings, true);
        assert_eq!(grade, EvidenceGrade::Unaudited);
    }

    #[test]
    fn ok_epistemic_graded_emits_explicit_grade() {
        // The 5-argument helper is the only path to `verified` or `derived`.
        // Promoting a tool is therefore a visible code change that a
        // reviewer can spot via `grep -n ok_epistemic_graded`.
        let result = ok_epistemic_graded(
            KnowledgeLayerField::Single(KnowledgeLayer::Observation),
            EpistemicConfidence::Mechanical,
            EvidenceGrade::Verified,
            ProvenanceField::Single(Provenance::PackExtraction {
                pack_id: "pack-20260414".into(),
                source: "files-0001.json".into(),
            }),
            json!({"test": "value"}),
        )
        .unwrap();

        let text = match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => t.text.as_str(),
            _ => panic!("expected text content"),
        };
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["evidenceGrade"], "verified");
    }
}
