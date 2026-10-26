use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path};

use rayon::prelude::*;
use tracing::info;

use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::graph::{
    FileGraph, GraphEdge, GraphNode, GraphResolutionMetrics, UnresolvedImport, UnresolvedImports,
};
use repolens_core::path_safety::resolve_existing_subpath;
use repolens_extract::registry::RelationshipExtractorRegistry;
use repolens_extract::Relationship;

use crate::rust_crates::RustCrates;

/// Result of building a file-level dependency graph.
pub struct GraphBuildResult {
    pub graph: FileGraph,
    pub metrics: GraphResolutionMetrics,
}

/// Builds a file-level dependency graph from scan entries.
///
/// Uses `RelationshipExtractorRegistry` to extract relationships from content
/// files, then resolves targets to file paths using multi-strategy resolution.
pub struct GraphBuilder {
    rel_registry: RelationshipExtractorRegistry,
}

/// Java relationship types that support same-package resolution.
const TYPE_RESOLVABLE: &[&str] = &["imports", "extends", "implements", "type_ref"];

/// JS/TS file extensions tried during relative import resolution.
const JS_EXTENSIONS: &[&str] = &[".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".mts", ".cts"];

/// Fully qualified type name → the files that declare it.
///
/// The same class can exist in two source trees (a Maven module copied into
/// another, vendored fixtures or examples). Each copy's references resolve
/// to the copy in its own tree, and a name that is still ambiguous stays
/// unresolved (#29).
#[derive(Default)]
struct QualifiedIndex {
    files: HashMap<String, Vec<String>>,
    /// Java files only: (source root, module root). The source root is the
    /// folder above the package folders (`m/src/main/java`); the module root
    /// drops a trailing `src/<set>/java` so main and test code of one
    /// Maven/Gradle module share it (`m`).
    java_roots: HashMap<String, (String, String)>,
}

impl QualifiedIndex {
    fn insert(&mut self, qn: &str, path: &str, language: &str) {
        let files = self.files.entry(qn.to_string()).or_default();
        if !files.iter().any(|f| f == path) {
            files.push(path.to_string());
        }
        if language == "Java" && !self.java_roots.contains_key(path) {
            if let Some(roots) = java_roots(qn, path) {
                self.java_roots.insert(path.to_string(), roots);
            }
        }
    }

    fn len(&self) -> usize {
        self.files.len()
    }

    fn keys(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }

    /// The file declaring `qn` as seen from `from`. One declaring file wins
    /// outright. With several, a Java source picks the one in its own
    /// source root, then its own module; anything else is ambiguous. Other
    /// languages keep the last declaring file, as before.
    fn get(&self, qn: &str, from: &str) -> Option<&String> {
        let files = self.files.get(qn)?;
        if files.len() == 1 {
            return files.first();
        }
        let Some(src) = self.java_roots.get(from) else {
            return files.last();
        };
        let pick = |same: &dyn Fn(&(String, String)) -> bool| -> Option<Option<&String>> {
            let hits: Vec<&String> = files
                .iter()
                .filter(|f| self.java_roots.get(*f).is_some_and(same))
                .collect();
            match hits.len() {
                0 => None,
                1 => Some(Some(hits[0])),
                _ => Some(None),
            }
        };
        pick(&|r| r.0 == src.0)
            .or_else(|| pick(&|r| r.1 == src.1))
            .flatten()
    }

    /// Whether two Java files are in the same module. True when either
    /// file's tree is unknown, so non-Java code keeps its old behaviour.
    fn same_module(&self, a: &str, b: &str) -> bool {
        match (self.java_roots.get(a), self.java_roots.get(b)) {
            (Some(x), Some(y)) => x.1 == y.1,
            _ => true,
        }
    }
}

/// Source and module roots of a Java file, from one of its qualified type
/// names: `m/src/main/java/com/x/A.java` with `com.x.A` gives
/// (`m/src/main/java`, `m`). `None` when the folders don't match the package.
fn java_roots(qn: &str, path: &str) -> Option<(String, String)> {
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let pkg = qn.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
    let pkg_dir = pkg.replace('.', "/");
    let source_root = if pkg_dir.is_empty() {
        dir
    } else if dir == pkg_dir {
        ""
    } else {
        dir.strip_suffix(pkg_dir.as_str())?.strip_suffix('/')?
    };
    let segs: Vec<&str> = source_root.split('/').collect();
    let module_root =
        if segs.len() >= 3 && segs[segs.len() - 3] == "src" && segs[segs.len() - 1] == "java" {
            segs[..segs.len() - 3].join("/")
        } else {
            source_root.to_string()
        };
    Some((source_root.to_string(), module_root))
}

/// Bundles resolution maps to avoid parameter sprawl.
struct ResolutionContext<'a> {
    qualified_to_file: &'a QualifiedIndex,
    simple_to_files: &'a HashMap<String, Vec<String>>,
    file_to_package: &'a HashMap<String, String>,
    /// Free/top-level callables (`symbol_type == "function"`). Rust call
    /// resolution selects this map exclusively: bare/scoped fn calls cannot
    /// reach impl methods, so method names must not be candidates.
    function_to_files: &'a HashMap<String, Vec<String>>,
    /// Member callables (`symbol_type == "method"`). Java call resolution
    /// selects this map (map-split spec 2026-06-12).
    method_to_files: &'a HashMap<String, Vec<String>>,
    included_paths: &'a HashSet<String>,
    import_map: HashMap<String, String>,
    source_package: Option<&'a str>,
    go_module_prefix: Option<&'a str>,
    rust_workspace_crates: &'a HashMap<String, String>,
    /// Cargo crate boundaries; empty when the repo has no `Cargo.toml`.
    rust_crates: &'a RustCrates,
    /// Directories that absolute Python imports resolve from (`""` is the
    /// repo root), e.g. `src` or a sub-project holding `pyproject.toml`.
    python_roots: &'a [String],
    /// Top-level Python module/package names found under those roots.
    python_local: &'a HashSet<String>,
}

/// Candidate files for a `calls` target, selected per language: Rust →
/// functions only, Java → methods only, anything else → union (status-quo
/// combined-map semantics until a language opts into call edges).
/// `is_external` MUST use the same selection (Codex D2) so a name outside
/// the selected namespace classifies external rather than unresolved.
fn calls_candidates<'b>(language: &str, name: &str, ctx: &'b ResolutionContext) -> Vec<&'b String> {
    let from = |m: &'b HashMap<String, Vec<String>>| -> Vec<&'b String> {
        m.get(name).map(|v| v.iter().collect()).unwrap_or_default()
    };
    match language {
        "Rust" => from(ctx.function_to_files),
        "Java" => from(ctx.method_to_files),
        _ => {
            let mut v = from(ctx.function_to_files);
            v.extend(from(ctx.method_to_files));
            v
        }
    }
}

/// Per-entry output of the parallel relationship pass, merged serially afterward.
struct EntryResult {
    language: String,
    /// Resolved, non-self-reference edges in relationship order (pre global dedup);
    /// its length is this entry's resolved-relationship count.
    candidate_edges: Vec<GraphEdge>,
    unresolved_count: i32,
    /// Imports (not name references) among the unresolved relationships.
    unresolved_imports: Vec<UnresolvedImport>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            rel_registry: RelationshipExtractorRegistry::new(),
        }
    }

    /// Build the graph from file entries.
    ///
    /// `pack_dir` is used to locate content files (entry.content_path relative to pack_dir).
    /// `repo_root` is used for go.mod resolution (Go module-path imports).
    pub fn build(
        &self,
        entries: &[FileEntry],
        pack_dir: &Path,
        repo_root: Option<&Path>,
    ) -> GraphBuildResult {
        self.build_with_relationships(entries, pack_dir, repo_root, None)
    }

    /// Like [`build`](Self::build), but with an optional in-memory map of
    /// pre-extracted relationships keyed by entry path. When an entry is present
    /// in `rels_by_path` its relationships are used directly (the parse-once
    /// fast path from a full scan — the file was already parsed during entry
    /// materialization); otherwise they are extracted from the packed copy
    /// (incremental carry-forward, non-tree-sitter languages, or graph-from-pack).
    pub fn build_with_relationships(
        &self,
        entries: &[FileEntry],
        pack_dir: &Path,
        repo_root: Option<&Path>,
        rels_by_path: Option<&HashMap<String, Vec<Relationship>>>,
    ) -> GraphBuildResult {
        info!(
            entries = entries.len(),
            pack_dir = %pack_dir.display(),
            "Starting graph build"
        );

        let included: Vec<&FileEntry> = entries.iter().filter(|e| e.included).collect();

        // Build resolution maps from included entry symbols
        let mut qualified_to_file = QualifiedIndex::default();
        let mut simple_to_files: HashMap<String, Vec<String>> = HashMap::new();
        let mut file_to_package: HashMap<String, String> = HashMap::new();
        let mut function_to_files: HashMap<String, Vec<String>> = HashMap::new();
        let mut method_to_files: HashMap<String, Vec<String>> = HashMap::new();
        let mut included_paths: HashSet<String> = HashSet::new();

        for entry in &included {
            included_paths.insert(entry.path.clone());
            for sym in &entry.symbols {
                if matches!(
                    sym.symbol_type.as_str(),
                    "class" | "interface" | "enum" | "record" | "annotation" | "type"
                ) {
                    if let Some(ref qn) = sym.qualified_name {
                        qualified_to_file.insert(qn, &entry.path, &entry.language);
                        if entry.language == "Java" {
                            if let Some(last_dot) = qn.rfind('.') {
                                file_to_package
                                    .insert(entry.path.clone(), qn[..last_dot].to_string());
                            }
                        }
                    }
                    simple_to_files
                        .entry(sym.name.clone())
                        .or_default()
                        .push(entry.path.clone());
                }
                match sym.symbol_type.as_str() {
                    "function" => function_to_files
                        .entry(sym.name.clone())
                        .or_default()
                        .push(entry.path.clone()),
                    "method" => method_to_files
                        .entry(sym.name.clone())
                        .or_default()
                        .push(entry.path.clone()),
                    _ => {}
                }
            }
        }

        info!(
            qualified = qualified_to_file.len(),
            simple = simple_to_files.len(),
            functions = function_to_files.len(),
            methods = method_to_files.len(),
            "Resolution maps built"
        );

        // Build nodes (all included entries, sorted by path)
        let mut nodes: Vec<GraphNode> = included
            .iter()
            .map(|e| GraphNode {
                id: e.path.clone(),
                symbols: e.symbols.iter().map(|s| s.name.clone()).collect(),
            })
            .collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));

        // Extract relationships and resolve edges
        let mut edges: Vec<GraphEdge> = Vec::new();
        let mut edge_keys: HashSet<String> = HashSet::new();
        let mut resolved_edges: i32 = 0;
        let mut unresolved_edges: i32 = 0;
        // BTreeMap (not HashMap) so the serialized per-language keys in
        // graph-metrics.json come out in a stable, sorted order run-to-run.
        let mut per_language: BTreeMap<String, Vec<i32>> = BTreeMap::new();

        let go_module_prefix = repo_root.and_then(detect_go_module_prefix);
        // Crate boundaries from Cargo.toml. When manifests are present they
        // are the only source of crate names; the directory-name heuristic
        // is a fallback for packs without them (it would otherwise map an
        // external `regex` crate onto a workspace dir named `regex`).
        let rust_crates = RustCrates::from_entries(&included, pack_dir);
        let rust_workspace_crates = if rust_crates.is_empty() {
            detect_rust_workspace_crates(&included_paths)
        } else {
            rust_crates.lib_roots()
        };
        let python_roots = detect_python_roots(&included_paths);
        let python_local = python_local_top_levels(&included_paths, &python_roots);
        let python_init_words = python_init_words(&included, pack_dir);

        // Extract relationships and resolve targets per entry IN PARALLEL, then merge
        // serially in the original included-entry order. The merge preserves edge
        // insertion order, the first-wins edge dedup, and the per-language / resolved /
        // unresolved metric counts exactly, while moving the dominant per-file
        // parse + resolve cost off the critical path. Resolution maps (above) stay serial.
        let entry_results: Vec<Option<EntryResult>> = included
            .par_iter()
            .map(|entry| {
                // Parse-once fast path: reuse the relationships the scan phase
                // already extracted in memory when present; otherwise read +
                // parse the packed copy (fallback for incremental carry-forward,
                // non-tree-sitter languages, or graph-from-pack).
                let rels = match rels_by_path.and_then(|m| m.get(&entry.path)) {
                    Some(prefetched) => prefetched.clone(),
                    None => {
                        let content_path = match &entry.content_path {
                            Some(cp) => match resolve_existing_subpath(pack_dir, cp) {
                                Ok(path) => path,
                                Err(_) => return None,
                            },
                            None => return None,
                        };
                        match self.rel_registry.for_language(&entry.language) {
                            Some(extractor) => extractor.extract(&content_path),
                            None => return None,
                        }
                    }
                };
                let rels = if entry.language == "Go" {
                    drop_external_go_imports(rels, go_module_prefix.as_deref())
                } else {
                    rels
                };
                if rels.is_empty() {
                    return None;
                }

                let import_map = if entry.language == "Java" {
                    build_java_import_map(&rels, &qualified_to_file)
                } else {
                    HashMap::new()
                };

                let ctx = ResolutionContext {
                    qualified_to_file: &qualified_to_file,
                    simple_to_files: &simple_to_files,
                    file_to_package: &file_to_package,
                    function_to_files: &function_to_files,
                    method_to_files: &method_to_files,
                    included_paths: &included_paths,
                    import_map,
                    source_package: file_to_package.get(&entry.path).map(|s| s.as_str()),
                    go_module_prefix: go_module_prefix.as_deref(),
                    rust_workspace_crates: &rust_workspace_crates,
                    rust_crates: &rust_crates,
                    python_roots: &python_roots,
                    python_local: &python_local,
                };

                let mut candidate_edges: Vec<GraphEdge> = Vec::new();
                let mut unresolved_count: i32 = 0;
                let mut unresolved_imports: Vec<UnresolvedImport> = Vec::new();

                let resolved: Vec<Option<String>> = rels
                    .iter()
                    .map(|rel| resolve_target(&entry.path, &entry.language, rel, &ctx))
                    .collect();
                // Files this file really imports (#21): a name-matched edge
                // (calls / type_ref / extends / implements) is only kept when
                // it lands in one of these, so a bare name like `Entry` or
                // `equals` can't link production code to whichever test file
                // happens to declare the same name.
                // Only runtime imports count: a name edge backed only by a
                // type-only import would turn an erased import back into a
                // cycle-forming dependency (the type-only import edge itself
                // is kept and still marks the target as used).
                let imported: HashSet<&str> = rels
                    .iter()
                    .zip(&resolved)
                    .filter(|(rel, _)| rel.rel_type == "imports")
                    .filter_map(|(_, t)| t.as_deref())
                    .collect();

                for (rel, target) in rels.iter().zip(resolved.iter().cloned()) {
                    match target {
                        Some(ref target_path) if target_path == &entry.path => {
                            // Self-reference: skip without counting (matches Java)
                        }
                        Some(ref target_path)
                            if entry.language == "Rust"
                                && !rust_crates.allows_edge(&entry.path, target_path) =>
                        {
                            // A name match in a crate this crate doesn't
                            // depend on: not a real import. Treated like an
                            // external reference (no edge, not unresolved).
                        }
                        Some(ref target_path)
                            if !name_edge_is_backed_by_import(
                                &entry.path,
                                &entry.language,
                                rel,
                                target_path,
                                &imported,
                                &ctx,
                            ) =>
                        {
                            // A name that matched a declaration in a file this
                            // file never imports: most likely a library name
                            // (`java.lang.reflect.Field`, `Map.Entry`). Treated
                            // as external (no edge, not unresolved).
                        }
                        Some(target_path) => {
                            let detail = if rel.detail.is_empty() {
                                None
                            } else {
                                Some(rel.detail.clone())
                            };
                            candidate_edges.push(GraphEdge {
                                source: entry.path.clone(),
                                target: target_path,
                                edge_type: rel.rel_type.clone(),
                                detail,
                            });
                        }
                        None => {
                            // #42: `from . import name` with no module
                            // `name` and no `name` in the package's
                            // `__init__.py` is a missing module.
                            let missing_py_module = entry.language == "Python"
                                && is_import_type(&rel.rel_type)
                                && python_package_import_is_missing(
                                    &entry.path,
                                    rel,
                                    &python_init_words,
                                );
                            if missing_py_module || !is_external(rel, &entry.language, &ctx) {
                                unresolved_count += 1;
                                // #31: a relative import (`./pkg/x.js`,
                                // Python `.mod`) names a repo path that
                                // isn't in the scan. Recorded so graph
                                // answers can say so. Other unresolved
                                // names (`self::Variant`, library
                                // packages) are mostly external.
                                let import = if rel.detail.is_empty() {
                                    &rel.target_name
                                } else {
                                    &rel.detail
                                };
                                if is_import_type(&rel.rel_type) && import.starts_with('.') {
                                    // #42: Python imports are shown the
                                    // way they are written (`.nope_mod`).
                                    let import = if entry.language == "Python" {
                                        python_dotted(import)
                                    } else {
                                        import.clone()
                                    };
                                    unresolved_imports.push(UnresolvedImport {
                                        source: entry.path.clone(),
                                        import,
                                    });
                                }
                            }
                        }
                    }
                }

                Some(EntryResult {
                    language: entry.language.clone(),
                    candidate_edges,
                    unresolved_count,
                    unresolved_imports,
                })
            })
            .collect();

        // Serial merge in original included-entry order: applies the exact edge dedup
        // (first occurrence wins) and metric accumulation, so output is byte-identical
        // to the previous sequential loop. Note resolved_edges counts every resolved,
        // non-self relationship (duplicates included) = candidate_edges.len().
        let mut unresolved_imports: Vec<UnresolvedImport> = Vec::new();
        for er in entry_results.into_iter().flatten() {
            unresolved_imports.extend(er.unresolved_imports);
            let resolved_here = er.candidate_edges.len() as i32;
            let lang_stats = per_language
                .entry(er.language)
                .or_insert_with(|| vec![0, 0]);
            lang_stats[0] += resolved_here;
            lang_stats[1] += er.unresolved_count;
            resolved_edges += resolved_here;
            unresolved_edges += er.unresolved_count;

            for edge in er.candidate_edges {
                let edge_key = format!("{}|{}|{}", edge.source, edge.target, edge.edge_type);
                if edge_keys.insert(edge_key) {
                    edges.push(edge);
                }
            }
        }

        drop_rust_test_only_edges(&mut edges);

        let total_relationships = resolved_edges + unresolved_edges;

        info!(
            nodes = nodes.len(),
            edges = edges.len(),
            total = total_relationships,
            resolved = resolved_edges,
            unresolved = unresolved_edges,
            "Graph build complete"
        );

        GraphBuildResult {
            graph: FileGraph { nodes, edges },
            metrics: GraphResolutionMetrics {
                total_relationships,
                resolved_edges,
                unresolved_edges,
                per_language: Some(per_language),
                unresolved_imports: UnresolvedImports::from_all(unresolved_imports),
            },
        }
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Drop Go imports of packages outside this module. The extractor keeps
/// every non-stdlib import because a path like `github.com/spf13/cobra/doc`
/// may be the repo's own package; only go.mod says which (#24). A path whose
/// first segment has a dot and isn't under the module path is a third-party
/// package, as before.
fn drop_external_go_imports(rels: Vec<Relationship>, module: Option<&str>) -> Vec<Relationship> {
    let in_module = |path: &str| {
        module
            .is_some_and(|m| path == m || path.strip_prefix(m).is_some_and(|r| r.starts_with('/')))
    };
    rels.into_iter()
        .filter(|rel| {
            if rel.rel_type != "imports" || in_module(&rel.target_name) {
                return true;
            }
            let first = rel.target_name.split('/').next().unwrap_or_default();
            !first.contains('.')
        })
        .collect()
}

/// Detect Go module prefix from go.mod in the repo root.
fn detect_go_module_prefix(repo_root: &Path) -> Option<String> {
    let go_mod = repo_root.join("go.mod");
    let content = std::fs::read_to_string(&go_mod).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Relationship types that come from an import statement. `type_import` is
/// a type-only import (Python `if TYPE_CHECKING:`, TypeScript `import type`).
fn is_import_type(rel_type: &str) -> bool {
    rel_type == "imports" || rel_type == repolens_extract::TYPE_IMPORT
}

/// Java production code can't see the test source set (`src/test/...`),
/// even when both declare the same package.
fn is_java_test_source(path: &str) -> bool {
    path.starts_with("src/test/") || path.contains("/src/test/")
}

/// Languages whose name-matched edges must line up with an import (#21).
/// Rust keeps its own crate-boundary rule and Go emits no name edges.
fn requires_import_backing(language: &str) -> bool {
    matches!(language, "Java" | "JavaScript" | "TypeScript" | "Python")
}

/// Whether a resolved edge is backed by a real import (or, for Java, by
/// the same package or a fully qualified name, which need no import).
/// Import edges themselves always pass.
fn name_edge_is_backed_by_import(
    source_path: &str,
    language: &str,
    rel: &Relationship,
    target_path: &str,
    imported: &HashSet<&str>,
    ctx: &ResolutionContext,
) -> bool {
    if is_import_type(&rel.rel_type) || !requires_import_backing(language) {
        return true;
    }
    if imported.contains(target_path) {
        return true;
    }
    if language != "Java" {
        return false;
    }
    // Same package: Java needs no import.
    if let (Some(src_pkg), Some(tgt_pkg)) = (
        ctx.file_to_package.get(source_path),
        ctx.file_to_package.get(target_path),
    ) {
        if src_pkg == tgt_pkg
            && ctx.qualified_to_file.same_module(source_path, target_path)
            && (is_java_test_source(source_path) || !is_java_test_source(target_path))
        {
            return true;
        }
    }
    // Named in an import (single-type or wildcard), even if the import
    // relationship itself resolved to a different (outer) file.
    if ctx.import_map.values().any(|q| {
        ctx.qualified_to_file
            .get(q, source_path)
            .map(|f| f.as_str())
            == Some(target_path)
    }) {
        return true;
    }
    // A fully qualified reference such as `com.x.Foo`.
    rel.target_name.contains('.')
        && ctx
            .qualified_to_file
            .get(&rel.target_name, source_path)
            .map(|f| f.as_str())
            == Some(target_path)
}

/// Directories that absolute Python imports resolve from: the repo root,
/// `src/`, and every directory holding a `pyproject.toml`, `setup.py` or
/// `setup.cfg` (plus its `src/`), deepest first so a sub-project wins.
fn detect_python_roots(included_paths: &HashSet<String>) -> Vec<String> {
    let mut roots: HashSet<String> = HashSet::new();
    roots.insert(String::new());
    roots.insert("src".to_string());
    let manifests = ["pyproject.toml", "setup.py", "setup.cfg"];
    for path in included_paths {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        if manifests.contains(&name) {
            roots.insert(dir.to_string());
            roots.insert(join_repo_path(dir, "src"));
        }
    }
    let mut roots: Vec<String> = roots.into_iter().collect();
    roots.sort_by(|a, b| {
        b.matches('/')
            .count()
            .cmp(&a.matches('/').count())
            .then_with(|| b.len().cmp(&a.len()))
            .then_with(|| a.cmp(b))
    });
    roots
}

/// Resolve an absolute Python module path (`app.render.runner`) against the
/// package roots: `<root>/app/render/runner.py`, then
/// `<root>/app/render/runner/__init__.py`. Exact module only, no fallback to
/// a parent package (that is a separate relationship) and no name matching.
fn resolve_python_module(module: &str, ctx: &ResolutionContext) -> Option<String> {
    let rel_path = module.replace('.', "/");
    for root in ctx.python_roots {
        let base = join_repo_path(root, &rel_path);
        for candidate in [format!("{base}.py"), format!("{base}/__init__.py")] {
            if ctx.included_paths.contains(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolve an absolute import from a script that is not inside a package
/// (its folder has no `__init__.py`): Python puts the script's own folder
/// first on `sys.path`, so `from collect import Collector` finds `collect.py`
/// beside it, even when a package root also has a `collect.py`. Consulted
/// before the package roots, only when the file exists, and never for a
/// module inside a package (Python 3 has no implicit relative imports).
fn resolve_python_script_sibling(
    source_path: &str,
    module: &str,
    ctx: &ResolutionContext,
) -> Option<String> {
    let dir = source_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    if ctx
        .included_paths
        .contains(&join_repo_path(dir, "__init__.py"))
    {
        return None;
    }
    let base = join_repo_path(dir, &module.replace('.', "/"));
    [format!("{base}.py"), format!("{base}/__init__.py")]
        .into_iter()
        .find(|c| ctx.included_paths.contains(c))
}

/// Top-level module and package names that live in this repo, under any
/// package root (`app` for `app/render/runner.py`, `main` for `main.py`).
fn python_local_top_levels(included_paths: &HashSet<String>, roots: &[String]) -> HashSet<String> {
    let mut tops = HashSet::new();
    for path in included_paths.iter().filter(|p| p.ends_with(".py")) {
        for root in roots {
            let rest = if root.is_empty() {
                Some(path.as_str())
            } else {
                path.strip_prefix(root.as_str())
                    .and_then(|r| r.strip_prefix('/'))
            };
            if let Some(rest) = rest {
                let top = rest.split('/').next().unwrap_or("");
                tops.insert(top.trim_end_matches(".py").to_string());
            }
        }
    }
    tops
}

/// Detect Rust workspace crate source roots from included file paths.
///
/// Scans for paths matching `{prefix}/{dir-name}/src/{file}` and derives a mapping
/// from the underscored crate directory name to its `src/` root path.
/// For example, `crates/repolens-core/src/lib.rs` produces:
///   `"repolens_core"` → `"crates/repolens-core/src"`
fn detect_rust_workspace_crates(included_paths: &HashSet<String>) -> HashMap<String, String> {
    let mut crate_map: HashMap<String, String> = HashMap::new();
    for path in included_paths {
        // Match: .../{dir}/src/{anything}.rs
        if let Some(src_idx) = path.find("/src/") {
            if path.ends_with(".rs") {
                let prefix = &path[..src_idx];
                // The crate directory name is the last component of prefix
                if let Some(dir_name) = prefix.rsplit('/').next() {
                    let code_name = dir_name.replace('-', "_");
                    let src_root = format!("{prefix}/src");
                    crate_map.entry(code_name).or_insert(src_root);
                }
            }
        }
    }
    crate_map
}

/// Build Java import map: simple name → qualified name.
/// Wildcard imports (ending `.*`) expand to all direct children in qualified_to_file.
fn build_java_import_map(
    rels: &[Relationship],
    qualified_to_file: &QualifiedIndex,
) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for rel in rels {
        if rel.rel_type != "imports" {
            continue;
        }
        let detail = &rel.detail;
        if detail.ends_with(".*") {
            let prefix = &detail[..detail.len() - 1];
            for qn in qualified_to_file.keys() {
                if let Some(suffix) = qn.strip_prefix(prefix) {
                    if !suffix.contains('.') {
                        map.entry(suffix.to_string()).or_insert_with(|| qn.clone());
                    }
                }
            }
        } else if !detail.is_empty() {
            map.insert(rel.target_name.clone(), detail.clone());
        }
    }
    map
}

/// Try to resolve a name via Java-specific strategies: import map, then same-package.
/// Used by both the main resolution pipeline and dotted-name prefix resolution.
fn resolve_java_name(name: &str, source_path: &str, ctx: &ResolutionContext) -> Option<String> {
    if let Some(qualified) = ctx.import_map.get(name) {
        if let Some(file) = ctx.qualified_to_file.get(qualified, source_path) {
            return Some(file.clone());
        }
    }
    if let Some(pkg) = ctx.source_package {
        let qualified = format!("{}.{}", pkg, name);
        if let Some(file) = ctx.qualified_to_file.get(&qualified, source_path) {
            return Some(file.clone());
        }
    }
    None
}

/// Try to resolve a relationship target to a file path.
fn resolve_target(
    source_path: &str,
    language: &str,
    rel: &Relationship,
    ctx: &ResolutionContext,
) -> Option<String> {
    // Strategy 0: Path-based (relative imports for JS/TS/Python)
    if rel.detail.starts_with("./") || rel.detail.starts_with("../") {
        if let Some(resolved) =
            resolve_relative_path(source_path, &rel.detail, language, ctx.included_paths)
        {
            return Some(resolved);
        }
    }

    // Strategy 0.3: Python imports resolve by path only (#23). Absolute
    // `from app.x import y` resolves against the package roots; nothing
    // falls through to bare-name matching (#21).
    if language == "Python" && is_import_type(&rel.rel_type) {
        if rel.detail.starts_with('.') {
            return None;
        }
        // A script's own folder comes first on sys.path; package roots after.
        return resolve_python_script_sibling(source_path, &rel.detail, ctx)
            .or_else(|| resolve_python_module(&rel.detail, ctx));
    }

    // Strategy 0.5: Go module-path
    if rel.rel_type == "imports" && language == "Go" {
        if let Some(prefix) = ctx.go_module_prefix {
            // An import names a package (a folder), not a file. The module
            // path itself is the package in the repo root (#24).
            let prefix_slash = format!("{}/", prefix);
            let dir_prefix = if rel.target_name == prefix {
                Some(String::new())
            } else {
                rel.target_name
                    .strip_prefix(&prefix_slash)
                    .map(|suffix| format!("{}/", suffix))
            };
            if let Some(dir_prefix) = dir_prefix {
                let in_package = |path: &&String| {
                    path.starts_with(&dir_prefix)
                        && path.ends_with(".go")
                        && !path[dir_prefix.len()..].contains('/')
                };
                // The edge stands for the whole package, so pick the file
                // that best names it: a file the package build compiles
                // over a test file, then the file named after the package
                // folder (`cobra.go` for `github.com/spf13/cobra`).
                let pkg_file = format!(
                    "{}.go",
                    rel.target_name.rsplit('/').next().unwrap_or_default()
                );
                let mut files: Vec<&String> =
                    ctx.included_paths.iter().filter(in_package).collect();
                files.sort_by_key(|p| {
                    (
                        p.ends_with("_test.go"),
                        p[dir_prefix.len()..] != pkg_file,
                        p.as_str(),
                    )
                });
                if let Some(path) = files.first() {
                    return Some((*path).clone());
                }
            }
        }
    }

    // Strategy 0.6: Rust mod declaration → child file
    if language == "Rust" && rel.rel_type == "declares" {
        let source_dir = source_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let mod_name = &rel.target_name;
        let candidates = vec![
            format!("{source_dir}/{mod_name}.rs"),
            format!("{source_dir}/{mod_name}/mod.rs"),
        ];
        for candidate in &candidates {
            if ctx.included_paths.contains(candidate) {
                return Some(candidate.clone());
            }
        }
        return None;
    }

    // Strategy 0.7: Rust module-path (crate::, super::, self::)
    if language == "Rust" && rel.rel_type == "imports" {
        if let Some(resolved) = resolve_rust_module_path(source_path, &rel.detail, ctx) {
            return Some(resolved);
        }
    }

    // Strategy 0.8: Rust workspace crate import (e.g. use repolens_core::model::FileEntry)
    if language == "Rust" && rel.rel_type == "imports" && !rel.detail.is_empty() {
        let segments: Vec<&str> = rel.detail.split("::").collect();
        if segments.len() >= 2 {
            let crate_name = segments[0];
            if let Some(src_root) = ctx.rust_workspace_crates.get(crate_name) {
                if let Some(resolved) =
                    resolve_rust_module_segments(src_root, &segments[1..], ctx.included_paths)
                {
                    return Some(resolved);
                }
            }
        }
    }

    // Strategy 1: Qualified from detail (imports with non-empty detail)
    if rel.rel_type == "imports" && !rel.detail.is_empty() {
        if let Some(file) = ctx.qualified_to_file.get(&rel.detail, source_path) {
            return Some(file.clone());
        }
    }

    // Strategy 2: Qualified from target_name
    if let Some(file) = ctx.qualified_to_file.get(&rel.target_name, source_path) {
        return Some(file.clone());
    }

    // Strategy 2.5+2.7: Java import map + same-package
    if language == "Java" {
        // Same-package only applies to type-resolvable relationships
        if TYPE_RESOLVABLE.contains(&rel.rel_type.as_str()) {
            if let Some(file) = resolve_java_name(&rel.target_name, source_path, ctx) {
                return Some(file);
            }
        } else if let Some(qualified) = ctx.import_map.get(&rel.target_name) {
            // For non-type-resolvable (e.g., calls), still try import map but not same-package
            if let Some(file) = ctx.qualified_to_file.get(qualified, source_path) {
                return Some(file.clone());
            }
        }
    }

    // Strategy 3: Simple name (1 candidate only)
    if let Some(candidates) = ctx.simple_to_files.get(&rel.target_name) {
        if candidates.len() == 1 {
            return Some(candidates[0].clone());
        }
    }

    // Strategy 3.5: Call resolution against the language-selected callable
    // namespace (map-split spec 2026-06-12): Rust → free functions, Java →
    // methods, others → union.
    if rel.rel_type == "calls" {
        let candidates = calls_candidates(language, &rel.target_name, ctx);
        if candidates.len() == 1 {
            return Some(candidates[0].clone());
        }
        if language == "Java" {
            for candidate in &candidates {
                if let Some(cpkg) = ctx.file_to_package.get(*candidate) {
                    let cpkg_dot = format!("{}.", cpkg);
                    // An imported class in the candidate's package. When the
                    // import resolves to a file, it must be in the
                    // candidate's own tree (#29).
                    for qualified in ctx.import_map.values() {
                        if qualified.starts_with(&cpkg_dot)
                            && ctx
                                .qualified_to_file
                                .get(qualified, source_path)
                                .is_none_or(|f| ctx.qualified_to_file.same_module(f, candidate))
                        {
                            return Some((*candidate).clone());
                        }
                    }
                    if ctx.source_package == Some(cpkg.as_str())
                        && ctx.qualified_to_file.same_module(source_path, candidate)
                    {
                        return Some((*candidate).clone());
                    }
                }
            }
        }

        // Strategy 3.6: Rust scoped-call detail-path disambiguation
        // (map-split spec, Codex D4). Only scoped free-fn calls — the
        // detail's final segment must equal the target (Type-targeted
        // associated-fn calls have target == Type and bypass this).
        // Exactly-one match or nothing; no tie-breaks.
        if language == "Rust" && candidates.len() >= 2 && rel.detail.contains("::") {
            let segments: Vec<&str> = rel.detail.split("::").collect();
            if segments.last() == Some(&rel.target_name.as_str()) {
                if let Some(target) =
                    disambiguate_scoped_call(source_path, &segments, &candidates, ctx)
                {
                    return Some(target);
                }
            }
        }
    }

    // Strategy 4: Import simple name from detail
    if rel.rel_type == "imports" && rel.detail.contains('.') {
        if let Some(simple) = rel.detail.rsplit('.').next() {
            if let Some(candidates) = ctx.simple_to_files.get(simple) {
                if candidates.len() == 1 {
                    return Some(candidates[0].clone());
                }
            }
        }
    }

    // Strategy 5: Dotted target names (e.g., ScanController.ScanRequest)
    if rel.target_name.contains('.') {
        if let Some(prefix) = rel.target_name.split('.').next() {
            if let Some(file) = ctx.qualified_to_file.get(prefix, source_path) {
                return Some(file.clone());
            }
            if language == "Java" && TYPE_RESOLVABLE.contains(&rel.rel_type.as_str()) {
                if let Some(file) = resolve_java_name(prefix, source_path, ctx) {
                    return Some(file);
                }
            }
            if let Some(candidates) = ctx.simple_to_files.get(prefix) {
                if candidates.len() == 1 {
                    return Some(candidates[0].clone());
                }
            }
        }
    }

    None
}

/// Resolve a relative import path to a file path in the project.
fn resolve_relative_path(
    source_path: &str,
    import_detail: &str,
    language: &str,
    included_paths: &HashSet<String>,
) -> Option<String> {
    let source_dir = source_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let resolved = normalize_relative(source_dir, import_detail);

    if included_paths.contains(&resolved) {
        return Some(resolved);
    }

    let extensions: &[&str] = match language {
        "JavaScript" | "TypeScript" => JS_EXTENSIONS,
        "Python" => &[".py"],
        _ => &[],
    };

    for ext in extensions {
        let with_ext = format!("{}{}", resolved, ext);
        if included_paths.contains(&with_ext) {
            return Some(with_ext);
        }
    }

    // TypeScript ESM convention: `import "./util.js"` names the emitted
    // file; the source is `util.ts` (likewise .jsx/.mjs/.cjs -> .tsx/.mts/.cts).
    if matches!(language, "JavaScript" | "TypeScript") {
        for (js, ts) in [
            (".js", ".ts"),
            (".jsx", ".tsx"),
            (".mjs", ".mts"),
            (".cjs", ".cts"),
            (".js", ".tsx"),
        ] {
            if let Some(stem) = resolved.strip_suffix(js) {
                let candidate = format!("{stem}{ts}");
                if included_paths.contains(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    if matches!(language, "JavaScript" | "TypeScript") {
        for ext in JS_EXTENSIONS {
            let index_path = format!("{}/index{}", resolved, ext);
            if included_paths.contains(&index_path) {
                return Some(index_path);
            }
        }
    }

    if language == "Python" {
        let init_path = format!("{}/__init__.py", resolved);
        if included_paths.contains(&init_path) {
            return Some(init_path);
        }
    }

    None
}

/// Normalize a relative path (handling .. segments) without filesystem access.
fn normalize_relative(base_dir: &str, relative: &str) -> String {
    let mut components: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };

    let rel_path = Path::new(relative);
    for comp in rel_path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                components.pop();
            }
            Component::Normal(seg) => {
                components.push(seg.to_str().unwrap_or(""));
            }
            _ => {}
        }
    }

    components.join("/")
}

/// Resolve a Rust module path (crate::x::y) to a file path.
/// Tries: x/y.rs, x/y/mod.rs
fn resolve_rust_module_path(
    source_path: &str,
    detail: &str,
    ctx: &ResolutionContext,
) -> Option<String> {
    let included_paths = ctx.included_paths;
    let segments: Vec<&str> = detail.split("::").collect();
    if segments.is_empty() {
        return None;
    }

    let module_segments = match segments[0] {
        "crate" => &segments[1..],
        "super" => {
            let base_dir = rust_super_base_dir(source_path)?;
            return resolve_rust_module_segments(&base_dir, &segments[1..], included_paths);
        }
        "self" => {
            let base_dir = rust_self_base_dir(source_path);
            return resolve_rust_module_segments(&base_dir, &segments[1..], included_paths);
        }
        _ => return None,
    };

    // For crate:: paths, resolve from the owning crate's module root.
    let src_root = rust_crate_root(source_path, ctx);
    resolve_rust_module_segments(&src_root, module_segments, included_paths)
}

fn resolve_rust_module_segments(
    base_dir: &str,
    module_segments: &[&str],
    included_paths: &HashSet<String>,
) -> Option<String> {
    if module_segments.is_empty() {
        return None;
    }

    let module_path = module_segments.join("/");
    for candidate in rust_module_candidates(base_dir, &module_path) {
        if included_paths.contains(&candidate) {
            return Some(candidate);
        }
    }

    if module_segments.len() >= 2 {
        let parent_path = module_segments[..module_segments.len() - 1].join("/");
        for candidate in rust_module_candidates(base_dir, &parent_path) {
            if included_paths.contains(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

fn rust_module_candidates(base_dir: &str, module_path: &str) -> [String; 2] {
    let joined = join_repo_path(base_dir, module_path);
    [format!("{joined}.rs"), format!("{joined}/mod.rs")]
}

/// Strategy 3.6 (map-split spec, Codex D4): pick between multiple free-fn
/// candidates using the scoped call's module-path prefix.
///
/// Rooted prefixes (`crate`/`self`/`super`/workspace crate) resolve the
/// module path to a concrete file via the same machinery as import
/// resolution — but STRICTLY (no parent-segment fallback: the fn segment is
/// already stripped, so a miss means no match, not "attribute to the
/// container"). The resolved file must itself be one of the candidates.
/// Relative prefixes suffix-match the candidates' normalized module paths
/// (`…/x.rs` → `…/x`, `…/x/mod.rs` → `…/x`). Exactly one match or nothing.
fn disambiguate_scoped_call(
    source_path: &str,
    segments: &[&str],
    candidates: &[&String],
    ctx: &ResolutionContext,
) -> Option<String> {
    let module_segments = &segments[..segments.len() - 1];
    if module_segments.is_empty() {
        return None;
    }

    let strict_resolve = |base_dir: &str, mods: &[&str]| -> Option<String> {
        if mods.is_empty() {
            return None;
        }
        rust_module_candidates(base_dir, &mods.join("/"))
            .into_iter()
            .find(|c| ctx.included_paths.contains(c))
    };

    let rooted = match module_segments[0] {
        "crate" => {
            let src_root = rust_crate_root(source_path, ctx);
            Some(strict_resolve(&src_root, &module_segments[1..]))
        }
        "self" => Some(strict_resolve(
            &rust_self_base_dir(source_path),
            &module_segments[1..],
        )),
        "super" => rust_super_base_dir(source_path)
            .map(|base| strict_resolve(&base, &module_segments[1..])),
        first => ctx
            .rust_workspace_crates
            .get(first)
            .map(|src_root| strict_resolve(src_root, &module_segments[1..])),
    };

    if let Some(resolved) = rooted {
        let file = resolved?;
        return candidates
            .iter()
            .find(|c| ***c == file)
            .map(|c| (**c).clone());
    }

    // Relative prefix: suffix-match against normalized candidate module paths.
    let suffix = module_segments.join("/");
    let matches: Vec<&&String> = candidates
        .iter()
        .filter(|c| {
            let p = c.strip_suffix(".rs").unwrap_or(c);
            let mod_path = p.strip_suffix("/mod").unwrap_or(p);
            mod_path == suffix || mod_path.ends_with(&format!("/{suffix}"))
        })
        .collect();
    if matches.len() == 1 {
        Some((**matches[0]).clone())
    } else {
        None
    }
}

/// Module root that `crate::` refers to in `source_path`: from Cargo.toml
/// when known (e.g. `crates/core` for a `[[bin]] path = "crates/core/main.rs"`),
/// else the enclosing `src/` directory.
fn rust_crate_root(source_path: &str, ctx: &ResolutionContext) -> String {
    if let Some(root) = ctx.rust_crates.module_root(source_path) {
        return root;
    }
    if let Some(idx) = source_path.find("/src/") {
        source_path[..idx + 4].to_string()
    } else {
        "src".to_string()
    }
}

fn rust_self_base_dir(source_path: &str) -> String {
    let source_dir = source_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let file_name = source_path
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(source_path);

    match file_name {
        "mod.rs" | "lib.rs" | "main.rs" => source_dir.to_string(),
        name if name.ends_with(".rs") => {
            let stem = name.trim_end_matches(".rs");
            join_repo_path(source_dir, stem)
        }
        _ => source_dir.to_string(),
    }
}

fn rust_super_base_dir(source_path: &str) -> Option<String> {
    let source_dir = source_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let file_name = source_path
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(source_path);

    match file_name {
        "mod.rs" => Some(
            source_dir
                .rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or("")
                .to_string(),
        ),
        "lib.rs" | "main.rs" => None,
        _ => Some(source_dir.to_string()),
    }
}

fn join_repo_path(base: &str, segment: &str) -> String {
    match (base.is_empty(), segment.is_empty()) {
        (true, true) => String::new(),
        (true, false) => segment.to_string(),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}/{segment}"),
    }
}

/// Word counts of every scanned `__init__.py`, keyed by its path. Used to
/// tell `from . import name` of a symbol from one of a missing module.
fn python_init_words(
    included: &[&FileEntry],
    pack_dir: &Path,
) -> HashMap<String, HashMap<String, usize>> {
    let mut out = HashMap::new();
    for e in included {
        if e.path != "__init__.py" && !e.path.ends_with("/__init__.py") {
            continue;
        }
        let Some(cp) = &e.content_path else { continue };
        let Ok(path) = resolve_existing_subpath(pack_dir, cp) else {
            continue;
        };
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut words: HashMap<String, usize> = HashMap::new();
        for w in text
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
        {
            *words.entry(w.to_string()).or_default() += 1;
        }
        out.insert(e.path.clone(), words);
    }
    out
}

/// For `from . import name` (`./name`, `../name`): the relative package
/// prefix (`./`, `../`), or `None` for any other import.
fn python_package_import(rel: &Relationship) -> Option<&str> {
    let name = rel.target_name.as_str();
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return None;
    }
    let prefix = rel.detail.strip_suffix(name)?;
    let same = prefix
        .strip_prefix("./")
        .map(|r| r.is_empty())
        .unwrap_or(false);
    let parents = !prefix.is_empty()
        && prefix.len() % 3 == 0
        && prefix.as_bytes().chunks(3).all(|c| c == b"../");
    (same || parents).then_some(prefix)
}

/// `from . import name` that matched no file is a missing module unless
/// the package's `__init__.py` mentions `name` (it is then most likely a
/// symbol defined or re-exported there). With no `__init__.py` scanned,
/// `name` can only be a module.
fn python_package_import_is_missing(
    source: &str,
    rel: &Relationship,
    init_words: &HashMap<String, HashMap<String, usize>>,
) -> bool {
    let Some(prefix) = python_package_import(rel) else {
        return false;
    };
    let dir = source.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in prefix.split('/') {
        if p == ".." && parts.pop().is_none() {
            return false;
        }
    }
    parts.push("__init__.py");
    let init = parts.join("/");
    let Some(words) = init_words.get(&init) else {
        return true;
    };
    // The `__init__.py` doing the import mentions the name once itself.
    let needed = if init == source { 2 } else { 1 };
    words.get(&rel.target_name).copied().unwrap_or(0) < needed
}

/// A Python relative import in the form it is written: `./nope_mod` ->
/// `.nope_mod`, `../core/base` -> `..core.base`.
fn python_dotted(import: &str) -> String {
    let mut rest = import;
    let mut dots = String::from(".");
    if let Some(r) = rest.strip_prefix("./") {
        rest = r;
    } else {
        dots.clear();
        while let Some(r) = rest.strip_prefix("../") {
            dots.push('.');
            rest = r;
        }
        if dots.is_empty() {
            return import.to_string();
        }
        dots.push('.');
    }
    format!("{dots}{}", rest.replace('/', "."))
}

/// Determine if an unresolved relationship target is external to the project.
fn is_external(rel: &Relationship, language: &str, ctx: &ResolutionContext) -> bool {
    if rel.rel_type == "calls" {
        // Mirror Strategy 3.5's namespace selection exactly (Codex D2): a
        // name absent from the language-selected map is external; a name
        // present but ambiguous stays unresolved.
        return calls_candidates(language, &rel.target_name, ctx).is_empty();
    }

    if language == "Python" && is_import_type(&rel.rel_type) {
        // `from a import name` emits `a.name` in case `name` is a submodule
        // (and `from . import name` emits `./name`): when no such file
        // exists, `name` is a symbol, not a missing module.
        if rel.target_name != rel.detail {
            return true;
        }
        // Absolute import whose top-level package is not in the repo
        // (stdlib or a third-party package).
        if !rel.detail.starts_with('.') {
            let top = rel.detail.split('.').next().unwrap_or("");
            return !ctx.python_local.contains(top);
        }
        return false;
    }

    // Rust: if the import detail starts with a non-workspace crate name, it's external
    if rel.detail.contains("::") {
        let first_segment = rel.detail.split("::").next().unwrap_or("");
        if !first_segment.is_empty()
            && !matches!(first_segment, "crate" | "super" | "self")
            && !ctx.rust_workspace_crates.contains_key(first_segment)
        {
            return true;
        }
    }

    false
}

/// Remove edges whose source is Rust test code: files under a `tests/`
/// directory, and files declared as `#[cfg(test)] mod x;` (and everything
/// they declare in turn). Their `declares` edges are kept so module structure
/// is still visible; cycle detection ignores `declares` anyway. Test-only
/// items inside ordinary files are already skipped by the extractor.
fn drop_rust_test_only_edges(edges: &mut Vec<GraphEdge>) {
    let is_rs = |p: &str| p.ends_with(".rs");
    let mut test_only: HashSet<String> = HashSet::new();
    for e in edges.iter() {
        for p in [&e.source, &e.target] {
            if is_rs(p) && (p.starts_with("tests/") || p.contains("/tests/")) {
                test_only.insert(p.clone());
            }
        }
        if e.edge_type == "declares"
            && e.detail.as_deref() == Some(repolens_extract::rust_rel::CFG_TEST_DETAIL)
        {
            test_only.insert(e.target.clone());
        }
    }
    // Everything a test-only file declares is test-only too.
    loop {
        let more: Vec<String> = edges
            .iter()
            .filter(|e| e.edge_type == "declares" && test_only.contains(&e.source))
            .filter(|e| !test_only.contains(&e.target))
            .map(|e| e.target.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        test_only.extend(more);
    }
    edges.retain(|e| {
        e.edge_type == "declares" || !is_rs(&e.source) || !test_only.contains(&e.source)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::{FileEntry, Symbol};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn empty_entries_produce_empty_graph() {
        let dir = tempfile::tempdir().unwrap();
        let result = GraphBuilder::new().build(&[], dir.path(), None);

        assert!(result.graph.nodes.is_empty());
        assert!(result.graph.edges.is_empty());
        assert_eq!(result.metrics.total_relationships, 0);
        assert_eq!(result.metrics.resolved_edges, 0);
        assert_eq!(result.metrics.unresolved_edges, 0);
        assert_eq!(result.metrics.per_language, Some(BTreeMap::new()));
    }

    #[test]
    fn nodes_created_for_included_entries_only() {
        let dir = tempfile::tempdir().unwrap();

        let entries = vec![
            FileEntry {
                path: "src/App.java".to_string(),
                sha256: "abc".to_string(),
                language: "Java".to_string(),
                loc: 10,
                included: true,
                reason: "included".to_string(),
                content_path: None,
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "App".to_string(),
                    qualified_name: Some("com.example.App".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
            FileEntry {
                path: "build/output.class".to_string(),
                sha256: String::new(),
                language: String::new(),
                loc: 0,
                included: false,
                reason: "excluded:pattern".to_string(),
                content_path: None,
                symbols: vec![],
            },
        ];

        let result = GraphBuilder::new().build(&entries, dir.path(), None);

        assert_eq!(result.graph.nodes.len(), 1);
        assert_eq!(result.graph.nodes[0].id, "src/App.java");
        assert_eq!(result.graph.nodes[0].symbols, vec!["App"]);
    }

    #[test]
    fn java_small_fixture_produces_correct_graph() {
        let fixture_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/java-small");

        if !fixture_root.exists() {
            eprintln!("Skipping: fixture not found at {}", fixture_root.display());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("java-small");
        copy_dir_recursive(&fixture_root, &root);

        let scan_result = crate::scan::ScanService::new()
            .run(&root, false, 100)
            .unwrap();

        let entries = read_all_chunks(&scan_result.pack_dir);

        let graph_result = GraphBuilder::new().build(&entries, &scan_result.pack_dir, Some(&root));

        let included_count = entries.iter().filter(|e| e.included).count();
        assert_eq!(
            graph_result.graph.nodes.len(),
            included_count,
            "node count should match included entries"
        );

        let edges = &graph_result.graph.edges;
        assert_eq!(edges.len(), 3, "expected 3 edges for java-small");

        let imports_edge = edges
            .iter()
            .find(|e| e.edge_type == "imports")
            .expect("should have imports edge");
        assert_eq!(imports_edge.source, "src/main/java/com/example/App.java");
        assert_eq!(imports_edge.target, "src/main/java/com/example/Helper.java");
        assert_eq!(imports_edge.detail, Some("com.example.Helper".to_string()));

        let calls_edge = edges
            .iter()
            .find(|e| e.edge_type == "calls")
            .expect("should have calls edge");
        assert_eq!(calls_edge.source, "src/main/java/com/example/App.java");
        assert_eq!(calls_edge.target, "src/main/java/com/example/Helper.java");
        assert_eq!(calls_edge.detail, Some("helper.greet".to_string()));

        let type_ref_edge = edges
            .iter()
            .find(|e| e.edge_type == "type_ref")
            .expect("should have type_ref edge");
        assert_eq!(type_ref_edge.source, "src/main/java/com/example/App.java");
        assert_eq!(
            type_ref_edge.target,
            "src/main/java/com/example/Helper.java"
        );
        assert_eq!(type_ref_edge.detail, Some("Helper".to_string()));

        assert_eq!(graph_result.metrics.total_relationships, 3);
        assert_eq!(graph_result.metrics.resolved_edges, 3);
        assert_eq!(graph_result.metrics.unresolved_edges, 0);

        let per_lang = graph_result.metrics.per_language.as_ref().unwrap();
        assert_eq!(per_lang.get("Java"), Some(&vec![3, 0]));
    }

    #[test]
    fn mixed_small_fixture_produces_correct_graph() {
        let fixture_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/mixed-small");

        if !fixture_root.exists() {
            eprintln!("Skipping: fixture not found at {}", fixture_root.display());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mixed-small");
        copy_dir_recursive(&fixture_root, &root);

        let scan_result = crate::scan::ScanService::new()
            .run(&root, false, 100)
            .unwrap();

        let entries = read_all_chunks(&scan_result.pack_dir);

        let graph_result = GraphBuilder::new().build(&entries, &scan_result.pack_dir, Some(&root));

        assert_eq!(
            graph_result.graph.edges.len(),
            1,
            "expected 1 edge for mixed-small"
        );
        let edge = &graph_result.graph.edges[0];
        assert_eq!(edge.source, "src/index.ts");
        assert_eq!(edge.target, "src/util.js");
        assert_eq!(edge.edge_type, "imports");
        assert_eq!(edge.detail, Some("./util".to_string()));

        assert_eq!(graph_result.metrics.resolved_edges, 1);
        assert_eq!(graph_result.metrics.unresolved_edges, 0);

        let per_lang = graph_result.metrics.per_language.as_ref().unwrap();
        assert_eq!(per_lang.get("TypeScript"), Some(&vec![1, 0]));
    }

    // Relationship extraction + resolution runs over `included.par_iter()` and the results
    // are `.collect()`ed and merged serially in included-entry order. rayon's ordered
    // `collect` makes the merged `edges` list and per-language metrics byte-identical
    // regardless of worker-thread count. That invariant is correct by construction today
    // but unguarded by the suite, so a future non-order-preserving refactor (par_bridge,
    // fold-into-shared-map, etc.) would silently reorder edges. This pins it: build the
    // same multi-edge input twice — once on the global pool, once pinned to one thread —
    // and require identical output. (This guards thread-count variance only; cross-run
    // HashMap-seed nondeterminism is a separate axis, handled by per_language being a
    // BTreeMap and the `per_language_serializes_with_sorted_keys` test.)
    #[test]
    fn graph_build_edges_thread_count_invariant() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("synthetic-multi");
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();

        // Many TS->JS relative-import pairs => many resolved `imports` edges from distinct
        // source files, so a merge that loses included-order would reorder `edges`.
        const PAIRS: usize = 16;
        for k in 0..PAIRS {
            let ts = format!(
                "import {{ fn{k} }} from './b{k}';\n\nexport function main{k}(): void {{\n    fn{k}('x');\n}}\n"
            );
            let js = format!("export function fn{k}(name) {{\n    return name;\n}}\n");
            fs::write(src.join(format!("a{k}.ts")), ts).unwrap();
            fs::write(src.join(format!("b{k}.js")), js).unwrap();
        }

        let scan_result = crate::scan::ScanService::new()
            .run(&root, false, 100)
            .unwrap();
        let entries = read_all_chunks(&scan_result.pack_dir);

        // Build on the global (multi-threaded) rayon pool...
        let multi = GraphBuilder::new().build(&entries, &scan_result.pack_dir, Some(&root));
        // ...and again pinned to a single worker thread.
        let single = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| GraphBuilder::new().build(&entries, &scan_result.pack_dir, Some(&root)));

        assert!(
            multi.graph.edges.len() >= 2,
            "test needs multiple edges to guard ordering, got {}",
            multi.graph.edges.len()
        );
        assert_eq!(
            multi.graph.edges, single.graph.edges,
            "edge list (including order) must be invariant to worker-thread count"
        );
        assert_eq!(
            multi.metrics.per_language, single.metrics.per_language,
            "per_language metrics must be invariant to worker-thread count"
        );
        assert_eq!(multi.metrics.resolved_edges, single.metrics.resolved_edges);
        assert_eq!(
            multi.metrics.unresolved_edges,
            single.metrics.unresolved_edges
        );
        assert_eq!(
            multi.metrics.total_relationships,
            single.metrics.total_relationships
        );
    }

    #[test]
    fn excluded_small_fixture_produces_empty_edges() {
        let fixture_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/excluded-small");

        if !fixture_root.exists() {
            eprintln!("Skipping: fixture not found at {}", fixture_root.display());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("excluded-small");
        copy_dir_recursive(&fixture_root, &root);

        let scan_result = crate::scan::ScanService::new()
            .run(&root, false, 100)
            .unwrap();

        let entries = read_all_chunks(&scan_result.pack_dir);

        let graph_result = GraphBuilder::new().build(&entries, &scan_result.pack_dir, Some(&root));

        assert!(
            graph_result.graph.edges.is_empty(),
            "expected 0 edges for excluded-small"
        );
        assert_eq!(graph_result.metrics.total_relationships, 0);
        assert_eq!(graph_result.metrics.resolved_edges, 0);
        assert_eq!(graph_result.metrics.unresolved_edges, 0);
        assert_eq!(graph_result.metrics.per_language, Some(BTreeMap::new()));
    }

    #[test]
    fn rust_mod_declaration_resolves_to_child_file() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let files_dir = pack_dir.join("files/crates/mylib/src");
        fs::create_dir_all(&files_dir).unwrap();

        // lib.rs declares two modules
        fs::write(files_dir.join("lib.rs"), "pub mod config;\npub mod util;\n").unwrap();
        fs::write(files_dir.join("config.rs"), "pub struct Settings;\n").unwrap();
        // util is a directory module
        let util_dir = files_dir.join("util");
        fs::create_dir_all(&util_dir).unwrap();
        fs::write(util_dir.join("mod.rs"), "pub fn help() {}\n").unwrap();

        let entries = vec![
            FileEntry {
                path: "crates/mylib/src/lib.rs".to_string(),
                sha256: "a".to_string(),
                language: "Rust".to_string(),
                loc: 2,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/crates/mylib/src/lib.rs".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "crates/mylib/src/config.rs".to_string(),
                sha256: "b".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/crates/mylib/src/config.rs".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "Settings".to_string(),
                    qualified_name: Some("Settings".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
            FileEntry {
                path: "crates/mylib/src/util/mod.rs".to_string(),
                sha256: "c".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/crates/mylib/src/util/mod.rs".to_string()),
                symbols: vec![],
            },
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);

        let declares_edges: Vec<_> = result
            .graph
            .edges
            .iter()
            .filter(|e| e.edge_type == "declares")
            .collect();
        assert_eq!(declares_edges.len(), 2, "edges: {declares_edges:?}");

        let config_edge = declares_edges
            .iter()
            .find(|e| e.target == "crates/mylib/src/config.rs")
            .expect("should resolve mod config to config.rs");
        assert_eq!(config_edge.source, "crates/mylib/src/lib.rs");

        let util_edge = declares_edges
            .iter()
            .find(|e| e.target == "crates/mylib/src/util/mod.rs")
            .expect("should resolve mod util to util/mod.rs");
        assert_eq!(util_edge.source, "crates/mylib/src/lib.rs");
    }

    // --- Resolver map split (spec 2026-06-12) -----------------------------

    /// Symbol shorthand for map-split tests.
    fn fn_symbol(name: &str, symbol_type: &str, qualified: &str) -> Symbol {
        Symbol {
            symbol_type: symbol_type.to_string(),
            name: name.to_string(),
            qualified_name: Some(qualified.to_string()),
            visibility: Some("public".to_string()),
            line: None,
        }
    }

    fn rust_entry(path: &str, content_path: Option<&str>, symbols: Vec<Symbol>) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            sha256: "x".to_string(),
            language: "Rust".to_string(),
            loc: 1,
            included: true,
            reason: "included".to_string(),
            content_path: content_path.map(String::from),
            symbols,
        }
    }

    #[test]
    fn rust_bare_call_to_method_only_name_is_external_not_an_edge() {
        // `helper` exists only as an impl method. A bare call cannot reach it
        // (no receiver), so the resolver must neither manufacture an edge nor
        // count it unresolved — it is external to the free-fn namespace.
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let src = pack_dir.join("files/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.rs"), "fn main() {\n    helper();\n}\n").unwrap();

        let entries = vec![
            rust_entry("src/a.rs", Some("files/src/a.rs"), vec![]),
            rust_entry(
                "src/b.rs",
                None,
                vec![fn_symbol("helper", "method", "Widget.helper")],
            ),
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);
        let calls: Vec<_> = result
            .graph
            .edges
            .iter()
            .filter(|e| e.edge_type == "calls")
            .collect();
        assert!(
            calls.is_empty(),
            "method-only name must not attract a bare-call edge: {calls:?}"
        );
        assert_eq!(
            result.metrics.unresolved_edges, 0,
            "method-only bare-call target is external, not unresolved"
        );
    }

    #[test]
    fn rust_bare_call_resolves_against_free_fn_despite_method_collision() {
        // One free fn `helper` + one impl method `helper` elsewhere: the free
        // fn is the only legal bare-call target, so the edge must resolve to
        // it instead of going ambiguous-unresolved.
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let src = pack_dir.join("files/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.rs"), "fn main() {\n    helper();\n}\n").unwrap();

        let entries = vec![
            rust_entry("src/a.rs", Some("files/src/a.rs"), vec![]),
            rust_entry(
                "src/b.rs",
                None,
                vec![fn_symbol("helper", "function", "helper")],
            ),
            rust_entry(
                "src/c.rs",
                None,
                vec![fn_symbol("helper", "method", "Widget.helper")],
            ),
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);
        let calls: Vec<_> = result
            .graph
            .edges
            .iter()
            .filter(|e| e.edge_type == "calls")
            .collect();
        assert_eq!(calls.len(), 1, "calls: {calls:?}");
        assert_eq!(calls[0].source, "src/a.rs");
        assert_eq!(
            calls[0].target, "src/b.rs",
            "must resolve to the free fn, not collide with the impl method"
        );
        assert_eq!(result.metrics.unresolved_edges, 0);
    }

    #[test]
    fn rust_scoped_call_detail_disambiguates_between_duplicate_free_fns() {
        // The gm-crypto shape: the same free-fn name in sibling modules
        // (sm2/encrypt.rs, sm4/encrypt.rs). A scoped call's detail carries
        // the module path, which picks the right file (Strategy 3.6).
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let src = pack_dir.join("files/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("a.rs"),
            "fn main() {\n    sm2::encrypt::encrypt();\n}\n",
        )
        .unwrap();

        let entries = vec![
            rust_entry("src/a.rs", Some("files/src/a.rs"), vec![]),
            rust_entry(
                "src/sm2/encrypt.rs",
                None,
                vec![fn_symbol("encrypt", "function", "encrypt")],
            ),
            rust_entry(
                "src/sm4/encrypt.rs",
                None,
                vec![fn_symbol("encrypt", "function", "encrypt")],
            ),
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);
        let calls: Vec<_> = result
            .graph
            .edges
            .iter()
            .filter(|e| e.edge_type == "calls")
            .collect();
        assert_eq!(calls.len(), 1, "calls: {calls:?}");
        assert_eq!(calls[0].source, "src/a.rs");
        assert_eq!(
            calls[0].target, "src/sm2/encrypt.rs",
            "detail module path must pick the sm2 sibling"
        );
        assert_eq!(result.metrics.unresolved_edges, 0);
    }

    #[test]
    fn strategy_36_rooted_relative_and_ambiguous_matching() {
        let function_map: HashMap<String, Vec<String>> = [
            (
                "run".to_string(),
                vec!["src/util.rs".to_string(), "src/misc.rs".to_string()],
            ),
            (
                "dup".to_string(),
                vec![
                    "src/left/foo.rs".to_string(),
                    "src/right/foo.rs".to_string(),
                ],
            ),
        ]
        .into();
        let method_map: HashMap<String, Vec<String>> = HashMap::new();
        let empty_q = QualifiedIndex::default();
        let empty_s: HashMap<String, Vec<String>> = HashMap::new();
        let empty_p: HashMap<String, String> = HashMap::new();
        let included: HashSet<String> =
            ["src/util.rs".to_string(), "src/misc.rs".to_string()].into();
        let crates: HashMap<String, String> = HashMap::new();
        let ctx = ResolutionContext {
            qualified_to_file: &empty_q,
            simple_to_files: &empty_s,
            file_to_package: &empty_p,
            function_to_files: &function_map,
            method_to_files: &method_map,
            included_paths: &included,
            import_map: HashMap::new(),
            source_package: None,
            go_module_prefix: None,
            rust_workspace_crates: &crates,
            rust_crates: &RustCrates::default(),
            python_roots: &[],
            python_local: &HashSet::new(),
        };
        let call = |detail: &str, target: &str| Relationship {
            rel_type: "calls".to_string(),
            target_name: target.to_string(),
            detail: detail.to_string(),
        };

        // crate-rooted: crate::util::run resolves via included_paths and
        // must land on the candidate it names.
        assert_eq!(
            resolve_target("src/a.rs", "Rust", &call("crate::util::run", "run"), &ctx).as_deref(),
            Some("src/util.rs"),
        );
        // relative single segment: util::run suffix-matches src/util only.
        assert_eq!(
            resolve_target("src/a.rs", "Rust", &call("util::run", "run"), &ctx).as_deref(),
            Some("src/util.rs"),
        );
        // ambiguous suffix (foo matches both candidates) stays unresolved.
        assert_eq!(
            resolve_target("src/a.rs", "Rust", &call("foo::dup", "dup"), &ctx),
            None,
        );
        // zero-match prefix stays unresolved — no fallback guessing.
        assert_eq!(
            resolve_target("src/a.rs", "Rust", &call("nowhere::run", "run"), &ctx),
            None,
        );
    }

    #[test]
    fn go_imports_outside_the_module_are_dropped() {
        let imp = |p: &str| Relationship {
            rel_type: "imports".to_string(),
            target_name: p.to_string(),
            detail: p.to_string(),
        };
        let rels = vec![
            imp("github.com/spf13/cobra"),
            imp("github.com/spf13/cobra/doc"),
            imp("github.com/spf13/cobra-cli"),
            imp("github.com/spf13/pflag"),
            imp("internal/x"),
        ];
        let kept: Vec<String> =
            drop_external_go_imports(rels.clone(), Some("github.com/spf13/cobra"))
                .into_iter()
                .map(|r| r.target_name)
                .collect();
        assert_eq!(
            kept,
            vec![
                "github.com/spf13/cobra",
                "github.com/spf13/cobra/doc",
                "internal/x"
            ]
        );
        let kept: Vec<String> = drop_external_go_imports(rels, None)
            .into_iter()
            .map(|r| r.target_name)
            .collect();
        assert_eq!(kept, vec!["internal/x"]);
    }

    #[test]
    fn non_rust_java_calls_use_union_semantics() {
        // Languages without call edges today keep status-quo (combined-map)
        // semantics: the same name split across a free fn and a method is
        // ambiguous for them, while Rust resolves it via the function map.
        let function_map: HashMap<String, Vec<String>> =
            [("helper".to_string(), vec!["src/b.rs".to_string()])].into();
        let method_map: HashMap<String, Vec<String>> =
            [("helper".to_string(), vec!["src/c.go".to_string()])].into();
        let empty_q = QualifiedIndex::default();
        let empty_s: HashMap<String, Vec<String>> = HashMap::new();
        let empty_p: HashMap<String, String> = HashMap::new();
        let included: HashSet<String> = HashSet::new();
        let crates: HashMap<String, String> = HashMap::new();
        let ctx = ResolutionContext {
            qualified_to_file: &empty_q,
            simple_to_files: &empty_s,
            file_to_package: &empty_p,
            function_to_files: &function_map,
            method_to_files: &method_map,
            included_paths: &included,
            import_map: HashMap::new(),
            source_package: None,
            go_module_prefix: None,
            rust_workspace_crates: &crates,
            rust_crates: &RustCrates::default(),
            python_roots: &[],
            python_local: &HashSet::new(),
        };
        let rel = Relationship {
            rel_type: "calls".to_string(),
            target_name: "helper".to_string(),
            detail: "helper".to_string(),
        };
        assert_eq!(
            resolve_target("src/main.go", "Go", &rel, &ctx),
            None,
            "Go sees the union (two candidates) and stays ambiguous"
        );
        assert_eq!(
            resolve_target("src/main.rs", "Rust", &rel, &ctx).as_deref(),
            Some("src/b.rs"),
            "Rust selects the function map and resolves"
        );
    }

    #[test]
    fn rust_workspace_crate_import_resolves_across_crates() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();

        // Create content files for two crates
        let alpha_dir = pack_dir.join("files/crates/alpha/src");
        let beta_dir = pack_dir.join("files/crates/beta/src");
        fs::create_dir_all(&alpha_dir).unwrap();
        fs::create_dir_all(&beta_dir).unwrap();

        // alpha/src/lib.rs imports from beta
        fs::write(
            alpha_dir.join("lib.rs"),
            "use beta::util::Helper;\n\nfn main() {}\n",
        )
        .unwrap();

        // beta/src/lib.rs and beta/src/util.rs
        fs::write(beta_dir.join("lib.rs"), "pub mod util;\n").unwrap();
        fs::write(beta_dir.join("util.rs"), "pub struct Helper;\n").unwrap();

        let entries = vec![
            FileEntry {
                path: "crates/alpha/src/lib.rs".to_string(),
                sha256: "a".to_string(),
                language: "Rust".to_string(),
                loc: 3,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/crates/alpha/src/lib.rs".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "crates/beta/src/lib.rs".to_string(),
                sha256: "b".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/crates/beta/src/lib.rs".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "crates/beta/src/util.rs".to_string(),
                sha256: "c".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/crates/beta/src/util.rs".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "Helper".to_string(),
                    qualified_name: Some("Helper".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);

        // alpha/src/lib.rs should have an imports edge to beta/src/util.rs
        let cross_edge = result
            .graph
            .edges
            .iter()
            .find(|e| e.source == "crates/alpha/src/lib.rs" && e.edge_type == "imports")
            .expect("should resolve cross-crate import");
        assert_eq!(cross_edge.target, "crates/beta/src/util.rs");

        // beta/src/lib.rs should have a declares edge to beta/src/util.rs
        let mod_edge = result
            .graph
            .edges
            .iter()
            .find(|e| e.source == "crates/beta/src/lib.rs" && e.edge_type == "declares")
            .expect("should resolve mod declaration");
        assert_eq!(mod_edge.target, "crates/beta/src/util.rs");
    }

    #[test]
    fn rust_super_import_from_leaf_module_resolves_to_parent_module_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let foo_dir = pack_dir.join("files/src/foo");
        fs::create_dir_all(&foo_dir).unwrap();
        fs::create_dir_all(pack_dir.join("files/src/other")).unwrap();

        fs::write(foo_dir.join("bar.rs"), "use super::baz::Thing;\n").unwrap();
        fs::write(foo_dir.join("baz.rs"), "pub struct Thing;\n").unwrap();
        fs::write(
            pack_dir.join("files/src/other/thing.rs"),
            "pub struct Thing;\n",
        )
        .unwrap();

        let entries = vec![
            FileEntry {
                path: "src/foo/bar.rs".to_string(),
                sha256: "a".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/foo/bar.rs".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "src/foo/baz.rs".to_string(),
                sha256: "b".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/foo/baz.rs".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "Thing".to_string(),
                    qualified_name: Some("Thing".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
            FileEntry {
                path: "src/other/thing.rs".to_string(),
                sha256: "c".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/other/thing.rs".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "Thing".to_string(),
                    qualified_name: Some("Thing".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);

        let edge = result
            .graph
            .edges
            .iter()
            .find(|e| e.source == "src/foo/bar.rs" && e.edge_type == "imports")
            .expect("super:: import should resolve");
        assert_eq!(edge.target, "src/foo/baz.rs");
    }

    #[test]
    fn rust_self_import_from_leaf_module_resolves_to_child_module() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let bar_dir = pack_dir.join("files/src/foo/bar");
        fs::create_dir_all(&bar_dir).unwrap();
        fs::create_dir_all(pack_dir.join("files/src/foo")).unwrap();

        fs::write(
            pack_dir.join("files/src/foo/bar.rs"),
            "use self::inner::Thing;\n",
        )
        .unwrap();
        fs::write(bar_dir.join("inner.rs"), "pub struct Thing;\n").unwrap();
        fs::write(
            pack_dir.join("files/src/foo/other.rs"),
            "pub struct Thing;\n",
        )
        .unwrap();

        let entries = vec![
            FileEntry {
                path: "src/foo/bar.rs".to_string(),
                sha256: "a".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/foo/bar.rs".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "src/foo/bar/inner.rs".to_string(),
                sha256: "b".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/foo/bar/inner.rs".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "Thing".to_string(),
                    qualified_name: Some("Thing".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
            FileEntry {
                path: "src/foo/other.rs".to_string(),
                sha256: "c".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/foo/other.rs".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "class".to_string(),
                    name: "Thing".to_string(),
                    qualified_name: Some("Thing".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
        ];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);

        let edge = result
            .graph
            .edges
            .iter()
            .find(|e| e.source == "src/foo/bar.rs" && e.edge_type == "imports")
            .expect("self:: import should resolve");
        assert_eq!(edge.target, "src/foo/bar/inner.rs");
    }

    #[test]
    fn rust_small_fixture_produces_correct_graph() {
        let fixture_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/rust-small");

        if !fixture_root.exists() {
            eprintln!("Skipping: fixture not found at {}", fixture_root.display());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("rust-small");
        copy_dir_recursive(&fixture_root, &root);

        let scan_result = crate::scan::ScanService::new()
            .run(&root, false, 100)
            .unwrap();

        let entries = read_all_chunks(&scan_result.pack_dir);

        let graph_result = GraphBuilder::new().build(&entries, &scan_result.pack_dir, Some(&root));

        let edges = &graph_result.graph.edges;

        // Expected edges:
        // 1. alpha/src/lib.rs --declares--> alpha/src/greet.rs
        // 2. beta/src/lib.rs --declares--> beta/src/util.rs
        // 3. gamma/src/main.rs --imports--> alpha/src/greet.rs (via workspace crate resolution)
        // 4. gamma/src/main.rs --imports--> beta/src/util.rs (via workspace crate resolution)

        let declares_edges: Vec<_> = edges.iter().filter(|e| e.edge_type == "declares").collect();
        assert_eq!(
            declares_edges.len(),
            2,
            "expected 2 declares edges: {declares_edges:?}"
        );

        let alpha_mod = edges
            .iter()
            .find(|e| {
                e.source.ends_with("alpha/src/lib.rs")
                    && e.target.ends_with("alpha/src/greet.rs")
                    && e.edge_type == "declares"
            })
            .expect("alpha lib.rs should declare greet module");
        assert!(alpha_mod.detail.as_deref() == Some("greet"));

        let beta_mod = edges
            .iter()
            .find(|e| {
                e.source.ends_with("beta/src/lib.rs")
                    && e.target.ends_with("beta/src/util.rs")
                    && e.edge_type == "declares"
            })
            .expect("beta lib.rs should declare util module");
        assert!(beta_mod.detail.as_deref() == Some("util"));

        let gamma_alpha = edges
            .iter()
            .find(|e| {
                e.source.ends_with("gamma/src/main.rs") && e.target.ends_with("alpha/src/greet.rs")
            })
            .expect("gamma main.rs should import from alpha::greet");
        assert_eq!(gamma_alpha.edge_type, "imports");

        let gamma_beta = edges
            .iter()
            .find(|e| {
                e.source.ends_with("gamma/src/main.rs") && e.target.ends_with("beta/src/util.rs")
            })
            .expect("gamma main.rs should import from beta::util");
        assert_eq!(gamma_beta.edge_type, "imports");

        // The fifth edge: gamma's `greet::hello()` is a scoped call, resolved
        // via the unique function name to alpha's greet.rs (Rust call edges
        // v1, spec 2026-06-11). `h.assist()` is a receiver method call —
        // deferred, so beta::util gains no calls edge.
        let gamma_calls_greet = edges
            .iter()
            .find(|e| {
                e.source.ends_with("gamma/src/main.rs")
                    && e.target.ends_with("alpha/src/greet.rs")
                    && e.edge_type == "calls"
            })
            .expect("gamma main.rs should have a calls edge to alpha greet.rs");
        assert_eq!(gamma_calls_greet.detail.as_deref(), Some("greet::hello"));

        assert_eq!(graph_result.metrics.unresolved_edges, 0);

        let per_lang = graph_result.metrics.per_language.as_ref().unwrap();
        let rust_stats = per_lang.get("Rust").expect("should have Rust stats");
        assert_eq!(rust_stats[0], 5, "should have 5 resolved Rust edges");
        assert_eq!(rust_stats[1], 0, "should have 0 unresolved Rust edges");
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let target = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir_recursive(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    fn read_all_chunks(pack_dir: &Path) -> Vec<FileEntry> {
        let mut entries = Vec::new();
        let mut chunk_files: Vec<_> = fs::read_dir(pack_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("files-"))
            .collect();
        chunk_files.sort_by_key(|e| e.file_name());

        for chunk in chunk_files {
            let content = fs::read_to_string(chunk.path()).unwrap();
            let chunk_entries: Vec<FileEntry> = serde_json::from_str(&content).unwrap();
            entries.extend(chunk_entries);
        }
        entries
    }

    #[test]
    fn type_alias_symbol_resolves_in_graph() {
        let dir = tempfile::tempdir().unwrap();

        let app_path = dir.path().join("src");
        fs::create_dir_all(&app_path).unwrap();
        fs::write(
            app_path.join("app.ts"),
            "import { Config } from './types';\nexport function init(c: Config): void {}",
        )
        .unwrap();
        fs::write(
            app_path.join("types.ts"),
            "export type Config = { host: string }",
        )
        .unwrap();

        let entries = vec![
            FileEntry {
                path: "src/app.ts".to_string(),
                sha256: "aaa".to_string(),
                language: "TypeScript".to_string(),
                loc: 5,
                included: true,
                reason: "included".to_string(),
                content_path: Some("src/app.ts".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "src/types.ts".to_string(),
                sha256: "bbb".to_string(),
                language: "TypeScript".to_string(),
                loc: 5,
                included: true,
                reason: "included".to_string(),
                content_path: Some("src/types.ts".to_string()),
                symbols: vec![Symbol {
                    symbol_type: "type".to_string(),
                    name: "Config".to_string(),
                    qualified_name: Some("Config".to_string()),
                    visibility: Some("public".to_string()),
                    line: None,
                }],
            },
        ];

        let result = GraphBuilder::new().build(&entries, dir.path(), None);

        let app_edges: Vec<_> = result
            .graph
            .edges
            .iter()
            .filter(|e| e.source == "src/app.ts")
            .collect();
        assert!(
            app_edges.iter().any(|e| e.target == "src/types.ts"),
            "type alias Config should resolve app.ts -> types.ts, edges: {app_edges:?}"
        );
    }

    #[test]
    fn rust_external_crate_import_is_classified_as_external() {
        // A Rust file importing from a non-workspace crate (`serde::Deserialize`)
        // should be classified as external: neither resolved nor unresolved.
        // Regression guard for the `is_external` `detail.contains("::")` gate,
        // which must still recognize multi-segment Rust imports as external.
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let source_dir = pack_dir.join("files/src");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("lib.rs"),
            "use serde::Deserialize;\n\n#[derive(Deserialize)]\npub struct Config { pub name: String }\n",
        )
        .unwrap();

        let entries = vec![FileEntry {
            path: "src/lib.rs".to_string(),
            sha256: "abc".to_string(),
            language: "Rust".to_string(),
            loc: 4,
            included: true,
            reason: "included".to_string(),
            content_path: Some("files/src/lib.rs".to_string()),
            symbols: vec![],
        }];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);

        assert_eq!(result.metrics.resolved_edges, 0);
        assert_eq!(
            result.metrics.unresolved_edges, 0,
            "Rust external-crate imports should be classified as external, not unresolved"
        );
    }

    #[test]
    fn unresolved_java_import_is_not_counted_as_external() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path();
        let source_dir = pack_dir.join("files/src/main/java/com/example");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("App.java"),
            "package com.example;\n\nimport com.external.Missing;\n\npublic class App {\n    Missing field;\n}\n",
        )
        .unwrap();

        let entries = vec![FileEntry {
            path: "src/main/java/com/example/App.java".to_string(),
            sha256: "abc".to_string(),
            language: "Java".to_string(),
            loc: 6,
            included: true,
            reason: "included".to_string(),
            content_path: Some("files/src/main/java/com/example/App.java".to_string()),
            symbols: vec![Symbol {
                symbol_type: "class".to_string(),
                name: "App".to_string(),
                qualified_name: Some("com.example.App".to_string()),
                visibility: Some("public".to_string()),
                line: None,
            }],
        }];

        let result = GraphBuilder::new().build(&entries, pack_dir, None);

        assert_eq!(result.metrics.resolved_edges, 0);
        assert_eq!(
            result.metrics.unresolved_edges, 1,
            "missing non-Rust imports should stay visible in unresolved metrics"
        );
        let per_lang = result.metrics.per_language.as_ref().unwrap();
        assert_eq!(per_lang.get("Java"), Some(&vec![0, 1]));
    }

    /// #42 item 3: Python relative imports in the form they're written.
    #[test]
    fn python_relative_imports_are_dotted() {
        assert_eq!(python_dotted("./nope_mod"), ".nope_mod");
        assert_eq!(python_dotted("./a/b"), ".a.b");
        assert_eq!(python_dotted("../core/base"), "..core.base");
        assert_eq!(python_dotted("../../x"), "...x");
        assert_eq!(python_dotted("lib/x"), "lib/x");
    }
}
