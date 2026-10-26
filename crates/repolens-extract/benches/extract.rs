//! Measurement harness (roadmap slice **P1**).
//!
//! Attributes per-file extraction time to its three cost components so that any
//! later optimization (parser reuse, parse-once, etc.) is justified by evidence
//! rather than intuition. For each tree-sitter language it runs four variants
//! over a representative fixture file:
//!
//! - **A `extract_full`**   — `SymbolExtractor::extract(path)`: read + `Parser::new`
//!   + `set_language` + parse + AST walk (the shipping path).
//! - **B `read_only`**      — `std::fs::read_to_string(path)`: file I/O only.
//! - **C `parse_fresh`**    — preloaded content, a fresh `Parser` each iteration:
//!   `Parser::new` + `set_language` + parse (no walk, no I/O).
//! - **D `parse_reused`**   — preloaded content, one `Parser` reused across
//!   iterations: parse only (no alloc, no walk, no I/O).
//!
//! Derived per-file shares (compare the criterion means):
//!   I/O ≈ B · parser_alloc ≈ C − D · parse ≈ D · AST_walk ≈ A − B − C.
//!
//! Decision rules (see the plan's P1):
//!   * Pursue parser reuse only if `(C − D)/A` is material **and** extraction is a
//!     material share of total scan time (per the scan phase timing in scan.rs).
//!   * If parse+walk dominates and a file is parsed twice in the pipeline, prefer
//!     parse-once over parser tricks.
//!
//! Caveat: the `fixtures/inputs/*` files are small (hundreds of bytes), so fixed
//! costs (the read syscall, `Parser::new`) are over-represented relative to a
//! large real file. Read these as ratios on small inputs, and corroborate with the
//! end-to-end per-phase scan timing against a real repository.

use std::path::PathBuf;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use repolens_extract::registry::SymbolExtractorRegistry;
use tree_sitter::{Language, Parser};

struct Case {
    /// Registry language key (e.g. "Java").
    lang: &'static str,
    /// tree-sitter grammar for the parse-only variants.
    grammar: Language,
    /// Fixture file path, relative to the workspace root.
    file: &'static str,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            lang: "Java",
            grammar: tree_sitter_java::LANGUAGE.into(),
            file: "fixtures/inputs/java-small/src/main/java/com/example/App.java",
        },
        Case {
            lang: "Go",
            grammar: tree_sitter_go::LANGUAGE.into(),
            file: "fixtures/inputs/go-small/internal/model/server.go",
        },
        Case {
            lang: "Rust",
            grammar: tree_sitter_rust::LANGUAGE.into(),
            file: "fixtures/inputs/rust-small/crates/gamma/src/main.rs",
        },
        Case {
            lang: "JavaScript",
            grammar: tree_sitter_javascript::LANGUAGE.into(),
            file: "fixtures/inputs/js-small/src/util.js",
        },
        Case {
            lang: "TypeScript",
            grammar: tree_sitter_typescript::LANGUAGE_TSX.into(),
            file: "fixtures/inputs/js-small/src/service.ts",
        },
    ]
}

/// Resolve a workspace-relative path from the crate manifest dir (CWD-independent).
fn workspace_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn bench_extract(c: &mut Criterion) {
    for case in cases() {
        let path = workspace_path(case.file);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
        let registry = SymbolExtractorRegistry::new();
        let extractor = registry
            .for_language(case.lang)
            .unwrap_or_else(|| panic!("no symbol extractor registered for {}", case.lang));

        let mut group = c.benchmark_group(case.lang);

        // A — full extract: read + Parser::new + set_language + parse + walk.
        group.bench_function("A_extract_full", |b| {
            b.iter(|| black_box(extractor.extract(black_box(&path))));
        });

        // B — file I/O only.
        group.bench_function("B_read_only", |b| {
            b.iter(|| black_box(std::fs::read_to_string(black_box(&path)).unwrap()));
        });

        // C — parse with a fresh parser each iteration (content preloaded).
        group.bench_function("C_parse_fresh", |b| {
            b.iter(|| {
                let mut parser = Parser::new();
                parser
                    .set_language(&case.grammar)
                    .expect("set grammar for parse_fresh");
                black_box(parser.parse(black_box(&content), None).expect("parse"));
            });
        });

        // D — parse with one reused parser (content preloaded).
        group.bench_function("D_parse_reused", |b| {
            let mut parser = Parser::new();
            parser
                .set_language(&case.grammar)
                .expect("set grammar for parse_reused");
            b.iter(|| {
                black_box(parser.parse(black_box(&content), None).expect("parse"));
            });
        });

        group.finish();
    }
}

criterion_group!(benches, bench_extract);
criterion_main!(benches);
