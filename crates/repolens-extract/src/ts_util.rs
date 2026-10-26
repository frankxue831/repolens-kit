/// Extracts UTF-8 text from a tree-sitter node, returning `""` on failure.
pub fn node_text<'a>(node: tree_sitter::Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

/// Reads a source file and parses it with the given tree-sitter `language`.
///
/// Centralizes the read + parse boilerplate shared by every tree-sitter
/// extractor: on a read or parse failure it logs a warning and returns `None`
/// (callers then return an empty result), matching the previous per-extractor
/// behavior exactly. `grammar_label` is the `.expect(...)` message for the
/// (statically linked, effectively infallible) `set_language` call, preserving
/// each extractor's original panic text.
///
/// Returns the owned `Tree` together with the file `source`; both must stay in
/// scope while walking nodes, since `Node::utf8_text` slices the source bytes.
pub fn parse_source(
    path: &std::path::Path,
    language: &tree_sitter::Language,
    grammar_label: &'static str,
) -> Option<(tree_sitter::Tree, String)> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            return None;
        }
    };

    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).expect(grammar_label);

    match parser.parse(&source, None) {
        Some(tree) => Some((tree, source)),
        None => {
            tracing::warn!("Failed to parse {}", path.display());
            None
        }
    }
}

/// Map a RepoLens language name to its tree-sitter grammar. None for languages
/// without a tree-sitter extractor (e.g. Python, which uses regex).
pub fn grammar_for(language: &str) -> Option<tree_sitter::Language> {
    Some(match language {
        "Java" => tree_sitter_java::LANGUAGE.into(),
        "Go" => tree_sitter_go::LANGUAGE.into(),
        // Mirror the per-extension grammar split the JS/TS extractors use (see
        // js_symbol::language_for_path): .js/.jsx/.mjs/.cjs → "JavaScript" → the
        // JavaScript grammar; .ts/.tsx/.mts/.cts → "TypeScript" → the TSX grammar
        // (a superset). lang::detect groups extensions the same way, so the
        // language name uniquely determines the grammar.
        "JavaScript" => tree_sitter_javascript::LANGUAGE.into(),
        "TypeScript" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "Rust" => tree_sitter_rust::LANGUAGE.into(),
        _ => return None,
    })
}

/// Parse already-read `source` for `language`. Returns None for non-tree-sitter
/// languages or on parse failure.
pub fn parse_for(language: &str, source: &str) -> Option<tree_sitter::Tree> {
    let language = grammar_for(language)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}
