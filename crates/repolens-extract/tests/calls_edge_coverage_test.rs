//! Tripwire for the `callEdgeCoverage: "java-and-rust"` disclosure that
//! `get_callers` emits: the static string rots silently if a language's
//! call-edge extraction changes, so this test pins the current coverage
//! boundary executable-y. Java and Rust are the POSITIVE set (Rust v1 is
//! bare + scoped calls only — receiver method calls deliberately excluded,
//! spec 2026-06-11); Go/JS/Python remain NEGATIVE.
//!
//! If a NEGATIVE assertion below starts failing, that is good news (a new
//! language gained call edges) — update `callEdgeCoverage` and
//! `callEdgeCoverageDetail` in `crates/repolens-mcp/src/tools/graph.rs` and
//! the `get_callers` tool description in the same change. Same rule if the
//! Rust form-boundary assertions move (e.g. receiver calls start emitting).

use repolens_extract::go_rel::GoRelationshipExtractor;
use repolens_extract::java_rel::JavaRelationshipExtractor;
use repolens_extract::js_rel::JsRelationshipExtractor;
use repolens_extract::py_rel::PyRelationshipExtractor;
use repolens_extract::rust_rel::RustRelationshipExtractor;
use repolens_extract::RelationshipExtractor;
use std::io::Write;

fn write_temp(suffix: &str, content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::with_suffix(suffix).unwrap();
    write!(f, "{content}").unwrap();
    f
}

#[test]
fn java_extractor_emits_calls_edges() {
    let f = write_temp(
        ".java",
        "package com.example;\n\npublic class App {\n    public void run() {\n        Helper h = new Helper();\n        h.greet();\n    }\n}\n",
    );
    let rels = JavaRelationshipExtractor::new().extract(f.path());
    assert!(
        rels.iter().any(|r| r.rel_type == "calls"),
        "Java must emit calls edges (the positive half of java-only); got: {rels:?}"
    );
}

#[test]
fn rust_extractor_emits_bare_and_scoped_calls_only() {
    // POSITIVE half of the Rust v1 boundary: bare and scoped calls emit.
    let f = write_temp(
        ".rs",
        "use crate::helper;\n\npub fn main() {\n    helper::run();\n    compute();\n}\n\nfn compute() {}\n",
    );
    let rels = RustRelationshipExtractor::new().extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(
        calls.len(),
        2,
        "Rust v1 emits bare + scoped calls; boundary moved — update callEdgeCoverageDetail; got: {rels:?}"
    );

    // NEGATIVE half: receiver method calls and stdlib paths stay out. If
    // either starts emitting, the v1 boundary moved — update the disclosure.
    let f2 = write_temp(
        ".rs",
        "pub fn main() {\n    let v = vec![1];\n    v.len();\n    std::mem::drop(v);\n}\n",
    );
    let rels2 = RustRelationshipExtractor::new().extract(f2.path());
    assert!(
        rels2.iter().all(|r| r.rel_type != "calls"),
        "Rust v1 boundary moved (receiver/stdlib calls emitting) — update callEdgeCoverageDetail; got: {rels2:?}"
    );
}

#[test]
fn go_extractor_emits_no_calls_edges() {
    let f = write_temp(
        ".go",
        "package main\n\nimport \"example.com/helper\"\n\nfunc main() {\n    helper.Run()\n    compute()\n}\n\nfunc compute() {}\n",
    );
    let rels = GoRelationshipExtractor::new().extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "Go gained calls edges — update callEdgeCoverage in mcp tools/graph.rs; got: {rels:?}"
    );
}

#[test]
fn js_extractor_emits_no_calls_edges() {
    // call_expression in js_rel routes only to extract_require (an imports
    // edge), so plain method calls must not produce calls edges.
    let f = write_temp(
        ".js",
        "const helper = require('./helper');\n\nfunction main() {\n    helper.run();\n    compute();\n}\n",
    );
    let rels = JsRelationshipExtractor::new().extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "JS gained calls edges — update callEdgeCoverage in mcp tools/graph.rs; got: {rels:?}"
    );
}

#[test]
fn python_extractor_emits_no_calls_edges() {
    let f = write_temp(
        ".py",
        "import helper\n\ndef main():\n    helper.run()\n    compute()\n\ndef compute():\n    pass\n",
    );
    let rels = PyRelationshipExtractor.extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "Python gained calls edges — update callEdgeCoverage in mcp tools/graph.rs; got: {rels:?}"
    );
}
