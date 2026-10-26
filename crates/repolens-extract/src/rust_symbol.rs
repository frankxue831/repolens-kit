use crate::ts_util::{node_text, parse_source};
use crate::SymbolExtractor;
use repolens_core::model::file_entry::Symbol;
use std::path::Path;

/// Extracts symbols from Rust source files using tree-sitter AST parsing.
pub struct RustSymbolExtractor {
    language: tree_sitter::Language,
}

impl RustSymbolExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
        }
    }
}

impl Default for RustSymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolExtractor for RustSymbolExtractor {
    fn extract(&self, path: &Path) -> Vec<Symbol> {
        let Some((tree, source)) = parse_source(path, &self.language, "tree-sitter-rust grammar")
        else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Symbol> {
        let root = tree.root_node();
        let src = source.as_bytes();

        let mut symbols = Vec::new();
        collect_root_symbols(root, src, &mut symbols);
        symbols
    }
}

/// Walks top-level declarations in source order. Only recurses into `impl_item`
/// bodies to collect methods — no deeper nesting in v1.
fn collect_root_symbols(root: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_item" => extract_function(child, src, None, symbols),
            "struct_item" => extract_type_symbol(child, src, "class", symbols),
            "enum_item" => extract_type_symbol(child, src, "enum", symbols),
            "trait_item" => extract_type_symbol(child, src, "interface", symbols),
            "type_item" => extract_type_symbol(child, src, "type", symbols),
            "const_item" | "static_item" => extract_type_symbol(child, src, "field", symbols),
            "mod_item" => extract_type_symbol(child, src, "module", symbols),
            "impl_item" => extract_impl_methods(child, src, symbols),
            _ => {}
        }
    }
}

fn extract_function(
    node: tree_sitter::Node,
    src: &[u8],
    impl_type: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };

    let qualified_name = match impl_type {
        Some(t) => format!("{t}.{name}"),
        None => name.clone(),
    };

    let visibility = resolve_visibility(node, src);

    // Impl-block fns are "method" so the resolver's free-function map stops
    // treating new/default/fmt-class names as bare/scoped call candidates
    // (map-split spec 2026-06-12). Top-level fns stay "function".
    let symbol_type = if impl_type.is_some() {
        "method"
    } else {
        "function"
    };

    symbols.push(Symbol {
        symbol_type: symbol_type.to_string(),
        name,
        qualified_name: Some(qualified_name),
        visibility: Some(visibility),
        // tree-sitter rows are 0-based; the wire convention is 1-based.
        line: Some(node.start_position().row as u32 + 1),
    });
}

fn extract_type_symbol(
    node: tree_sitter::Node,
    src: &[u8],
    symbol_type: &str,
    symbols: &mut Vec<Symbol>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };
    let visibility = resolve_visibility(node, src);
    symbols.push(Symbol {
        symbol_type: symbol_type.to_string(),
        name: name.clone(),
        qualified_name: Some(name),
        visibility: Some(visibility),
        // tree-sitter rows are 0-based; the wire convention is 1-based.
        line: Some(node.start_position().row as u32 + 1),
    });
}

fn extract_impl_methods(impl_node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    let impl_type = match impl_node.child_by_field_name("type") {
        Some(t) => rightmost_type_identifier(t, src),
        None => return,
    };

    if let Some(body) = impl_node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" {
                extract_function(child, src, Some(&impl_type), symbols);
            }
        }
    }
}

/// Extracts the right-most simple identifier from a type node.
/// `Server` → `Server`, `crate::net::Server` → `Server`, `Server<T>` → `Server`.
fn rightmost_type_identifier(node: tree_sitter::Node, src: &[u8]) -> String {
    match node.kind() {
        "type_identifier" => node_text(node, src).to_string(),
        "scoped_type_identifier" => {
            if let Some(name) = node.child_by_field_name("name") {
                node_text(name, src).to_string()
            } else {
                node_text(node, src).to_string()
            }
        }
        "generic_type" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                rightmost_type_identifier(type_node, src)
            } else {
                node_text(node, src).to_string()
            }
        }
        _ => node_text(node, src).to_string(),
    }
}

fn resolve_visibility(node: tree_sitter::Node, src: &[u8]) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(child, src);
            return match text {
                "pub" => "public".to_string(),
                s if s.starts_with("pub(crate)") => "crate".to_string(),
                s if s.starts_with("pub(super)") => "restricted".to_string(),
                s if s.starts_with("pub(in") => "restricted".to_string(),
                _ => "public".to_string(),
            };
        }
    }
    "private".to_string()
}
