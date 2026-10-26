//! End-to-end test for the evidence-aware finding pipeline:
//! create a small Rust library fixture, run a real scan, then call
//! `SummaryService::summarize` on the resulting pack and assert that the
//! findings are well-formed and free of Phase-1 forbidden language.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md`.

use std::fs;
use std::path::Path;

use repolens_app::evidence_findings;
use repolens_app::scan::ScanService;
use repolens_app::summary::{SummaryMemoryTier, SummaryService};
use repolens_core::model::evidence::{
    EvidenceItem, FindingClassification, FindingSeverity, RecommendedAction, RepoContextTag,
};

const FORBIDDEN_TITLE_TOKENS: &[&str] =
    &["coverage", "fitness", "quality", "readiness", "risk level"];

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn make_rust_library_fixture(root: &Path) {
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lib]\npath = \"src/lib.rs\"\n",
    );
    write(
        root,
        "src/lib.rs",
        "pub mod a;\npub mod b;\n\npub fn entry() -> &'static str { \"hello\" }\n",
    );
    write(
        root,
        "src/a.rs",
        "use crate::b;\npub fn a() -> &'static str { b::b() }\n",
    );
    write(root, "src/b.rs", "pub fn b() -> &'static str { \"b\" }\n");
    // Root manifests / docs that previously polluted dead-code/orphan claims.
    write(root, "README.md", "# Demo\n\nSome docs.\n");
    write(root, "LICENSE", "MIT\n");
    write(root, ".gitignore", "target/\n");
    // A genuine source orphan to exercise F-ORPHAN-FILES.
    write(root, "src/orphan_module.rs", "pub fn orphan() {}\n");
}

#[test]
fn rust_library_fixture_emits_well_formed_findings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    make_rust_library_fixture(root);

    let result = ScanService::new().run(root, false, 100).unwrap();
    let pack_dir = &result.pack_dir;

    let summary =
        SummaryService::summarize(pack_dir, Some(root), SummaryMemoryTier::Compact).unwrap();

    let findings = &summary.evidence_findings;
    assert!(
        !findings.is_empty(),
        "summary should emit at least one evidence finding for a non-trivial fixture"
    );

    for f in findings {
        // Title hygiene — no forbidden tokens.
        let title_lc = f.title.to_ascii_lowercase();
        for tok in FORBIDDEN_TITLE_TOKENS {
            assert!(
                !title_lc.contains(tok),
                "{}: title contains forbidden token '{tok}': '{}'",
                f.id.as_str(),
                f.title
            );
        }
        // Four-part shape: evidence + inference always populated.
        assert!(
            !f.evidence.is_empty(),
            "{}: evidence must be non-empty",
            f.id.as_str()
        );
        assert!(
            !f.inference.statement.is_empty(),
            "{}: inference statement must be non-empty",
            f.id.as_str()
        );
        // For NoAction findings, does_not_imply must be non-empty
        // (anti-overclaim guardrail in the spec).
        if matches!(f.action, RecommendedAction::NoAction) {
            assert!(
                !f.inference.does_not_imply.is_empty(),
                "{}: NoAction findings must populate does_not_imply",
                f.id.as_str()
            );
        }
    }

    // The library fixture has no entry_points → F-NO-ENTRY-POINTS must
    // classify as Expected (the Phase 1 baseline test, end-to-end).
    let entry = findings
        .iter()
        .find(|f| f.id.as_str() == evidence_findings::F_NO_ENTRY_POINTS)
        .expect("F-NO-ENTRY-POINTS must be present for a library fixture");
    assert_eq!(entry.classification, FindingClassification::Expected);
    assert_eq!(entry.severity, FindingSeverity::None);
    assert!(entry.repo_context.contains(&RepoContextTag::RustLibrary));

    // F-CARGO-LOCK-STATE must be present because we know the build system is
    // cargo (we set it up that way) and the detector found no lockfile.
    let lock = findings
        .iter()
        .find(|f| f.id.as_str() == evidence_findings::F_CARGO_LOCK_STATE)
        .expect("F-CARGO-LOCK-STATE must be present for a Rust fixture");
    // Library + AbsentFromRepo → Informational, severity None.
    assert_eq!(lock.classification, FindingClassification::Informational);
    assert_eq!(lock.severity, FindingSeverity::None);

    // F-ORPHAN-FILES must omit README.md / LICENSE / .gitignore: those are
    // root config/policy/docs and must not appear in the orphan paths.
    if let Some(orph) = findings
        .iter()
        .find(|f| f.id.as_str() == evidence_findings::F_ORPHAN_FILES)
    {
        for ev in &orph.evidence {
            if let EvidenceItem::Observation { source_paths, .. } = ev {
                for path in source_paths {
                    let lower = path.to_ascii_lowercase();
                    assert!(
                        !lower.contains("readme") && lower != "license" && lower != ".gitignore",
                        "orphan source_paths must not include root configs/docs, got: {path}"
                    );
                }
            }
        }
    }

    // Every metric emitted is from the constant table.
    use repolens_core::model::evidence::metric_names;
    for f in findings {
        for ev in &f.evidence {
            match ev {
                EvidenceItem::Observation { metric, .. } => {
                    assert!(
                        metric_names::is_known(metric),
                        "{}: unknown metric {metric}",
                        f.id.as_str()
                    );
                }
                EvidenceItem::HeuristicJudgment {
                    observed_metric, ..
                } => {
                    assert!(
                        metric_names::is_known(observed_metric),
                        "{}: unknown observed_metric {observed_metric}",
                        f.id.as_str()
                    );
                }
                EvidenceItem::ExternalSignal { .. } => {}
            }
        }
    }
}
