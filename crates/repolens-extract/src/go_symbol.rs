use crate::ts_util::{node_text, parse_source};
use crate::SymbolExtractor;
use repolens_core::model::file_entry::Symbol;
use std::path::Path;

/// Extracts symbols from Go source files using tree-sitter AST parsing.
pub struct GoSymbolExtractor {
    language: tree_sitter::Language,
}

impl GoSymbolExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_go::LANGUAGE.into(),
        }
    }
}

impl Default for GoSymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolExtractor for GoSymbolExtractor {
    fn extract(&self, path: &Path) -> Vec<Symbol> {
        let Some((tree, source)) = parse_source(path, &self.language, "tree-sitter-go grammar")
        else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Symbol> {
        let root = tree.root_node();
        let src = source.as_bytes();

        let mut symbols = Vec::new();
        collect_symbols(root, src, &mut symbols);
        symbols
    }
}

fn collect_symbols(root: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => extract_function(child, src, None, symbols),
            "method_declaration" => extract_method(child, src, symbols),
            "type_declaration" => extract_type_declaration(child, src, symbols),
            _ => {}
        }
    }
}

fn extract_function(
    node: tree_sitter::Node,
    src: &[u8],
    qualified_prefix: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };
    let visibility = go_visibility(&name);
    let qualified_name = match qualified_prefix {
        Some(prefix) => format!("{prefix}.{name}"),
        None => name.clone(),
    };
    symbols.push(Symbol {
        symbol_type: "function".to_string(),
        name,
        qualified_name: Some(qualified_name),
        visibility: Some(visibility),
        line: None,
    });
}

fn extract_method(node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    // method_declaration has a "receiver" field containing the receiver type
    let receiver_type = match node.child_by_field_name("receiver") {
        Some(r) => extract_receiver_type(r, src),
        None => return,
    };
    extract_function(node, src, Some(&receiver_type), symbols);
}

/// Extracts the base type name from a method receiver parameter list.
/// e.g. `(s *Server)` → `Server`, `(s server)` → `server`
fn extract_receiver_type(receiver_node: tree_sitter::Node, src: &[u8]) -> String {
    // receiver is a parameter_list; find the parameter inside
    let mut cursor = receiver_node.walk();
    for child in receiver_node.children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            if let Some(type_node) = child.child_by_field_name("type") {
                return strip_pointer(type_node, src);
            }
        }
    }
    // Fallback: use raw text
    node_text(receiver_node, src).to_string()
}

/// Strips a leading `*` pointer indirection from a type node.
fn strip_pointer(type_node: tree_sitter::Node, src: &[u8]) -> String {
    if type_node.kind() == "pointer_type" {
        // pointer_type has one unnamed child: the base type
        let mut cursor = type_node.walk();
        for child in type_node.children(&mut cursor) {
            if child.is_named() {
                return node_text(child, src).to_string();
            }
        }
    }
    node_text(type_node, src).to_string()
}

fn extract_type_declaration(node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    // type_declaration contains one or more type_spec children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" {
            extract_type_spec(child, src, symbols);
        }
    }
}

fn extract_type_spec(node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };
    let type_node = match node.child_by_field_name("type") {
        Some(t) => t,
        None => return,
    };
    let symbol_type = match type_node.kind() {
        "struct_type" => "class",
        "interface_type" => "interface",
        _ => "type",
    };
    let visibility = go_visibility(&name);
    symbols.push(Symbol {
        symbol_type: symbol_type.to_string(),
        name: name.clone(),
        qualified_name: Some(name),
        visibility: Some(visibility),
        line: None,
    });
}

fn go_visibility(name: &str) -> String {
    if name.starts_with(|c: char| c.is_uppercase()) {
        "public".to_string()
    } else {
        "private".to_string()
    }
}
