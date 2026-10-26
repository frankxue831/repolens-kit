use crate::ts_util::{node_text, parse_source};
use crate::SymbolExtractor;
use repolens_core::model::file_entry::Symbol;
use std::path::Path;

/// Extracts symbols from Java source files using tree-sitter AST parsing.
pub struct JavaSymbolExtractor {
    language: tree_sitter::Language,
}

impl JavaSymbolExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_java::LANGUAGE.into(),
        }
    }
}

impl Default for JavaSymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolExtractor for JavaSymbolExtractor {
    fn extract(&self, path: &Path) -> Vec<Symbol> {
        let Some((tree, source)) = parse_source(path, &self.language, "tree-sitter-java grammar")
        else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Symbol> {
        let root = tree.root_node();
        let src = source.as_bytes();

        // Extract package name
        let package_name = find_package_name(root, src);

        // Collect all symbols via recursive full-tree walk (matching Java's findAll)
        let mut symbols = Vec::new();
        collect_all_symbols(root, src, &package_name, &mut symbols);
        symbols
    }
}

fn find_package_name(root: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_declaration" {
            let mut inner = child.walk();
            for c in child.children(&mut inner) {
                if c.kind() == "scoped_identifier" || c.kind() == "identifier" {
                    return Some(node_text(c, src).to_string());
                }
            }
        }
    }
    None
}

/// Recursively walks the entire AST and collects symbols, matching Java's
/// `cu.findAll(...)` which finds all nested declarations at any depth.
fn collect_all_symbols(
    node: tree_sitter::Node,
    src: &[u8],
    package_name: &Option<String>,
    symbols: &mut Vec<Symbol>,
) {
    match node.kind() {
        "class_declaration" => {
            extract_type_symbol(node, src, package_name, "class", symbols);
        }
        "interface_declaration" => {
            extract_type_symbol(node, src, package_name, "interface", symbols);
        }
        "record_declaration" => {
            extract_type_symbol(node, src, package_name, "record", symbols);
        }
        "enum_declaration" => {
            extract_type_symbol(node, src, package_name, "enum", symbols);
        }
        "annotation_type_declaration" => {
            extract_type_symbol(node, src, package_name, "annotation", symbols);
        }
        "method_declaration" => {
            extract_method_symbol(node, src, symbols);
        }
        "constructor_declaration" => {
            extract_constructor_symbol(node, src, symbols);
        }
        "field_declaration" => {
            extract_field_symbol(node, src, symbols);
        }
        _ => {}
    }

    // Always recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_symbols(child, src, package_name, symbols);
    }
}

fn extract_type_symbol(
    node: tree_sitter::Node,
    src: &[u8],
    package_name: &Option<String>,
    symbol_type: &str,
    symbols: &mut Vec<Symbol>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };

    let qualified_name = match package_name {
        Some(pkg) => format!("{pkg}.{name}"),
        None => name.clone(),
    };

    let visibility = resolve_visibility(node, src);

    symbols.push(Symbol {
        symbol_type: symbol_type.to_string(),
        name,
        qualified_name: Some(qualified_name),
        visibility: Some(visibility),
        line: None,
    });
}

fn extract_method_symbol(node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };

    let return_type = match node.child_by_field_name("type") {
        Some(t) => node_text(t, src).to_string(),
        None => "void".to_string(),
    };

    let params = extract_param_types(node, src);
    let qualified_name = format!("{return_type} {name}({params})");
    let visibility = resolve_visibility(node, src);

    symbols.push(Symbol {
        symbol_type: "method".to_string(),
        name,
        qualified_name: Some(qualified_name),
        visibility: Some(visibility),
        line: None,
    });
}

fn extract_constructor_symbol(node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    let name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };

    let params = extract_param_types(node, src);
    let qualified_name = format!("{name}({params})");
    let visibility = resolve_visibility(node, src);

    symbols.push(Symbol {
        symbol_type: "method".to_string(),
        name,
        qualified_name: Some(qualified_name),
        visibility: Some(visibility),
        line: None,
    });
}

fn extract_field_symbol(node: tree_sitter::Node, src: &[u8], symbols: &mut Vec<Symbol>) {
    // Get the field type from the declaration
    let type_name = find_field_type(node, src);
    let visibility = resolve_visibility(node, src);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = node_text(name_node, src).to_string();
                symbols.push(Symbol {
                    symbol_type: "field".to_string(),
                    name,
                    qualified_name: type_name.clone(),
                    visibility: Some(visibility.clone()),
                    line: None,
                });
            }
        }
    }
}

/// Extracts the type name from a field_declaration node.
/// In tree-sitter-java, the type is a direct child (type_identifier, generic_type, etc.)
fn find_field_type(node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_identifier"
            | "generic_type"
            | "array_type"
            | "integral_type"
            | "floating_point_type"
            | "boolean_type"
            | "void_type"
            | "scoped_type_identifier" => {
                return Some(node_text(child, src).to_string());
            }
            _ => {}
        }
    }
    None
}

fn extract_param_types(node: tree_sitter::Node, src: &[u8]) -> String {
    let params_node = match node.child_by_field_name("parameters") {
        Some(p) => p,
        None => return String::new(),
    };

    let mut types = Vec::new();
    let mut cursor = params_node.walk();
    for child in params_node.children(&mut cursor) {
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            if let Some(type_node) = child.child_by_field_name("type") {
                let mut type_str = node_text(type_node, src).to_string();
                if has_array_dimensions(child) {
                    type_str.push_str("[]");
                }
                if child.kind() == "spread_parameter" {
                    type_str.push_str("...");
                }
                types.push(type_str);
            }
        }
    }
    types.join(", ")
}

fn has_array_dimensions(param: tree_sitter::Node) -> bool {
    let mut cursor = param.walk();
    for child in param.children(&mut cursor) {
        if child.kind() == "dimensions" {
            return true;
        }
    }
    false
}

fn resolve_visibility(node: tree_sitter::Node, src: &[u8]) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut mod_cursor = child.walk();
            for modifier in child.children(&mut mod_cursor) {
                let text = node_text(modifier, src);
                match text {
                    "public" => return "public".to_string(),
                    "private" => return "private".to_string(),
                    "protected" => return "protected".to_string(),
                    _ => {}
                }
            }
        }
    }
    "package-private".to_string()
}
