use crate::relationship::{Relationship, TYPE_IMPORT};
use crate::ts_util::{node_text, parse_source};
use crate::RelationshipExtractor;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::Path;

/// Extracts relationships from JavaScript and TypeScript files using tree-sitter AST parsing.
///
/// Stores two grammars (JS + TS TSX). Shared instance for both "JavaScript" and "TypeScript".
pub struct JsRelationshipExtractor {
    js_language: tree_sitter::Language,
    ts_language: tree_sitter::Language,
}

impl JsRelationshipExtractor {
    pub fn new() -> Self {
        Self {
            js_language: tree_sitter_javascript::LANGUAGE.into(),
            ts_language: tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

impl Default for JsRelationshipExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// JS built-in types filtered from type_ref.
static COMMON_JS_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "String", "Number", "Boolean", "Object", "Array", "Function", "Symbol", "RegExp", "Error",
        "Date", "Map", "Set", "WeakMap", "WeakSet", "Promise",
    ]
    .into_iter()
    .collect()
});

/// TS built-in utility types filtered from type_ref.
static COMMON_TS_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "Partial",
        "Required",
        "Readonly",
        "Record",
        "Pick",
        "Omit",
        "Exclude",
        "Extract",
        "ReturnType",
        "Parameters",
        "InstanceType",
    ]
    .into_iter()
    .collect()
});

/// Primitive types and keywords filtered from type_ref.
static PRIMITIVE_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "string",
        "number",
        "boolean",
        "void",
        "never",
        "any",
        "unknown",
        "null",
        "undefined",
        "bigint",
    ]
    .into_iter()
    .collect()
});

/// Single-letter generic type parameters filtered from type_ref.
static SINGLE_LETTER_GENERICS: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ["T", "K", "V", "U", "E", "R"].into_iter().collect());

impl RelationshipExtractor for JsRelationshipExtractor {
    fn extract(&self, path: &Path) -> Vec<Relationship> {
        let language = language_for_path(path, &self.js_language, &self.ts_language);
        let Some((tree, source)) = parse_source(path, language, "JS/TS grammar") else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Relationship> {
        let root = tree.root_node();
        let src = source.as_bytes();

        let mut rels = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        collect_relationships(root, src, &mut rels, &mut seen);
        rels
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

/// Recursively walks the AST collecting relationships.
fn collect_relationships(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    match node.kind() {
        "import_statement" => {
            extract_import(node, src, rels, seen);
            return; // don't recurse into import children
        }
        "export_statement" => {
            // Re-export: export { ... } from './path'
            if let Some(source_node) = node.child_by_field_name("source") {
                let path = unquote(node_text(source_node, src));
                if is_relative(&path) {
                    let kind = import_kind(node_text(node, src));
                    add_rel(kind, &path, &path, rels, seen);
                }
            }
            // Fall through to recurse into export's declaration for extends/implements/type_ref
        }
        "call_expression" => {
            extract_require(node, src, rels, seen);
        }
        "class_declaration" | "abstract_class_declaration" => {
            extract_class_heritage(node, src, rels, seen);
        }
        "interface_declaration" => {
            extract_interface_heritage(node, src, rels, seen);
        }
        "type_annotation" | "return_type" => {
            collect_type_refs(node, src, rels, seen);
            return; // don't recurse further into type nodes
        }
        _ => {}
    }

    // Recurse into named children using index-based iteration (avoids cursor borrow issues)
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            collect_relationships(child, src, rels, seen);
        }
    }
}

/// Extracts import path from `import ... from './path'` or `import './path'`.
fn extract_import(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(source_node) = node.child_by_field_name("source") {
        let path = unquote(node_text(source_node, src));
        if is_relative(&path) {
            let kind = import_kind(node_text(node, src));
            add_rel(kind, &path, &path, rels, seen);
        }
    }
}

/// `type_import` for a TypeScript type-only import or re-export, which the
/// compiler erases (`import type { A } from`, `export type { A } from`,
/// `import { type A, type B } from`); `imports` otherwise. Type-only imports
/// still mark the file as used but are left out of cycle detection, like
/// Python's `if TYPE_CHECKING:` imports.
fn import_kind(statement: &str) -> &'static str {
    let s = statement.trim_start();
    let rest = s
        .strip_prefix("import")
        .or_else(|| s.strip_prefix("export"))
        .unwrap_or("");
    if !rest.starts_with(char::is_whitespace) {
        return "imports";
    }
    let rest = rest.trim_start();
    if rest.starts_with("type ") || rest.starts_with("type{") {
        return TYPE_IMPORT;
    }
    // `import { type A, type B } from`: every named specifier is type-only
    // and there is no default or namespace import.
    if let Some(inner) = rest.strip_prefix('{') {
        if let Some(end) = inner.find('}') {
            let specs: Vec<&str> = inner[..end]
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            if !specs.is_empty() && specs.iter().all(|p| p.starts_with("type ")) {
                return TYPE_IMPORT;
            }
        }
    }
    "imports"
}

/// Extracts import path from `require('./path')`.
fn extract_require(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    // Check if callee is "require"
    if let Some(callee) = node.child_by_field_name("function") {
        if node_text(callee, src) != "require" {
            return;
        }
    } else {
        return;
    }
    // Get first argument
    if let Some(args) = node.child_by_field_name("arguments") {
        for i in 0..args.child_count() {
            if let Some(child) = args.child(i) {
                if child.kind() == "string" {
                    let path = unquote(node_text(child, src));
                    if is_relative(&path) {
                        add_rel("imports", &path, &path, rels, seen);
                    }
                    return;
                }
            }
        }
    }
}

/// Extracts `extends` and `implements` from class declarations.
fn extract_class_heritage(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            match child.kind() {
                "class_heritage" => {
                    // class Foo extends Bar implements Baz, Qux
                    for j in 0..child.child_count() {
                        if let Some(hc) = child.child(j) {
                            match hc.kind() {
                                "extends_clause" => {
                                    // First named child after "extends" keyword is the type
                                    for k in 0..hc.named_child_count() {
                                        if let Some(extends_child) = hc.named_child(k) {
                                            let name = rightmost_identifier(extends_child, src);
                                            if !name.is_empty() {
                                                add_rel("extends", &name, &name, rels, seen);
                                            }
                                            break; // only one superclass
                                        }
                                    }
                                }
                                "implements_clause" => {
                                    for k in 0..hc.named_child_count() {
                                        if let Some(impl_child) = hc.named_child(k) {
                                            if impl_child.kind() != "implements" {
                                                let name = rightmost_identifier(impl_child, src);
                                                if !name.is_empty() {
                                                    add_rel("implements", &name, &name, rels, seen);
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // Some grammars put extends directly as a field
                "extends_clause" => {
                    for j in 0..child.named_child_count() {
                        if let Some(extends_child) = child.named_child(j) {
                            let name = rightmost_identifier(extends_child, src);
                            if !name.is_empty() {
                                add_rel("extends", &name, &name, rels, seen);
                            }
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Extracts `extends` from interface declarations: `interface Foo extends Bar, Baz`.
fn extract_interface_heritage(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "extends_type_clause" {
                for j in 0..child.named_child_count() {
                    if let Some(extends_child) = child.named_child(j) {
                        let name = rightmost_identifier(extends_child, src);
                        if !name.is_empty() {
                            add_rel("extends", &name, &name, rels, seen);
                        }
                    }
                }
            }
        }
    }
}

/// Recursively finds type identifiers within type annotations.
fn collect_type_refs(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    match node.kind() {
        "type_identifier" | "identifier" => {
            let name = node_text(node, src);
            if should_emit_type_ref(name) {
                add_rel("type_ref", name, name, rels, seen);
            }
        }
        "nested_type_identifier" => {
            // e.g., Namespace.Type — extract the rightmost
            let name = rightmost_identifier(node, src);
            if should_emit_type_ref(&name) {
                add_rel("type_ref", &name, &name, rels, seen);
            }
        }
        _ => {
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    collect_type_refs(child, src, rels, seen);
                }
            }
        }
    }
}

/// Extracts the rightmost simple identifier from a type expression.
fn rightmost_identifier(node: tree_sitter::Node, src: &[u8]) -> String {
    match node.kind() {
        "type_identifier" | "identifier" => node_text(node, src).to_string(),
        "generic_type" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                rightmost_identifier(name_node, src)
            } else {
                node_text(node, src).to_string()
            }
        }
        "nested_type_identifier" => {
            // Module.Type — take the last named child
            let mut last = node_text(node, src).to_string();
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    last = node_text(child, src).to_string();
                }
            }
            last
        }
        _ => node_text(node, src).to_string(),
    }
}

fn should_emit_type_ref(name: &str) -> bool {
    !COMMON_JS_TYPES.contains(name)
        && !COMMON_TS_TYPES.contains(name)
        && !PRIMITIVE_TYPES.contains(name)
        && !SINGLE_LETTER_GENERICS.contains(name)
}

fn is_relative(path: &str) -> bool {
    path.starts_with("./") || path.starts_with("../")
}

/// Remove surrounding quotes from a string literal.
fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '\'' || c == '"' || c == '`')
        .to_string()
}

fn add_rel(
    rel_type: &str,
    target_name: &str,
    detail: &str,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let key = if rel_type == "imports" || rel_type == TYPE_IMPORT {
        format!("{rel_type}:{detail}")
    } else {
        format!("{rel_type}:{target_name}")
    };
    if seen.insert(key) {
        rels.push(Relationship {
            rel_type: rel_type.to_string(),
            target_name: target_name.to_string(),
            detail: detail.to_string(),
        });
    }
}
