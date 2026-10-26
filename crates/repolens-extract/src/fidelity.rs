//! Extraction fidelity: how completely RepoLens can extract symbols and
//! relationships from a given language. Drives the `F-LANGUAGE-EXTRACTION-GAP`
//! evidence finding, which discloses where the symbol list and import graph are
//! partial or absent so an agent knows when to corroborate with a source read.
//!
//! Tiers are derived from RepoLens's actual capability rather than re-declared:
//! - `Ast`         — a tree-sitter grammar exists (`ts_util::grammar_for`): full parse.
//! - `Heuristic`   — a registered extractor exists but no grammar (regex): Python.
//! - `Unsupported` — a recognized programming language with no extractor at all.
//! - `NotApplicable` — data/markup/config or unrecognized: no symbols expected.

/// How completely RepoLens extracts symbols/relationships for a language name
/// (as produced by [`crate::lang::detect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionFidelity {
    /// Full AST-based extraction via a tree-sitter grammar.
    Ast,
    /// Heuristic (regex) extraction — present but partial; misses nested,
    /// dynamic, or non-top-level constructs an AST would capture.
    Heuristic,
    /// Recognized as a programming language, but RepoLens has no extractor.
    /// Files appear in the inventory and as graph nodes but contribute no
    /// symbols and no import edges.
    Unsupported,
    /// Not a programming language RepoLens extracts symbols from (data, markup,
    /// config) or an unrecognized extension. No symbol output is expected, so
    /// this is not a gap.
    NotApplicable,
}

/// Programming languages [`crate::lang::detect`] recognizes but for which
/// RepoLens has no symbol/relationship extractor. Every name here is a
/// `lang::detect` programming-language row that is NOT registered in
/// [`crate::registry::SymbolExtractorRegistry`]. Data/markup languages
/// (Markdown, JSON, YAML, TOML, HTML, CSS, …) are deliberately absent — they
/// map to [`ExtractionFidelity::NotApplicable`].
const UNSUPPORTED_SOURCE_LANGUAGES: &[&str] = &[
    // JVM, systems, scripting, query, and component-framework languages that
    // `lang::detect` recognizes but `SymbolExtractorRegistry` does not extract.
    "Kotlin", "Scala", "Groovy", "C", "C++", "Swift", "Zig", "Dart", "Ruby", "PHP", "Lua", "R",
    "Shell", "SQL", "Vue", "Svelte",
];

/// Classify a RepoLens language name by how completely RepoLens can extract
/// symbols and relationships from it.
pub fn extraction_fidelity(language: &str) -> ExtractionFidelity {
    if crate::ts_util::grammar_for(language).is_some() {
        ExtractionFidelity::Ast
    } else if language == "Python" {
        // The only regex-backed extractor (see `registry` / `py_symbol`).
        ExtractionFidelity::Heuristic
    } else if UNSUPPORTED_SOURCE_LANGUAGES.contains(&language) {
        ExtractionFidelity::Unsupported
    } else {
        ExtractionFidelity::NotApplicable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sitter_languages_are_ast() {
        for lang in ["Java", "Go", "JavaScript", "TypeScript", "Rust"] {
            assert_eq!(
                extraction_fidelity(lang),
                ExtractionFidelity::Ast,
                "{lang} should be Ast"
            );
        }
    }

    #[test]
    fn python_is_heuristic() {
        assert_eq!(extraction_fidelity("Python"), ExtractionFidelity::Heuristic);
    }

    #[test]
    fn recognized_source_without_extractor_is_unsupported() {
        for lang in ["Ruby", "Shell", "C", "C++", "Kotlin", "Swift", "SQL", "PHP"] {
            assert_eq!(
                extraction_fidelity(lang),
                ExtractionFidelity::Unsupported,
                "{lang} should be Unsupported"
            );
        }
    }

    #[test]
    fn data_markup_and_unknown_are_not_applicable() {
        for lang in [
            "Markdown",
            "JSON",
            "YAML",
            "TOML",
            "HTML",
            "CSS",
            "Properties",
            "XML",
            "Unknown",
            "",
        ] {
            assert_eq!(
                extraction_fidelity(lang),
                ExtractionFidelity::NotApplicable,
                "{lang} should be NotApplicable"
            );
        }
    }

    /// Drift guard: a language RepoLens actually extracts (Ast or Heuristic)
    /// must never be listed as Unsupported. The registry / grammar table are
    /// the source of truth for extraction; this list is the complement.
    #[test]
    fn extracted_languages_not_in_unsupported_list() {
        for lang in ["Java", "Go", "JavaScript", "TypeScript", "Rust", "Python"] {
            assert!(
                !UNSUPPORTED_SOURCE_LANGUAGES.contains(&lang),
                "{lang} is extracted but listed as Unsupported"
            );
        }
    }
}
