use crate::relationship::Relationship;
use crate::ts_util::{node_text, parse_source};
use crate::RelationshipExtractor;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::Path;

/// Extracts relationships from Go source files using tree-sitter AST parsing.
pub struct GoRelationshipExtractor {
    language: tree_sitter::Language,
}

impl GoRelationshipExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_go::LANGUAGE.into(),
        }
    }
}

impl Default for GoRelationshipExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Known Go standard library packages (top-level names).
static GO_STDLIB: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "fmt",
        "os",
        "io",
        "net",
        "log",
        "time",
        "strings",
        "strconv",
        "sync",
        "context",
        "errors",
        "bytes",
        "bufio",
        "sort",
        "math",
        "path",
        "regexp",
        "testing",
        "encoding",
        "crypto",
        "reflect",
        "runtime",
        "flag",
        "http",
        "sql",
        "html",
        "text",
        "unicode",
        "archive",
        "compress",
        "container",
        "database",
        "debug",
        "embed",
        "expvar",
        "go",
        "hash",
        "image",
        "index",
        "internal",
        "mime",
        "plugin",
        "unsafe",
        "syscall",
        "atomic",
    ]
    .into_iter()
    .collect()
});

impl RelationshipExtractor for GoRelationshipExtractor {
    fn extract(&self, path: &Path) -> Vec<Relationship> {
        let Some((tree, source)) = parse_source(path, &self.language, "tree-sitter-go grammar")
        else {
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

fn collect_relationships(
    root: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_declaration" => extract_imports(child, src, rels, seen),
            "type_declaration" => extract_struct_embeddings(child, src, rels, seen),
            _ => {}
        }
    }
}

fn extract_imports(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            // Single import: import "path" or import alias "path"
            "import_spec" => {
                if let Some(path_node) = child.child_by_field_name("path") {
                    let raw = node_text(path_node, src);
                    // Strip surrounding quotes
                    let import_path = raw.trim_matches('"');
                    if let Some(rel) = make_import_rel(import_path) {
                        add_rel_dedup(rel, rels, seen);
                    }
                }
            }
            // Grouped imports: import ( ... )
            "import_spec_list" => {
                let mut list_cursor = child.walk();
                for spec in child.children(&mut list_cursor) {
                    if spec.kind() == "import_spec" {
                        if let Some(path_node) = spec.child_by_field_name("path") {
                            let raw = node_text(path_node, src);
                            let import_path = raw.trim_matches('"');
                            if let Some(rel) = make_import_rel(import_path) {
                                add_rel_dedup(rel, rels, seen);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_struct_embeddings(
    type_decl: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let mut cursor = type_decl.walk();
    for child in type_decl.children(&mut cursor) {
        if child.kind() == "type_spec" {
            if let Some(type_node) = child.child_by_field_name("type") {
                if type_node.kind() == "struct_type" {
                    extract_embeddings_from_struct(type_node, src, rels, seen);
                }
            }
        }
    }
}

fn extract_embeddings_from_struct(
    struct_node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    // struct_type → field_declaration_list → field_declaration (embedded)
    let mut cursor = struct_node.walk();
    for child in struct_node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let mut field_cursor = child.walk();
            for field in child.children(&mut field_cursor) {
                if field.kind() == "field_declaration" {
                    // An embedded field has no name field — just a type
                    // Check: does it have "name" field?  If not, it's embedded.
                    if field.child_by_field_name("name").is_none() {
                        if let Some(type_node) = field.child_by_field_name("type") {
                            let type_name = extract_embedded_type_name(type_node, src);
                            if !type_name.is_empty() {
                                let key = format!("extends:{type_name}");
                                if seen.insert(key) {
                                    rels.push(Relationship {
                                        rel_type: "extends".to_string(),
                                        target_name: type_name.clone(),
                                        detail: type_name,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Extracts the base type name from an embedded field type node.
/// Handles: `BaseServer`, `*model.Server`, `*Server`, `pkg.Type`.
fn extract_embedded_type_name(type_node: tree_sitter::Node, src: &[u8]) -> String {
    match type_node.kind() {
        "pointer_type" => {
            // Pointer embedding: *BaseType
            let mut cursor = type_node.walk();
            for child in type_node.children(&mut cursor) {
                if child.is_named() {
                    return extract_embedded_type_name(child, src);
                }
            }
            node_text(type_node, src).to_string()
        }
        "qualified_type" => {
            // pkg.Type — use the full "pkg.Type" text as target
            node_text(type_node, src).to_string()
        }
        "type_identifier" => node_text(type_node, src).to_string(),
        _ => node_text(type_node, src).to_string(),
    }
}

fn make_import_rel(import_path: &str) -> Option<Relationship> {
    // Paths with a dot in the first segment (e.g. github.com/...) are kept:
    // the repo's own packages look like that too (`module
    // github.com/spf13/cobra`), and only go.mod can tell them apart. The
    // graph builder drops the ones outside the module (#24).
    let first_segment = import_path.split('/').next().unwrap_or(import_path);
    // Filter standard library top-level packages
    if GO_STDLIB.contains(first_segment) {
        return None;
    }
    Some(Relationship {
        rel_type: "imports".to_string(),
        target_name: import_path.to_string(),
        detail: import_path.to_string(),
    })
}

fn add_rel_dedup(rel: Relationship, rels: &mut Vec<Relationship>, seen: &mut HashSet<String>) {
    let key = format!("{}:{}", rel.rel_type, rel.detail);
    if seen.insert(key) {
        rels.push(rel);
    }
}
