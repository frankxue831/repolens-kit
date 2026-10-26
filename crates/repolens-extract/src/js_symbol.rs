use crate::ts_util::{node_text, parse_source};
use crate::SymbolExtractor;
use repolens_core::model::file_entry::Symbol;
use std::path::Path;

/// Extracts symbols from JavaScript and TypeScript files using tree-sitter AST parsing.
///
/// Stores two grammars: JavaScript (for .js/.jsx/.mjs/.cjs) and TypeScript TSX
/// (for .ts/.tsx/.mts/.cts — TSX is a superset of TS). Single shared instance
/// registered under both "JavaScript" and "TypeScript" in the registry.
pub struct JsSymbolExtractor {
    js_language: tree_sitter::Language,
    ts_language: tree_sitter::Language,
}

impl JsSymbolExtractor {
    pub fn new() -> Self {
        Self {
            js_language: tree_sitter_javascript::LANGUAGE.into(),
            ts_language: tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

impl Default for JsSymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolExtractor for JsSymbolExtractor {
    fn extract(&self, path: &Path) -> Vec<Symbol> {
        let language = language_for_path(path, &self.js_language, &self.ts_language);
        let Some((tree, source)) = parse_source(path, language, "JS/TS grammar") else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Symbol> {
        let root = tree.root_node();
        let src = source.as_bytes();

        // Collect CommonJS exported names for visibility detection
        let cjs_exports = collect_cjs_export_names(root, src);

        let mut symbols = Vec::new();
        collect_top_level_symbols(root, src, &cjs_exports, &mut symbols);
        symbols
    }
}

/// Select grammar based on file extension.
fn language_for_path<'a>(
    path: &Path,
    js_lang: &'a tree_sitter::Language,
    ts_lang: &'a tree_sitter::Language,
) -> &'a tree_sitter::Language {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "ts" | "tsx" | "mts" | "cts" => ts_lang,
        _ => js_lang,
    }
}

/// Walks program children (top-level only) and class bodies for methods.
fn collect_top_level_symbols(
    root: tree_sitter::Node,
    src: &[u8],
    cjs_exports: &[String],
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "export_statement" => {
                // Exported declaration: unwrap and extract with public visibility.
                // `declaration` covers: export function/class/interface/type/enum/const ...
                // `value` covers: export default <expr>
                // Unnamed-children fallback covers: export default class Foo {} where the
                // class node is a direct child without a named field.
                let has_declaration = child.child_by_field_name("declaration").is_some();
                let has_value = child.child_by_field_name("value").is_some();
                if let Some(decl) = child.child_by_field_name("declaration") {
                    extract_declaration(decl, src, true, cjs_exports, symbols);
                }
                if let Some(value) = child.child_by_field_name("value") {
                    extract_declaration(value, src, true, cjs_exports, symbols);
                }
                // Only scan unnamed children when neither field fired, to avoid duplicates.
                if !has_declaration && !has_value {
                    let mut inner_cursor = child.walk();
                    for inner in child.children(&mut inner_cursor) {
                        if matches!(
                            inner.kind(),
                            "class_declaration"
                                | "abstract_class_declaration"
                                | "function_declaration"
                        ) {
                            extract_declaration(inner, src, true, cjs_exports, symbols);
                        }
                    }
                }
            }
            _ => {
                extract_declaration(child, src, false, cjs_exports, symbols);
            }
        }
    }
}

/// Extracts a symbol from a declaration node if it's a recognized type.
fn extract_declaration(
    node: tree_sitter::Node,
    src: &[u8],
    is_export: bool,
    cjs_exports: &[String],
    symbols: &mut Vec<Symbol>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let name_str = node_text(name, src).to_string();
                let vis = if is_export || cjs_exports.contains(&name_str) {
                    "public"
                } else {
                    "module-private"
                };
                symbols.push(Symbol {
                    symbol_type: "function".to_string(),
                    name: name_str.clone(),
                    qualified_name: Some(name_str),
                    visibility: Some(vis.to_string()),
                    line: None,
                });
            }
        }
        "class_declaration" | "abstract_class_declaration" => {
            extract_class(node, src, is_export, cjs_exports, symbols);
        }
        "interface_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let name_str = node_text(name, src).to_string();
                let vis = if is_export {
                    "public"
                } else {
                    "module-private"
                };
                symbols.push(Symbol {
                    symbol_type: "interface".to_string(),
                    name: name_str.clone(),
                    qualified_name: Some(name_str),
                    visibility: Some(vis.to_string()),
                    line: None,
                });
            }
        }
        "type_alias_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let name_str = node_text(name, src).to_string();
                let vis = if is_export {
                    "public"
                } else {
                    "module-private"
                };
                symbols.push(Symbol {
                    symbol_type: "type".to_string(),
                    name: name_str.clone(),
                    qualified_name: Some(name_str),
                    visibility: Some(vis.to_string()),
                    line: None,
                });
            }
        }
        "enum_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let name_str = node_text(name, src).to_string();
                let vis = if is_export {
                    "public"
                } else {
                    "module-private"
                };
                symbols.push(Symbol {
                    symbol_type: "enum".to_string(),
                    name: name_str.clone(),
                    qualified_name: Some(name_str),
                    visibility: Some(vis.to_string()),
                    line: None,
                });
            }
        }
        "lexical_declaration" => {
            extract_lexical_functions(node, src, is_export, cjs_exports, symbols);
        }
        _ => {}
    }
}

/// Extracts class symbol + its methods.
fn extract_class(
    node: tree_sitter::Node,
    src: &[u8],
    is_export: bool,
    cjs_exports: &[String],
    symbols: &mut Vec<Symbol>,
) {
    let class_name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };
    let vis = if is_export || cjs_exports.contains(&class_name) {
        "public"
    } else {
        "module-private"
    };
    symbols.push(Symbol {
        symbol_type: "class".to_string(),
        name: class_name.clone(),
        qualified_name: Some(class_name.clone()),
        visibility: Some(vis.to_string()),
        line: None,
    });

    // Extract methods from class body
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "method_definition" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let method_name = node_text(name_node, src).to_string();
                    let qualified = format!("{class_name}.{method_name}");
                    symbols.push(Symbol {
                        symbol_type: "method".to_string(),
                        name: method_name,
                        qualified_name: Some(qualified),
                        visibility: Some("public".to_string()),
                        line: None,
                    });
                }
            }
        }
    }
}

/// Detects `const foo = () => ...` or `const foo = function() ...` patterns.
fn extract_lexical_functions(
    node: tree_sitter::Node,
    src: &[u8],
    is_export: bool,
    cjs_exports: &[String],
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            let name_node = child.child_by_field_name("name");
            let value_node = child.child_by_field_name("value");
            if let (Some(name), Some(value)) = (name_node, value_node) {
                if matches!(value.kind(), "arrow_function" | "function_expression") {
                    let name_str = node_text(name, src).to_string();
                    let vis = if is_export || cjs_exports.contains(&name_str) {
                        "public"
                    } else {
                        "module-private"
                    };
                    symbols.push(Symbol {
                        symbol_type: "function".to_string(),
                        name: name_str.clone(),
                        qualified_name: Some(name_str),
                        visibility: Some(vis.to_string()),
                        line: None,
                    });
                }
            }
        }
    }
}

/// Scans for CommonJS `module.exports = { name1, name2 }` and `exports.name = ...` patterns.
/// Returns a list of exported symbol names (best-effort heuristic).
fn collect_cjs_export_names(root: tree_sitter::Node, src: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "expression_statement" {
            // tree-sitter-javascript does not use a field name "expression" on
            // expression_statement; the expression is the first named child.
            if let Some(expr) = child.named_child(0) {
                if expr.kind() == "assignment_expression" {
                    let left = expr.child_by_field_name("left");
                    let right = expr.child_by_field_name("right");
                    if let Some(left_node) = left {
                        let left_text = node_text(left_node, src);
                        if left_text == "module.exports" {
                            // module.exports = { foo, bar } or module.exports = Foo
                            if let Some(right_node) = right {
                                collect_export_object_names(right_node, src, &mut names);
                            }
                        } else if left_text.starts_with("exports.") {
                            // exports.foo = ...
                            if let Some(prop) = left_text.strip_prefix("exports.") {
                                names.push(prop.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    names
}

/// Extracts property names from an object literal: `{ foo, bar, baz: qux }`.
fn collect_export_object_names(node: tree_sitter::Node, src: &[u8], names: &mut Vec<String>) {
    if node.kind() == "object" {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                match child.kind() {
                    "shorthand_property_identifier" => {
                        names.push(node_text(child, src).to_string());
                    }
                    "pair" => {
                        if let Some(key) = child.child_by_field_name("key") {
                            names.push(node_text(key, src).to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    } else if node.kind() == "identifier" {
        // module.exports = SomeClass
        names.push(node_text(node, src).to_string());
    }
}
