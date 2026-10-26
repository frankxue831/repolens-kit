use crate::relationship::Relationship;
use crate::ts_util::{node_text, parse_source};
use crate::RelationshipExtractor;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::Path;

/// Common Java types to filter from calls and type_ref relationships.
static COMMON_JAVA_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "String",
        "Object",
        "List",
        "Map",
        "Set",
        "Collection",
        "Collections",
        "ArrayList",
        "HashMap",
        "HashSet",
        "LinkedHashSet",
        "LinkedHashMap",
        "LinkedList",
        "TreeMap",
        "TreeSet",
        "ConcurrentHashMap",
        "Optional",
        "Stream",
        "Collectors",
        "Arrays",
        "Objects",
        "Integer",
        "Long",
        "Short",
        "Byte",
        "Float",
        "Double",
        "Boolean",
        "Character",
        "Void",
        "Class",
        "Enum",
        "Number",
        "Math",
        "Iterable",
        "Iterator",
        "Comparable",
        "Runnable",
        "Callable",
        "Exception",
        "RuntimeException",
        "Throwable",
        "Error",
        "IOException",
        "UncheckedIOException",
        "IllegalArgumentException",
        "IllegalStateException",
        "NullPointerException",
        "UnsupportedOperationException",
        "StringBuilder",
        "StringBuffer",
        "Pattern",
        "Matcher",
        "Path",
        "Files",
        "Paths",
        "File",
        "URI",
        "URL",
        "Instant",
        "Duration",
        "LocalDate",
        "LocalDateTime",
        "ZonedDateTime",
        "Level",
        "Logger",
        "LogManager",
        "Thread",
        "System",
        "Runtime",
        "ProcessBuilder",
        "Comparator",
        "Function",
        "Supplier",
        "Consumer",
        "Predicate",
        "BiFunction",
        "CompletableFuture",
        "Future",
        "ExecutorService",
        "Executors",
        "AtomicInteger",
        "AtomicLong",
        "AtomicReference",
        "MessageDigest",
        "HexFormat",
        "Base64",
    ]
    .into_iter()
    .collect()
});

static PRIMITIVE_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "int", "long", "short", "byte", "float", "double", "boolean", "char", "void",
    ]
    .into_iter()
    .collect()
});

/// Extracts relationships from Java source files using tree-sitter AST parsing.
pub struct JavaRelationshipExtractor {
    language: tree_sitter::Language,
}

impl JavaRelationshipExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_java::LANGUAGE.into(),
        }
    }
}

impl Default for JavaRelationshipExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl RelationshipExtractor for JavaRelationshipExtractor {
    fn extract(&self, path: &Path) -> Vec<Relationship> {
        let Some((tree, source)) = parse_source(path, &self.language, "tree-sitter-java grammar")
        else {
            return Vec::new();
        };
        self.extract_from_tree(&tree, &source)
    }

    fn extract_from_tree(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Relationship> {
        let root = tree.root_node();
        let src = source.as_bytes();

        // Build external types set from imports
        let package_prefix = find_project_package_prefix(root, src);
        let external_types = build_external_types(root, src, &package_prefix);

        let mut rels = Vec::new();
        let mut seen = HashSet::new();

        collect_all_rels(root, src, &external_types, &mut rels, &mut seen);
        rels
    }
}

/// Derive project root package prefix (e.g., "com.repolens" from "com.repolens.core").
fn find_project_package_prefix(root: tree_sitter::Node, src: &[u8]) -> String {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_declaration" {
            let mut inner = child.walk();
            for c in child.children(&mut inner) {
                if c.kind() == "scoped_identifier" || c.kind() == "identifier" {
                    let pkg = node_text(c, src);
                    let first_dot = pkg.find('.');
                    let second_dot =
                        first_dot.and_then(|i| pkg[i + 1..].find('.').map(|j| i + 1 + j));
                    return match second_dot {
                        Some(i) => pkg[..i].to_string(),
                        None => pkg.to_string(),
                    };
                }
            }
        }
    }
    String::new()
}

/// Build set of simple type names from third-party (non-project) imports.
fn build_external_types(
    root: tree_sitter::Node,
    src: &[u8],
    project_prefix: &str,
) -> HashSet<String> {
    let mut external = HashSet::new();
    let prefix_dot = format!("{project_prefix}.");
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "import_declaration" {
            let import_path = get_import_path(child, src);
            if import_path.is_empty()
                || import_path.starts_with("java.")
                || import_path.starts_with("javax.")
                || import_path.contains('*')
            {
                continue;
            }
            if !project_prefix.is_empty() && !import_path.starts_with(&prefix_dot) {
                if let Some(simple) = import_path.rsplit('.').next() {
                    external.insert(simple.to_string());
                }
            }
        }
    }
    external
}

fn get_import_path(node: tree_sitter::Node, src: &[u8]) -> String {
    let mut inner = node.walk();
    for child in node.children(&mut inner) {
        if child.kind() == "scoped_identifier" || child.kind() == "identifier" {
            return node_text(child, src).to_string();
        }
    }
    String::new()
}

/// Recursively walks the full AST collecting all relationship types.
fn collect_all_rels(
    node: tree_sitter::Node,
    src: &[u8],
    external: &HashSet<String>,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    match node.kind() {
        "import_declaration" => {
            extract_import(node, src, rels, seen);
        }
        "class_declaration"
        | "interface_declaration"
        | "record_declaration"
        | "enum_declaration" => {
            extract_extends_implements(node, src, rels, seen);
        }
        "method_invocation" => {
            extract_method_call(node, src, external, rels, seen);
        }
        "object_creation_expression" | "local_variable_declaration" | "field_declaration" => {
            collect_type_identifiers(node, src, external, rels, seen);
        }
        "method_declaration" => {
            // Return type
            if let Some(type_node) = node.child_by_field_name("type") {
                collect_type_identifiers(type_node, src, external, rels, seen);
            }
            // Parameter types
            if let Some(params) = node.child_by_field_name("parameters") {
                let mut cursor = params.walk();
                for child in params.children(&mut cursor) {
                    if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
                        if let Some(type_node) = child.child_by_field_name("type") {
                            collect_type_identifiers(type_node, src, external, rels, seen);
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // Always recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_rels(child, src, external, rels, seen);
    }
}

fn extract_import(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let import_path = get_import_path(node, src);
    if import_path.is_empty()
        || import_path.starts_with("java.")
        || import_path.starts_with("javax.")
    {
        return;
    }

    let target = import_path
        .rsplit('.')
        .next()
        .unwrap_or(&import_path)
        .to_string();
    add_if_new("imports", &target, &import_path, rels, seen);
}

fn extract_extends_implements(
    node: tree_sitter::Node,
    src: &[u8],
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "superclass" => {
                let mut inner = child.walk();
                for c in child.children(&mut inner) {
                    if c.kind() == "type_identifier" {
                        let name = node_text(c, src);
                        add_if_new("extends", name, name, rels, seen);
                    }
                }
            }
            "super_interfaces" | "interfaces" => {
                let mut inner = child.walk();
                for c in child.children(&mut inner) {
                    if c.kind() == "type_list" {
                        let mut tl_cursor = c.walk();
                        for t in c.children(&mut tl_cursor) {
                            if t.kind() == "type_identifier" {
                                let name = node_text(t, src);
                                add_if_new("implements", name, name, rels, seen);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_method_call(
    node: tree_sitter::Node,
    src: &[u8],
    external: &HashSet<String>,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let method_name = match node.child_by_field_name("name") {
        Some(n) => node_text(n, src).to_string(),
        None => return,
    };

    if let Some(obj) = node.child_by_field_name("object") {
        let scope = node_text(obj, src);

        // Extract type name from scope (first segment before dot, if uppercase)
        let target = if !scope.is_empty() && scope.starts_with(|c: char| c.is_uppercase()) {
            match scope.find('.') {
                Some(i) => &scope[..i],
                None => scope,
            }
        } else {
            scope
        };

        // Variable-scoped calls (lowercase) or ALL_CAPS constants
        if target.is_empty()
            || !target.starts_with(|c: char| c.is_uppercase())
            || target.chars().all(|c| c.is_uppercase() || c == '_')
        {
            let is_all_caps = scope.chars().all(|c| c.is_uppercase() || c == '_');
            if !COMMON_JAVA_TYPES.contains(scope) && !is_all_caps {
                let detail = format!("{scope}.{method_name}");
                add_if_new("calls", &method_name, &detail, rels, seen);
            }
            return;
        }

        // Skip common and external types
        if COMMON_JAVA_TYPES.contains(target) || external.contains(target) {
            return;
        }

        let detail = format!("{scope}.{method_name}");
        add_if_new("calls", target, &detail, rels, seen);
    } else {
        // Unscoped call
        add_if_new("calls", &method_name, &method_name, rels, seen);
    }
}

/// Recursively finds all type_identifier nodes within a type expression
/// (handles generics like List<Foo>, Map<String, Bar>).
fn collect_type_identifiers(
    node: tree_sitter::Node,
    src: &[u8],
    external: &HashSet<String>,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    if node.kind() == "type_identifier" {
        let name = node_text(node, src);
        if !PRIMITIVE_TYPES.contains(name)
            && !COMMON_JAVA_TYPES.contains(name)
            && !external.contains(name)
        {
            add_if_new("type_ref", name, name, rels, seen);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_identifiers(child, src, external, rels, seen);
    }
}

/// Deduplicates relationships by `type:targetName` key, matching Java's addIfNew.
fn add_if_new(
    rel_type: &str,
    target_name: &str,
    detail: &str,
    rels: &mut Vec<Relationship>,
    seen: &mut HashSet<String>,
) {
    let key = format!("{rel_type}:{target_name}");
    if seen.insert(key) {
        rels.push(Relationship {
            rel_type: rel_type.to_string(),
            target_name: target_name.to_string(),
            detail: detail.to_string(),
        });
    }
}
