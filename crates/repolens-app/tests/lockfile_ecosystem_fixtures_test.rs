//! #25: end-to-end lockfile advice on small fixture repos. Each fixture is
//! scanned for real and summarized; the test checks whether
//! F-LOCKFILE-MISSING-NON-CARGO fires and how the repo was classified.

use std::fs;
use std::path::Path;

use repolens_app::evidence_findings::F_LOCKFILE_MISSING_NON_CARGO;
use repolens_app::scan::ScanService;
use repolens_app::summary::{SummaryMemoryTier, SummaryService};
use repolens_core::model::evidence::{EvidenceFinding, RepoContextTag};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// Scans `files` and returns (all findings, the lockfile finding if any).
fn scan(files: &[(&str, &str)]) -> (Vec<EvidenceFinding>, Option<EvidenceFinding>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, body) in files {
        write(root, rel, body);
    }
    let result = ScanService::new().run(root, false, 100).unwrap();
    let summary =
        SummaryService::summarize(&result.pack_dir, Some(root), SummaryMemoryTier::Compact)
            .unwrap();
    let findings = summary.evidence_findings;
    let lock = findings
        .iter()
        .find(|f| f.id.as_str() == F_LOCKFILE_MISSING_NON_CARGO)
        .cloned();
    (findings, lock)
}

fn has_tag(findings: &[EvidenceFinding], tag: RepoContextTag) -> bool {
    findings.iter().any(|f| f.repo_context.contains(&tag))
}

const PY_HELPER: &str = "def run():\n    return 0\n";

/// Like psf/requests: setuptools via pyproject.toml plus setup.py, no
/// lockfile tool, and a module with an `if __name__` block.
#[test]
fn requests_like_plain_setuptools_gets_no_lockfile_finding() {
    let (_, lock) = scan(&[
        (
            "pyproject.toml",
            "[build-system]\nrequires = [\"setuptools>=61.0\"]\n\
             build-backend = \"setuptools.build_meta\"\n\n\
             [project]\nname = \"requests\"\ndependencies = [\"idna\"]\n",
        ),
        ("setup.py", "from setuptools import setup\n\nsetup()\n"),
        ("requirements-dev.txt", "pytest\n"),
        ("src/requests/__init__.py", "from .api import get\n"),
        ("src/requests/api.py", PY_HELPER),
        (
            "src/requests/help.py",
            "def main():\n    pass\n\nif __name__ == \"__main__\":\n    main()\n",
        ),
    ]);
    assert!(lock.is_none(), "plain setuptools has no lockfile: {lock:?}");
}

/// Like ManimSimpleRender: a setuptools app with [project.scripts] and a
/// requirements.txt, no lockfile tool.
#[test]
fn msr_like_requirements_txt_app_gets_no_lockfile_finding() {
    let (findings, lock) = scan(&[
        (
            "pyproject.toml",
            "[build-system]\nrequires = [\"setuptools\"]\n\n\
             [project]\nname = \"msr\"\n\n\
             [project.scripts]\nmsr = \"app.cli:main\"\n",
        ),
        ("requirements.txt", "manim\n"),
        ("app/__init__.py", ""),
        (
            "app/cli.py",
            "from app import render\n\ndef main():\n    render.run()\n",
        ),
        ("app/render.py", PY_HELPER),
    ]);
    assert!(
        lock.is_none(),
        "pip requirements have no lockfile: {lock:?}"
    );
    assert!(has_tag(&findings, RepoContextTag::PythonScript));
    assert!(!has_tag(&findings, RepoContextTag::PythonPackage));
}

/// A Poetry app whose only entry point is [project.scripts]: it is an app,
/// so a missing poetry.lock is still reported.
#[test]
fn python_app_with_project_scripts_is_not_silenced_as_a_library() {
    let (findings, lock) = scan(&[
        (
            "pyproject.toml",
            "[project]\nname = \"tool\"\n\n\
             [project.scripts]\ntool = \"tool.cli:main\"\n\n\
             [tool.poetry]\npackage-mode = true\n",
        ),
        ("tool/__init__.py", ""),
        (
            "tool/cli.py",
            "from tool import core\n\ndef main():\n    core.run()\n",
        ),
        ("tool/core.py", PY_HELPER),
    ]);
    assert!(has_tag(&findings, RepoContextTag::PythonScript));
    let lock = lock.expect("a Poetry app without poetry.lock is a finding");
    let s = &lock.inference.statement;
    assert!(s.contains("poetry.lock"), "{s}");
    assert!(
        !s.contains("package-lock.json") && !s.contains("go.sum"),
        "{s}"
    );
}

/// A Node library: main/exports and no bin. Libraries leave lockfiles out.
#[test]
fn node_library_with_main_and_exports_is_a_library() {
    let (findings, lock) = scan(&[
        (
            "package.json",
            r#"{"name":"lib","version":"1.0.0","main":"index.js","exports":"./index.js"}"#,
        ),
        ("index.js", "module.exports = require('./util');\n"),
        ("util.js", "module.exports = {};\n"),
    ]);
    assert!(has_tag(&findings, RepoContextTag::NodeLibrary));
    assert!(lock.is_none(), "a Node library gets no lockfile finding");
}

/// A Node CLI with bin is an app, so a missing lockfile is reported with
/// only npm-family names.
#[test]
fn node_cli_with_bin_gets_the_lockfile_finding() {
    let (findings, lock) = scan(&[
        (
            "package.json",
            r#"{"name":"tool","version":"1.0.0","bin":{"tool":"cli.js"}}"#,
        ),
        ("cli.js", "require('./util');\n"),
        ("util.js", "module.exports = {};\n"),
    ]);
    assert!(has_tag(&findings, RepoContextTag::NodeApplication));
    let lock = lock.expect("a Node CLI without a lockfile is a finding");
    let s = &lock.inference.statement;
    assert!(s.contains("package-lock.json"), "{s}");
    assert!(!s.contains("poetry.lock"), "{s}");
}
