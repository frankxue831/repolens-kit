use crate::relationship::Relationship;
use crate::ts_util::{node_text, parse_source};
use crate::RelationshipExtractor;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::Path;

/// Extracts relationships from Rust source files using tree-sitter AST parsing.
pub struct RustRelationshipExtractor {
    language: tree_sitter::Language,
}

impl RustRelationshipExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
        }
    }
}

impl Default for RustRelationshipExtractor {
    fn default() -> Self {
        Self::new()
    }
}

static PRIMITIVE_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32",
        "i64", "i128", "isize", "f32", "f64",
    ]
    .into_iter()
    .collect()
});

/// Common Rust standard library types filtered from type_ref.
/// Ubiquitous types that produce low-value unresolved graph edges.
static COMMON_RUST_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "String", "Vec", "VecDeque", "Option", "Result", "Box", "HashMap", "HashSet", "BTreeMap",
        "BTreeSet", "Path", "PathBuf", "Arc", "Rc", "Mutex", "RwLock",
    ]
    .into_iter()
    .collect()
});

impl RelationshipExtractor for RustRelationshipExtractor {
    fn extract(&self, path: &Path) -> Vec<Relationship> {
        let Some((tree, source)) = parse_source(path, &self.language, "tree-sitter-rust grammar")
        else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Relationship> {
        let root = tree.root_node();
        let src = source.as_bytes();

        // Pass 1 (Layer 1.6, map-split spec): names this file binds to
        // std/core/alloc items. Calls through those bindings are stdlib
        // calls invisible to the path-based filter. Names only — emission
        // stays in pass 2 so relationship order is unchanged.
        let mut std_aliases: HashSet<String> = HashSet::new();
        collect_std_aliases(root, src, &mut std_aliases);

        let mut rels = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        collect_relationships(root, src, &std_aliases, &mut rels, &mut seen);
        rels
    }
}

/// Recursively find `use` declarations and record names bound to
/// std/core/alloc paths: `use std::fs;` binds "fs", `use std::fs::write;`
/// binds "write", `use std::io as stdio;` binds "stdio",
/// `use std::fs::{self, File}` binds "fs" (and "File"). Wildcards bind
/// unknowable names and are skipped.
fn collect_std_aliases(node: tree_sitter::Node, src: &[u8], out: &mut HashSet<String>) {
    if node.kind() == "use_declaration" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                collect_std_alias_tree(child, src, "", out);
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_std_aliases(child, src, out);
    }
}

fn collect_std_alias_tree(
    node: tree_sitter::Node,
    src: &[u8],
    prefix: &str,
    out: &mut HashSet<String>,
) {
    match node.kind() {
        "scoped_identifier" | "identifier" | "crate" => {
            let full_path = join_prefix(prefix, node_text(node, src));
            if is_stdlib_path(&full_path) {
                if let Some(name) = full_path.rsplit("::").next() {
                    out.insert(name.to_string());
                }
            }
        }
        "scoped_use_list" => {
            let path_text = node
                .child_by_field_name("path")
                .map(|p| node_text(p, src))
                .unwrap_or("");
            let new_prefix = join_prefix(prefix, path_text);
            if let Some(list) = node.child_by_field_name("list") {
                collect_std_alias_tree(list, src, &new_prefix, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    collect_std_alias_tree(child, src, prefix, out);
                }
            }
        }
        "use_as_clause" => {
            if let (Some(p), Some(a)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("alias"),
            ) {
                if is_stdlib_path(&join_prefix(prefix, node_text(p, src))) {
                    out.insert(node_text(a, src).to_string());
                }
            }
        }
        // use std::fs::{self} — binds the prefix's own last segment.
        "self" if !prefix.is_empty() && is_stdlib_path(prefix) => {
            if let Some(name) = prefix.rsplit("::").next() {
                out.insert(name.to_string());
            }
        }
        _ => {}
    }
}

/// Recursively walks the AST collecting relationships.
fn collect_relationships(
    node: tree_sitter::Node,
    src: &[u8],
    std_aliases: &HashSet<String>,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    // Test-only code (`#[cfg(test)]` / `#[test]` items) is not part of the
    // shipped dependency graph. A test-only file module (`#[cfg(test)] mod
    // x;`) is still declared, but marked so the graph can treat that file as
    // test code too.
    if is_test_only_item(node, src) {
        if node.kind() == "mod_item" && node.child_by_field_name("body").is_none() {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(name_node, src);
                add_rel("declares", name, CFG_TEST_DETAIL, rels, seen);
            }
        }
        return;
    }
    match node.kind() {
        "use_declaration" => {
            flatten_use_tree(node, src, "", rels, seen);
            return;
        }
        "impl_item" => {
            extract_impl_relationships(node, src, rels, seen);
        }
        "mod_item" => {
            extract_mod_declaration(node, src, rels, seen);
        }
        "function_item" | "function_signature_item" => {
            collect_function_type_refs(node, src, rels, seen);
        }
        "struct_item" => {
            collect_struct_type_refs(node, src, rels, seen);
        }
        "enum_item" => {
            collect_enum_type_refs(node, src, rels, seen);
        }
        "type_item" | "const_item" | "static_item" => {
            collect_annotated_type_ref(node, src, rels, seen);
        }
        "trait_item" => {
            collect_trait_type_refs(node, src, rels, seen);
        }
        "call_expression" => {
            extract_call(node, src, std_aliases, rels, seen);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            collect_relationships(child, src, std_aliases, rels, seen);
        }
    }
}

fn flatten_use_tree(
    node: tree_sitter::Node,
    src: &[u8],
    prefix: &str,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    match node.kind() {
        "use_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    flatten_use_tree(child, src, prefix, rels, seen);
                }
            }
        }
        "scoped_identifier" | "identifier" | "crate" => {
            let full_path = join_prefix(prefix, node_text(node, src));
            if !is_stdlib_path(&full_path) {
                let target = full_path.rsplit("::").next().unwrap_or(&full_path);
                add_rel("imports", target, &full_path, rels, seen);
            }
        }
        "scoped_use_list" => {
            let path_text = node
                .child_by_field_name("path")
                .map(|p| node_text(p, src))
                .unwrap_or("");
            let new_prefix = join_prefix(prefix, path_text);
            if let Some(list) = node.child_by_field_name("list") {
                flatten_use_tree(list, src, &new_prefix, rels, seen);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    flatten_use_tree(child, src, prefix, rels, seen);
                }
            }
        }
        "use_as_clause" => {
            let path_node = node.child_by_field_name("path");
            let alias_node = node.child_by_field_name("alias");
            if let Some(p) = path_node {
                let full_path = join_prefix(prefix, node_text(p, src));
                if !is_stdlib_path(&full_path) {
                    let target = match alias_node {
                        Some(a) => node_text(a, src),
                        None => full_path.rsplit("::").next().unwrap_or(&full_path),
                    };
                    add_rel("imports", target, &full_path, rels, seen);
                }
            }
        }
        "use_wildcard" => {
            // tree-sitter has no named "path" field on use_wildcard;
            // the module path is an unnamed child node.
            let mut cursor = node.walk();
            let path_text = node
                .children(&mut cursor)
                .find(|c| c.is_named() && c.kind() != "use_wildcard")
                .map(|p| node_text(p, src).to_string());
            let detail = match path_text {
                Some(p) => join_prefix(prefix, &p),
                None => prefix.to_string(),
            };
            if !is_stdlib_path(&detail) {
                add_rel("imports", "*", &detail, rels, seen);
            }
        }
        "self" if !prefix.is_empty() && !is_stdlib_path(prefix) => {
            let target = prefix.rsplit("::").next().unwrap_or(prefix);
            add_rel("imports", target, prefix, rels, seen);
        }
        _ => {}
    }
}

/// v1 call-edge extraction (spec 2026-06-11): bare `fn()` and scoped
/// `module::fn()` / `Type::assoc()` only. Receiver method calls
/// (`obj.method()`, a `field_expression` callee) are deliberately deferred —
/// the receiver type is unknown here and resolution is name-only, so they
/// are the highest-noise form. Macros are `macro_invocation`, not
/// `call_expression`, and stay out naturally.
fn extract_call(
    node: tree_sitter::Node,
    src: &[u8],
    std_aliases: &HashSet<String>,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    match func.kind() {
        "identifier" => {
            let name = node_text(func, src);
            // `use std::mem::take;` + `take(..)` — a stdlib call behind a
            // local binding (Layer 1.6).
            if std_aliases.contains(name) {
                return;
            }
            add_rel("calls", name, name, rels, seen);
        }
        "scoped_identifier" => {
            let full = node_text(func, src);
            if is_stdlib_path(full) {
                return;
            }
            // `use std::fs;` + `fs::write(..)` — the dominant measured
            // false-edge class (Layer 1.6).
            if let Some(first) = full.split("::").next() {
                if std_aliases.contains(first) {
                    return;
                }
            }
            let segments: Vec<&str> = full.split("::").collect();
            let last = segments.last().copied().unwrap_or(full);
            // Type::assoc() targets the TYPE (uppercase-initial second-from-
            // right segment): method_to_files["new"]-style names have many
            // candidates and would inflate unresolvedCount, while the type's
            // defining file is unique. Mirrors the Java uppercase-receiver
            // route. Common/std types (Vec::new, String::from) and Self are
            // filtered with the same predicate as type_ref.
            let target = if segments.len() >= 2 {
                let second = segments[segments.len() - 2];
                if second.starts_with(|c: char| c.is_uppercase()) {
                    if !should_emit_type_ref(second) {
                        return;
                    }
                    second
                } else if PRIMITIVE_TYPES.contains(second) {
                    // u128::from(x) etc.: lowercase primitive segments dodge
                    // the uppercase-Type route above, and the bare fn name
                    // ("from") would file-attribute to an arbitrary impl —
                    // a measured false-edge class (map-split spec, Layer 1.5).
                    return;
                } else {
                    last
                }
            } else {
                last
            };
            add_rel("calls", target, full, rels, seen);
        }
        _ => {}
    }
}

fn join_prefix(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}

fn is_stdlib_path(path: &str) -> bool {
    let first = path.split("::").next().unwrap_or(path);
    matches!(first, "std" | "core" | "alloc")
}

fn extract_impl_relationships(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(trait_node) = node.child_by_field_name("trait") {
        let trait_text = node_text(trait_node, src);
        let trait_name = trait_text.rsplit("::").next().unwrap_or(trait_text);
        add_rel("implements", trait_name, trait_text, rels, seen);
    }

    if let Some(type_node) = node.child_by_field_name("type") {
        collect_type_identifiers(type_node, src, rels, seen);
    }
}

/// `detail` of a `declares` relationship for a `#[cfg(test)] mod x;` file module.
pub const CFG_TEST_DETAIL: &str = "#[cfg(test)]";

/// True when `node` is an item carrying `#[cfg(test)]` or `#[test]` in the
/// attributes directly above it.
fn is_test_only_item(node: tree_sitter::Node, src: &[u8]) -> bool {
    let kind = node.kind();
    if !(kind.ends_with("_item") || kind == "use_declaration") || kind == "attribute_item" {
        return false;
    }
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        match p.kind() {
            "attribute_item" => {
                let text: String = node_text(p, src)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                if text == "#[test]" || text == "#[cfg(test)]" {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        prev = p.prev_sibling();
    }
    false
}

/// Only emits `declares` for file modules (`mod foo;`), not inline `mod foo { ... }`.
fn extract_mod_declaration(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if node.child_by_field_name("body").is_some() {
        return;
    }
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(name_node, src);
        add_rel("declares", name, name, rels, seen);
    }
}

fn collect_function_type_refs(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(params) = node.child_by_field_name("parameters") {
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            if child.kind() == "parameter" || child.kind() == "self_parameter" {
                if let Some(type_node) = child.child_by_field_name("type") {
                    collect_type_identifiers(type_node, src, rels, seen);
                }
            }
        }
    }
    if let Some(ret) = node.child_by_field_name("return_type") {
        collect_type_identifiers(ret, src, rels, seen);
    }
}

fn collect_trait_type_refs(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_signature_item" || child.kind() == "function_item" {
                collect_function_type_refs(child, src, rels, seen);
            }
        }
    }
}

fn collect_struct_type_refs(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(body) = node.child_by_field_name("body") {
        match body.kind() {
            "field_declaration_list" => {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    if child.kind() == "field_declaration" {
                        if let Some(type_node) = child.child_by_field_name("type") {
                            collect_type_identifiers(type_node, src, rels, seen);
                        }
                    }
                }
            }
            "ordered_field_declaration_list" => {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    if child.is_named() && child.kind() != "visibility_modifier" {
                        collect_type_identifiers(child, src, rels, seen);
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_enum_type_refs(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for variant in body.children(&mut cursor) {
            if variant.kind() == "enum_variant" {
                if let Some(variant_body) = variant.child_by_field_name("body") {
                    let mut vcursor = variant_body.walk();
                    for child in variant_body.children(&mut vcursor) {
                        if child.kind() == "field_declaration" {
                            if let Some(type_node) = child.child_by_field_name("type") {
                                collect_type_identifiers(type_node, src, rels, seen);
                            }
                        } else if child.is_named() && child.kind() != "visibility_modifier" {
                            collect_type_identifiers(child, src, rels, seen);
                        }
                    }
                }
            }
        }
    }
}

fn collect_annotated_type_ref(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if let Some(type_node) = node.child_by_field_name("type") {
        collect_type_identifiers(type_node, src, rels, seen);
    }
}

/// Recursively finds type_identifier nodes within a type expression,
/// handling generics, references, arrays, tuples.
fn collect_type_identifiers(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    match node.kind() {
        "type_identifier" => {
            let name = node_text(node, src);
            if should_emit_type_ref(name) {
                add_rel("type_ref", name, name, rels, seen);
            }
        }
        "scoped_type_identifier" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(name_node, src);
                if should_emit_type_ref(name) {
                    let full_text = node_text(node, src);
                    add_rel("type_ref", name, full_text, rels, seen);
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    collect_type_identifiers(child, src, rels, seen);
                }
            }
        }
    }
}

fn should_emit_type_ref(name: &str) -> bool {
    name != "Self"
        && name != "self"
        && !PRIMITIVE_TYPES.contains(name)
        && !COMMON_RUST_TYPES.contains(name)
}

fn add_rel(
    rel_type: &str,
    target_name: &str,
    detail: &str,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    // imports and calls key on detail: distinct paths to same-named targets
    // (a::run vs b::run) must each reach the resolver. Other rel types key
    // on target_name only.
    let key = if rel_type == "imports" || rel_type == "calls" {
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
