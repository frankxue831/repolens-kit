use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use repolens_app::discover::PackResolver;
use repolens_app::evidence_findings::EvidenceFindingsBuilder;
use repolens_app::graph_analysis::GraphAnalyzer;
use repolens_app::summary::read_graph;
use repolens_core::model::graph::{FileGraph, GraphResolutionMetrics, UnresolvedImports};
use repolens_core::path_classification::is_non_orphan_source;
use repolens_profile::profile::ProfileAnalyzer;
use repolens_profile::repo_context::RepoContextDetector;
use rmcp::{model::CallToolResult, ErrorData as McpError};

use crate::epistemic::*;
use crate::err_json;
use crate::triage;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetGraphParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[schemars(
        description = "File path to extract neighbor subgraph for (optional — omit for full graph)"
    )]
    pub file: Option<String>,
    #[schemars(description = "BFS depth for neighbor extraction (default 1)")]
    pub depth: Option<u32>,
}

pub fn handle_get_graph(
    resolver: &Arc<PackResolver>,
    params: GetGraphParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(
        pack = ?params.pack,
        root = ?params.root,
        file = ?params.file,
        depth = ?params.depth,
        "get_graph: entry"
    );

    let (pack_dir, _index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref())
    {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_graph: resolve error");
            return err_json(&e.to_string());
        }
    };

    // Read graph.json
    let graph_path = pack_dir.join("graph.json");
    let graph: FileGraph = if graph_path.exists() {
        match std::fs::read_to_string(&graph_path)
            .ok()
            .and_then(|s| serde_json::from_str::<FileGraph>(&s).ok())
        {
            Some(g) => g,
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), "get_graph: graph.json missing or invalid");
                return err_json("graph.json is missing or invalid");
            }
        }
    } else {
        info!(elapsed_ms = %start.elapsed().as_millis(), "get_graph: graph.json not found");
        return err_json("graph.json not found in pack");
    };

    let response = if let Some(ref file) = params.file {
        // BFS subgraph extraction
        let depth = params.depth.unwrap_or(1).max(1) as usize;
        info!(file = %file, depth = depth, "get_graph: extracting neighbor subgraph");

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();

        visited.insert(file.clone());
        queue.push_back((file.clone(), 0));

        while let Some((node_id, current_depth)) = queue.pop_front() {
            if current_depth >= depth {
                continue;
            }
            // Follow edges in both directions
            for edge in &graph.edges {
                if edge.source == node_id && !visited.contains(&edge.target) {
                    visited.insert(edge.target.clone());
                    queue.push_back((edge.target.clone(), current_depth + 1));
                }
                if edge.target == node_id && !visited.contains(&edge.source) {
                    visited.insert(edge.source.clone());
                    queue.push_back((edge.source.clone(), current_depth + 1));
                }
            }
        }

        // Filter nodes and edges
        let nodes: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| visited.contains(&n.id))
            .collect();
        let edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| visited.contains(&e.source) && visited.contains(&e.target))
            .collect();

        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            node_count = nodes.len(),
            edge_count = edges.len(),
            "get_graph: subgraph exit"
        );
        serde_json::json!({ "nodes": nodes, "edges": edges })
    } else {
        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            node_count = graph.nodes.len(),
            edge_count = graph.edges.len(),
            "get_graph: full graph exit"
        );
        serde_json::json!({ "nodes": graph.nodes, "edges": graph.edges })
    };

    let pid = pack_id_from_dir(&pack_dir);
    // evidenceGrade: Derived — the no-`file` path returns graph.json verbatim
    // (verified by the import-edges and declares/cycles/orphans catalogues),
    // but the file-scoped path runs a bidirectional BFS neighbour walk whose
    // output has no invariant coverage. One handler, two modes: take the
    // lowest grade per the envelope spec's mixed-response rule.
    ok_epistemic_graded(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        EvidenceGrade::Derived,
        ProvenanceField::Single(Provenance::PackExtraction {
            pack_id: pid,
            source: "graph.json".into(),
        }),
        response,
    )
}

// ---------------------------------------------------------------------------
// get_callers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetCallersParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[schemars(description = "Repo-relative file path to find callers for")]
    pub file: String,
    #[schemars(description = "Maximum number of results to return (default 50, max 200)")]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CallerEntry {
    source: String,
    edge_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub fn handle_get_callers(
    resolver: &Arc<PackResolver>,
    params: GetCallersParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(pack = ?params.pack, root = ?params.root, file = %params.file, "get_callers: entry");

    let (pack_dir, _index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref())
    {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_callers: resolve error");
            return err_json(&e.to_string());
        }
    };

    let graph = read_graph(&pack_dir).unwrap_or_else(|| FileGraph {
        nodes: vec![],
        edges: vec![],
    });

    let mut callers: Vec<CallerEntry> = graph
        .edges
        .iter()
        .filter(|e| e.target == params.file)
        .map(|e| CallerEntry {
            source: e.source.clone(),
            edge_type: e.edge_type.clone(),
            detail: e.detail.clone(),
        })
        .collect();

    callers.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.edge_type.cmp(&b.edge_type))
            .then_with(|| a.detail.cmp(&b.detail))
    });

    // Truncate AFTER the sort: determinism here comes from the in-handler sort
    // above (find_dead_code's mid-pipeline `.take` is only order-safe because
    // find_orphans pre-sorts its output).
    let total = callers.len();
    let limit = params.limit.unwrap_or(50).min(200);
    callers.truncate(limit);
    let returned = callers.len();
    let truncated = returned < total;

    // Edge-type composition of the RETURNED (post-truncation) entries, so the
    // breakdown always sums to `returned` and an agent can see at a glance
    // whether it got call edges or only imports/type references.
    let mut edge_type_breakdown: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for c in &callers {
        *edge_type_breakdown.entry(c.edge_type.as_str()).or_insert(0) += 1;
    }
    info!(elapsed_ms = %start.elapsed().as_millis(), total_callers = total, returned, "get_callers: exit");

    let pid = pack_id_from_dir(&pack_dir);
    // #31: an import that couldn't be resolved may point at this file, so
    // the answer names it and is not graded verified.
    let unresolved = read_unresolved_imports(&pack_dir)
        .and_then(|u| relevant_to(u, &[params.file.as_str()], &graph, &pack_dir));
    let grade = if unresolved.is_some() {
        EvidenceGrade::Derived
    } else {
        EvidenceGrade::Verified
    };
    // evidenceGrade: Verified (Derived when an unresolved import could be
    // a caller of this file, see `relevant_to`) — callers are a structural reverse-edge filter
    // over graph.json. Backed by graph import-edges catalogue (5 invariants)
    // and declares/cycles/orphans catalogue (10). Bounding is a count-cap on
    // a sorted single-hop projection — no derivation introduced, totals stay
    // exact — so the grade survives the "verified inputs + uncaught
    // derivation = derived" rule. Call-semantics coverage is disclosed
    // in-payload (callEdgeCoverage/callEdgeCoverageDetail): "calls"-typed
    // edges exist for Java (method invocations) and Rust (bare + scoped
    // calls, v1 — guarded by calls_edge_coverage_test in repolens-extract),
    // so the Verified claim is "verified reverse edges", not call sites.
    ok_epistemic_graded(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        grade,
        ProvenanceField::Single(Provenance::PackExtraction {
            pack_id: pid,
            source: "graph.json".into(),
        }),
        with_unresolved(
            serde_json::json!({
                "file": params.file,
                "callers": callers,
                "totalCallers": total,
                "returned": returned,
                "truncated": truncated,
                "edgeTypeBreakdown": edge_type_breakdown,
                "callEdgeCoverage": "java-and-rust",
                "callEdgeCoverageDetail": {
                    "java": "method-invocations",
                    "rust": "bare-and-scoped-function-calls; associated-functions-target-type; receiver-method-calls-excluded",
                },
            }),
            unresolved.as_ref(),
            None,
        ),
    )
}

// ---------------------------------------------------------------------------
// get_dependencies
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDependenciesParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[schemars(description = "Repo-relative file path to find dependencies for")]
    pub file: String,
    #[schemars(description = "Maximum number of results to return (default 50, max 200)")]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DependencyEntry {
    target: String,
    edge_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub fn handle_get_dependencies(
    resolver: &Arc<PackResolver>,
    params: GetDependenciesParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(pack = ?params.pack, root = ?params.root, file = %params.file, "get_dependencies: entry");

    let (pack_dir, _index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref())
    {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_dependencies: resolve error");
            return err_json(&e.to_string());
        }
    };

    let graph = read_graph(&pack_dir).unwrap_or_else(|| FileGraph {
        nodes: vec![],
        edges: vec![],
    });

    let mut deps: Vec<DependencyEntry> = graph
        .edges
        .iter()
        .filter(|e| e.source == params.file)
        .map(|e| DependencyEntry {
            target: e.target.clone(),
            edge_type: e.edge_type.clone(),
            detail: e.detail.clone(),
        })
        .collect();

    deps.sort_by(|a, b| {
        a.target
            .cmp(&b.target)
            .then_with(|| a.edge_type.cmp(&b.edge_type))
            .then_with(|| a.detail.cmp(&b.detail))
    });

    // Truncate AFTER the sort — same idiom and rationale as get_callers.
    let total = deps.len();
    let limit = params.limit.unwrap_or(50).min(200);
    deps.truncate(limit);
    let returned = deps.len();
    let truncated = returned < total;
    info!(elapsed_ms = %start.elapsed().as_millis(), total_deps = total, returned, "get_dependencies: exit");

    let pid = pack_id_from_dir(&pack_dir);
    // #31: this file's own imports that matched no file are named too.
    let unresolved = read_unresolved_imports(&pack_dir).and_then(|u| u.only_from(&params.file));
    let grade = if unresolved.is_some() {
        EvidenceGrade::Derived
    } else {
        EvidenceGrade::Verified
    };
    // evidenceGrade: Verified (Derived when this file has unresolved
    // imports) — dependencies are a structural forward-edge
    // filter over graph.json. Backed by graph import-edges catalogue (5
    // invariants) and declares/cycles/orphans catalogue (10). Bounding is a
    // count-cap on a sorted single-hop projection — no derivation introduced,
    // totals stay exact — so the grade survives the "verified inputs +
    // uncaught derivation = derived" rule.
    ok_epistemic_graded(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        grade,
        ProvenanceField::Single(Provenance::PackExtraction {
            pack_id: pid,
            source: "graph.json".into(),
        }),
        with_unresolved(
            serde_json::json!({
                "file": params.file,
                "dependencies": deps,
                "totalDependencies": total,
                "returned": returned,
                "truncated": truncated,
            }),
            unresolved.as_ref(),
            None,
        ),
    )
}

// ---------------------------------------------------------------------------
// get_impact
// ---------------------------------------------------------------------------

const DEFAULT_MAX_DEPTH: usize = 10;
const HARD_CAP_DEPTH: usize = 50;

/// Edge types the impact BFS does not traverse. Disclosed verbatim in the
/// payload as `excludedEdgeTypes`. `calls` stays in: a caller IS behaviorally
/// impacted by its callee changing — that's the blast-radius question.
const IMPACT_EXCLUDED_EDGE_TYPES: &[&str] = &["declares"];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetImpactParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[serde(default)]
    #[schemars(description = "Repo-relative paths of changed files (seed set)")]
    pub files: Vec<String>,
    #[schemars(
        description = "Repo-relative path of a single changed file — alias for `files` matching get_callers/get_dependencies. Provide `file` or `files` (or both; they are unioned)."
    )]
    pub file: Option<String>,
    #[serde(rename = "maxDepth")]
    #[schemars(description = "Max traversal depth (default 10, hard cap 50)")]
    pub max_depth: Option<usize>,
    #[schemars(description = "Maximum number of results to return (default 50, max 200)")]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImpactEntry {
    file: String,
    depth: usize,
    path: Vec<String>,
}

pub fn handle_get_impact(
    resolver: &Arc<PackResolver>,
    params: GetImpactParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(pack = ?params.pack, root = ?params.root, files = ?params.files, file = ?params.file, "get_impact: entry");

    let (pack_dir, _index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref())
    {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_impact: resolve error");
            return err_json(&e.to_string());
        }
    };

    let max_depth = params
        .max_depth
        .filter(|&d| d > 0)
        .unwrap_or(DEFAULT_MAX_DEPTH)
        .min(HARD_CAP_DEPTH);

    let graph = read_graph(&pack_dir).unwrap_or_else(|| FileGraph {
        nodes: vec![],
        edges: vec![],
    });

    // Build incoming adjacency map: target → [edges where target == key]
    // "incoming" means: for each file, which edges point TO it (i.e., which files depend on it)
    // We traverse these in reverse: if A imports B, and B changes, then A is impacted.
    // So incoming[B] = [edge(A→B)], and we follow edge.source (A) as the impacted file.
    //
    // `declares` edges are excluded: they are module-tree containment
    // (`mod foo;`), not a dependency — the same rationale that keeps them
    // out of DEPENDENCY_EDGE_TYPES for cycle detection. Traversing them
    // admits the module container and then every sibling it declares into
    // the blast radius (dogfood round 2, friction finding 4).
    let mut incoming: HashMap<&str, Vec<&repolens_core::model::graph::GraphEdge>> = HashMap::new();
    for edge in &graph.edges {
        if IMPACT_EXCLUDED_EDGE_TYPES.contains(&edge.edge_type.as_str()) {
            continue;
        }
        incoming.entry(edge.target.as_str()).or_default().push(edge);
    }

    // Deduplicated, sorted seed set — union of the plural `files` param and
    // the singular `file` alias (sibling tools take `file`; agents reuse it).
    let mut seed_set: Vec<String> = params.files.clone();
    if let Some(f) = &params.file {
        seed_set.push(f.clone());
    }
    if seed_set.is_empty() {
        return err_json(
            "No seed files given. Provide `file` (a single repo-relative path) \
             or `files` (an array of repo-relative paths).",
        );
    }
    seed_set.sort();
    seed_set.dedup();

    // Multi-source BFS over reverse edges (dependents of changed files)
    let mut visited: HashSet<String> = seed_set.iter().cloned().collect();
    let mut queue: VecDeque<(String, usize, Vec<String>)> = VecDeque::new();
    let mut max_depth_reached = false;

    for seed in &seed_set {
        queue.push_back((seed.clone(), 0, vec![seed.clone()]));
    }

    let mut results: Vec<ImpactEntry> = Vec::new();

    while let Some((node, depth, path)) = queue.pop_front() {
        if depth >= max_depth {
            if let Some(edges) = incoming.get(node.as_str()) {
                if edges.iter().any(|e| !visited.contains(&e.source)) {
                    max_depth_reached = true;
                }
            }
            continue;
        }

        if let Some(edges) = incoming.get(node.as_str()) {
            for edge in edges {
                if !visited.contains(&edge.source) {
                    visited.insert(edge.source.clone());
                    let mut new_path = path.clone();
                    new_path.push(edge.source.clone());
                    results.push(ImpactEntry {
                        file: edge.source.clone(),
                        depth: depth + 1,
                        path: new_path.clone(),
                    });
                    queue.push_back((edge.source.clone(), depth + 1, new_path));
                }
            }
        }
    }

    results.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.file.cmp(&b.file)));

    // Truncate AFTER the (depth, file) sort so the cap keeps the shallowest
    // entries — the blast radius nearest the seed. `truncated` is the COUNT
    // cap signal; `maxDepthReached` remains the separate DEPTH cap signal.
    let total = results.len();
    let limit = params.limit.unwrap_or(50).min(200);
    results.truncate(limit);
    let returned = results.len();
    let truncated = returned < total;
    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        total_impacted = total,
        returned,
        max_depth_reached = max_depth_reached,
        "get_impact: exit"
    );

    let pid = pack_id_from_dir(&pack_dir);
    let touched: Vec<&str> = seed_set
        .iter()
        .map(String::as_str)
        .chain(results.iter().map(|r| r.file.as_str()))
        .collect();
    let unresolved = read_unresolved_imports(&pack_dir)
        .and_then(|u| relevant_to(u, &touched, &graph, &pack_dir));
    let tests_note = rust_tests_not_covered(&graph, &seed_set, &results);
    // evidenceGrade: Derived — inputs are verified (graph.json edges covered
    // by the import-edges and declares/cycles/orphans catalogues), but the
    // reverse-BFS blast-radius traversal is itself a graph-derived
    // computation with no invariant coverage on its output. Doc 06 §5
    // classifies impact analysis as graph-derived, not as a raw observation.
    ok_epistemic_graded(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        EvidenceGrade::Derived,
        ProvenanceField::Single(Provenance::PackExtraction {
            pack_id: pid,
            source: "graph.json".into(),
        }),
        with_unresolved(
            serde_json::json!({
                "seedFiles": seed_set,
                "impactedFiles": results,
                "totalImpacted": total,
                "returned": returned,
                "truncated": truncated,
                "maxDepthReached": max_depth_reached,
                "excludedEdgeTypes": IMPACT_EXCLUDED_EDGE_TYPES,
            }),
            unresolved.as_ref(),
            tests_note,
        ),
    )
}

// ---------------------------------------------------------------------------
// #31: say what the graph skipped
// ---------------------------------------------------------------------------

/// Unresolved imports recorded by the scan in `graph-metrics.json`.
fn read_unresolved_imports(pack_dir: &std::path::Path) -> Option<UnresolvedImports> {
    let text = std::fs::read_to_string(pack_dir.join("graph-metrics.json")).ok()?;
    serde_json::from_str::<GraphResolutionMetrics>(&text)
        .ok()?
        .unresolved_imports
}

/// The unresolved imports that could change an answer about `files`
/// (a callers or impact answer), or `None` when there are none (QA bug 3
/// on #41: the note used to cover the whole repo).
///
/// A relative import that matched no file names a path that isn't in the
/// scan, so it is never a plain edge to a scanned file. RepoLens doesn't
/// guess which file it meant (#42): a build output (`../dist/bundle.js`),
/// a wrong relative path (`../components`) or a renamed output that no
/// build command in the repo names (`./pkg/index.js`) is reported against
/// the importing file only, by `get_dependencies`, and other files'
/// answers stay verified.
///
/// The one exception is a build output whose name the repo itself ties to
/// a package: wasm-pack writes `./pkg/<name>.js` for a Rust crate, so
/// `./pkg/gm_web.js` covers the crate `gm-web`'s `src/lib.rs`, but not
/// `www/i18n.js`. A crate's names are its `[package]` name, its
/// `[lib] name`, and any `wasm-pack build --out-name` given for it in CI
/// or a package.json script. Names are compared without extension, case,
/// `-`/`_` differences and a wasm-bindgen `_bg` suffix.
fn relevant_to(
    u: UnresolvedImports,
    files: &[&str],
    graph: &FileGraph,
    pack_dir: &std::path::Path,
) -> Option<UnresolvedImports> {
    let packages = source_packages(graph, pack_dir);
    let mut keys: HashSet<String> = HashSet::new();
    for f in files {
        if let Some(names) = package_names_for(f, &packages) {
            keys.extend(names.iter().cloned());
        }
    }
    u.only_where(|i| keys.contains(&module_key(&i.import)))
}

/// `pkg/gm_web.js` -> `gm_web`, `.nope_mod` -> `nope_mod`,
/// `gm_web_bg.wasm` -> `gm_web`, `gm-web` -> `gm_web`.
fn module_key(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or(path);
    let last = last.trim_start_matches('.');
    let stem = last.split('.').next().unwrap_or(last);
    let key = stem.to_ascii_lowercase().replace('-', "_");
    key.strip_suffix("_bg").map(str::to_string).unwrap_or(key)
}

/// Package manifests read here and the file extensions they build. Only
/// Cargo.toml gives names; package.json gives the scripts that may run
/// `wasm-pack build --out-name`.
const SOURCE_MANIFESTS: &[(&str, &[&str])] = &[
    ("Cargo.toml", &["rs"]),
    ("package.json", &["js", "mjs", "cjs", "jsx", "ts", "tsx"]),
];

/// A package: its dir, its manifest, and its normalized module names.
type SourcePackage = (String, &'static str, Vec<String>);

/// Every Cargo crate in the graph whose name could be read, with the
/// names its build output can be imported by.
fn source_packages(graph: &FileGraph, pack_dir: &std::path::Path) -> Vec<SourcePackage> {
    let root = repolens_core::storage::RepoStoragePaths::repo_root_from_pack_dir(pack_dir);
    let mut out: Vec<SourcePackage> = Vec::new();
    // (command dir, command text) for every script and CI step text.
    let mut commands: Vec<(String, String)> = Vec::new();
    for n in &graph.nodes {
        let (dir, name) = n.id.rsplit_once('/').unwrap_or(("", n.id.as_str()));
        let Some((manifest, _)) = SOURCE_MANIFESTS.iter().find(|(m, _)| *m == name) else {
            continue;
        };
        let Some(text) = root.and_then(|r| std::fs::read_to_string(r.join(&n.id)).ok()) else {
            continue;
        };
        if *manifest == "package.json" {
            commands.extend(
                package_json_scripts(&text)
                    .into_iter()
                    .map(|c| (dir.to_string(), c)),
            );
        }
        // Only a Rust crate's name says what its build output is called
        // (wasm-pack). A package.json or pyproject.toml name doesn't name
        // a bundle or a module file, so it isn't matched (#42).
        let mut names: Vec<String> = Vec::new();
        if *manifest == "Cargo.toml" {
            names.extend(toml_value(&text, "[package]", "name"));
            names.extend(toml_value(&text, "[lib]", "name"));
        }
        if !names.is_empty() {
            let names = names.iter().map(|n| module_key(n)).collect();
            out.push((dir.to_string(), *manifest, names));
        }
    }
    if let Some(root) = root {
        commands.extend(
            ci_workflow_texts(root)
                .into_iter()
                .map(|t| (String::new(), t)),
        );
    }
    for (cwd, text) in &commands {
        for (crate_dir, out_name) in wasm_pack_out_names(cwd, text) {
            if let Some(pkg) = out
                .iter_mut()
                .find(|(d, m, _)| *m == "Cargo.toml" && *d == crate_dir)
            {
                pkg.2.push(module_key(&out_name));
            }
        }
    }
    out
}

/// The script commands in a package.json's `scripts`.
fn package_json_scripts(text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![];
    };
    v.get("scripts")
        .and_then(|s| s.as_object())
        .map(|o| {
            o.values()
                .filter_map(|c| c.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The text of every `.github/workflows/*.yml` file. Commands are found by
/// scanning lines, so `run: |` blocks and one-line `run:` steps both work.
fn ci_workflow_texts(root: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(".github/workflows")) else {
        return vec![];
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "yml" || e == "yaml"))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

/// `wasm-pack build` flags that take a value (the rest are switches).
const WASM_PACK_VALUE_FLAGS: &[&str] = &[
    "-t",
    "--target",
    "-d",
    "--out-dir",
    "--out-name",
    "-s",
    "--scope",
    "-m",
    "--mode",
    "--profile",
];

/// (crate dir, out name) for each `wasm-pack build ... --out-name X` in
/// `text`, run from `cwd`. The crate dir is the first plain argument
/// (default `.`), joined to `cwd` and normalized, `""` for the repo root.
fn wasm_pack_out_names(cwd: &str, text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("wasm-pack") {
            rest = &rest[at + "wasm-pack".len()..];
            let mut tokens = rest
                .split_whitespace()
                .map(|t| t.trim_matches(|c| c == '"' || c == '\''))
                .take_while(|t| !matches!(*t, "&&" | "||" | ";" | "|" | "--"));
            if tokens.next() != Some("build") {
                continue;
            }
            let (mut path, mut name) = (None, None);
            while let Some(t) = tokens.next() {
                if let Some(v) = t.strip_prefix("--out-name=") {
                    name = Some(v.to_string());
                } else if t == "--out-name" {
                    name = tokens.next().map(String::from);
                } else if WASM_PACK_VALUE_FLAGS.contains(&t) {
                    tokens.next();
                } else if t.starts_with('-') {
                } else if path.is_none() {
                    path = Some(t.to_string());
                }
            }
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                out.push((join_rel(cwd, path.as_deref().unwrap_or(".")), name));
            }
        }
    }
    out
}

/// `cwd` joined with a relative `path`, normalized; `""` is the repo root.
fn join_rel(cwd: &str, path: &str) -> String {
    let mut parts: Vec<&str> = cwd.split('/').filter(|p| !p.is_empty()).collect();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// A string `key` in a TOML `section`, read line by line.
fn toml_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let t = strip_toml_comment(line).trim();
        if t.starts_with('[') {
            inside = t == section;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                return (!v.is_empty()).then(|| v.to_string());
            }
        }
    }
    None
}

/// `line` without a trailing `# comment`; a `#` inside a quoted string
/// stays (#42 L1: `name = "baz" # c` and `[lib] # c`).
fn strip_toml_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return &line[..i],
            None => {}
        }
    }
    line
}

/// Names of the innermost package `file` is source code of: the package's
/// manifest builds the file's language (a `.js` file under a Cargo crate
/// is not part of the crate's build).
fn package_names_for<'a>(file: &str, packages: &'a [SourcePackage]) -> Option<&'a Vec<String>> {
    let ext = file.rsplit_once('.').map(|(_, e)| e)?;
    let around: Vec<&SourcePackage> = packages
        .iter()
        .filter(|(dir, _, _)| dir.is_empty() || file.starts_with(&format!("{dir}/")))
        .collect();
    let innermost = around.iter().map(|(dir, _, _)| dir.len()).max()?;
    // A dir can hold both a Cargo.toml and a package.json (#42's wasm-pack
    // script case); the one that builds this file's language wins.
    around
        .into_iter()
        .filter(|(dir, _, _)| dir.len() == innermost)
        .find(|(_, manifest, _)| {
            SOURCE_MANIFESTS
                .iter()
                .any(|(m, exts)| m == manifest && exts.contains(&ext))
        })
        .map(|(_, _, names)| names)
}

/// One plain line: "1 import couldn't be resolved: ./pkg/gm_web.js in
/// www/app.js", or "3 imports couldn't be resolved, e.g. ...".
///
/// When some weren't stored (more than the pack keeps), the line says how
/// many couldn't be checked against this answer (#42).
fn unresolved_line(u: &UnresolvedImports) -> String {
    if u.unchecked > 0 {
        return not_stored_line(u);
    }
    let Some(first) = u.sample.first() else {
        return format!("{} imports couldn't be resolved", u.count);
    };
    if u.count == 1 {
        format!(
            "1 import couldn't be resolved: {} in {}",
            first.import, first.source
        )
    } else {
        format!(
            "{} imports couldn't be resolved, e.g. {} in {}",
            u.count, first.import, first.source
        )
    }
}

/// The line for imports the pack counted but didn't store (#42 L3, N1),
/// within 80 columns. It applies to every graph answer (`get_callers`,
/// `get_dependencies`, `get_impact`), so it says "results", not "callers".
/// A plain `repolens scan`, incremental and with no file changed, rebuilds
/// the full list, so an older pack names that.
fn not_stored_line(u: &UnresolvedImports) -> String {
    if u.from_older_pack() {
        return format!(
            "This pack stores only {} unresolved imports. Run `repolens scan` to check all.",
            u.stored
        );
    }
    let (total, missing) = (grouped(u.total), grouped(u.unchecked));
    let line =
        format!("{total} unresolved imports ({missing} not stored); results may be incomplete.");
    if line.chars().count() <= 80 {
        return line;
    }
    // Only when 10,000,000 or more imports are not stored.
    format!("{total} unresolved imports; results may be incomplete.")
}

/// `5101` -> `5,101`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Add `unresolvedImports` and plain `notes` lines to a graph answer.
fn with_unresolved(
    mut data: serde_json::Value,
    unresolved: Option<&UnresolvedImports>,
    extra_note: Option<String>,
) -> serde_json::Value {
    let mut notes: Vec<String> = Vec::new();
    if let Some(u) = unresolved {
        data["unresolvedImports"] = serde_json::json!(u);
        notes.push(unresolved_line(u));
    }
    notes.extend(extra_note);
    if !notes.is_empty() {
        data["notes"] = serde_json::json!(notes);
    }
    data
}

/// Rust integration tests (`tests/*.rs` in any crate) are left out of the
/// import graph, so `get_impact` can't list them. When a seed is a Rust
/// file and the repo has such tests, one line says so, naming how many sit
/// in the seed's own crate.
fn rust_tests_not_covered(
    graph: &FileGraph,
    seeds: &[String],
    results: &[ImpactEntry],
) -> Option<String> {
    let is_test =
        |p: &str| p.ends_with(".rs") && (p.starts_with("tests/") || p.contains("/tests/"));
    if !seeds.iter().any(|s| s.ends_with(".rs")) || results.iter().any(|r| is_test(&r.file)) {
        return None;
    }
    let total = graph.nodes.iter().filter(|n| is_test(&n.id)).count();
    if total == 0 {
        return None;
    }
    let mut dirs: Vec<String> = seeds
        .iter()
        .filter(|s| s.ends_with(".rs"))
        .map(|s| match s.split_once("/src/") {
            Some((krate, _)) => format!("{krate}/tests/"),
            None => "tests/".to_string(),
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    let near: Vec<String> = dirs
        .iter()
        .filter_map(|dir| {
            let n = graph
                .nodes
                .iter()
                .filter(|n| is_test(&n.id) && n.id.starts_with(dir.as_str()))
                .count();
            (n > 0).then(|| format!("{n} in {dir}"))
        })
        .collect();
    let noun = if total == 1 { "file" } else { "files" };
    let mut line = format!(
        "Tests aren't covered: {total} Rust test {noun} in tests/ folders aren't in the import graph"
    );
    if !near.is_empty() {
        line.push_str(&format!(" ({})", near.join(", ")));
    }
    Some(line)
}

// ---------------------------------------------------------------------------
// find_cycles
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindCyclesParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[schemars(
        description = "Repo-relative file path. If given, only cycles involving this file are returned."
    )]
    pub file: Option<String>,
    #[schemars(description = "Maximum number of cycles to return (default 50, max 200)")]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CycleEntry {
    files: Vec<String>,
    edges: Vec<CycleEdgeEntry>,
    size: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CycleEdgeEntry {
    source: String,
    target: String,
    #[serde(rename = "type")]
    edge_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub fn handle_find_cycles(
    resolver: &Arc<PackResolver>,
    params: FindCyclesParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(pack = ?params.pack, root = ?params.root, file = ?params.file, "find_cycles: entry");

    let (pack_dir, _index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref())
    {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "find_cycles: resolve error");
            return err_json(&e.to_string());
        }
    };

    let graph = read_graph(&pack_dir).unwrap_or_else(|| FileGraph {
        nodes: vec![],
        edges: vec![],
    });

    let report = GraphAnalyzer::find_cycles(&graph);

    let mut cycles: Vec<CycleEntry> = report
        .cycles
        .into_iter()
        .filter(|scc| params.file.as_ref().is_none_or(|f| scc.files.contains(f)))
        .map(|scc| CycleEntry {
            size: scc.size,
            edges: scc
                .edges
                .into_iter()
                .map(|e| CycleEdgeEntry {
                    source: e.source,
                    target: e.target,
                    edge_type: e.edge_type,
                    detail: e.detail,
                })
                .collect(),
            files: scc.files,
        })
        .collect();

    // The cap drops whole CYCLES, never files/edges within one — truncating
    // inside an SCC would misrepresent the cycle. Order is deterministic
    // upstream: GraphAnalyzer::find_cycles sorts by size desc, then first
    // file, so the cap keeps the largest cycles.
    let total_cycles = cycles.len();
    let limit = params.limit.unwrap_or(50).min(200);
    cycles.truncate(limit);
    let returned = cycles.len();
    let truncated = returned < total_cycles;

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        total_cycles = total_cycles,
        returned,
        "find_cycles: exit"
    );

    let pid = pack_id_from_dir(&pack_dir);
    // evidenceGrade: Verified — CycleReport is directly covered by the
    // graph cycles family: I-GRAPH-CYCLES-EXCLUDE-DECLARES,
    // I-GRAPH-CYCLES-EXCLUDE-CALLS, I-GRAPH-CYCLES-EXCLUDE-SELF-LOOP,
    // I-GRAPH-CYCLE-REPORT-SHAPE. See
    // docs/superpowers/specs/2026-04-14-graph-declares-cycles-orphans-catalogue.md.
    ok_epistemic_graded(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        EvidenceGrade::Verified,
        ProvenanceField::Single(Provenance::PackExtraction {
            pack_id: pid,
            source: "graph.json".into(),
        }),
        serde_json::json!({
            "cycles": cycles,
            "totalCycles": total_cycles,
            "returned": returned,
            "truncated": truncated,
        }),
    )
}

// ---------------------------------------------------------------------------
// find_dead_code
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindDeadCodeParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[schemars(
        description = "Filter to a specific language (e.g. 'Java', 'Rust'). Optional — omit for all languages."
    )]
    pub language: Option<String>,
    #[schemars(
        description = "Filter to files under a directory prefix (e.g. 'src/main'). Optional."
    )]
    pub directory: Option<String>,
    #[schemars(description = "Maximum number of results to return (default 50, max 200)")]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeadCodeEntry {
    file: String,
    language: String,
    symbols: Vec<String>,
    loc: i32,
}

pub fn handle_find_dead_code(
    resolver: &Arc<PackResolver>,
    params: FindDeadCodeParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(
        pack = ?params.pack,
        root = ?params.root,
        language = ?params.language,
        directory = ?params.directory,
        "find_dead_code: entry"
    );

    let (pack_dir, index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "find_dead_code: resolve error");
            return err_json(&e.to_string());
        }
    };

    // Track whether graph.json was actually loaded. Phase 3 promotion to
    // `Derived` is gated on a real graph being read — otherwise the
    // "0 dead code files" fallback response would falsely claim catalogue
    // backing for evidence that was never observed.
    let graph_opt = read_graph(&pack_dir);
    let graph_was_loaded = graph_opt.is_some();
    let graph = graph_opt.unwrap_or_else(|| FileGraph {
        nodes: vec![],
        edges: vec![],
    });

    // Build entry point set from profile
    let profile = ProfileAnalyzer::analyze(&index, &pack_dir);
    let entry_point_files: HashSet<String> = profile
        .entry_points
        .iter()
        .map(|ep| ep.path.clone())
        .collect();

    let report = GraphAnalyzer::find_orphans(&graph, &entry_point_files);

    // Build a lookup from file path to pack entry for enrichment
    let entry_map: HashMap<&str, &repolens_core::model::file_entry::FileEntry> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| (e.path.as_str(), e))
        .collect();

    let limit = params.limit.unwrap_or(50).min(200);

    // Filter non-source paths before treating any path as a "dead code"
    // candidate. Uses the same `is_non_orphan_source` predicate as the
    // F-ORPHAN-FILES emitter — both `find_dead_code` and the health
    // finding answer the same question ("files with no inbound import
    // edges") and must use the same exclusion set. Otherwise dead-code
    // output flags Cargo integration tests / benches / examples / build
    // scripts / fixtures / insta snapshots / crate-root lib.rs files
    // that the health finding correctly skips.
    let source_orphans: Vec<&String> = report
        .orphan_files
        .iter()
        .filter(|f| !is_non_orphan_source(f))
        .collect();

    let results: Vec<DeadCodeEntry> = source_orphans
        .iter()
        .filter(|f| {
            if let Some(ref lang) = params.language {
                entry_map
                    .get(f.as_str())
                    .is_some_and(|e| e.language.eq_ignore_ascii_case(lang))
            } else {
                true
            }
        })
        .filter(|f| {
            if let Some(ref dir) = params.directory {
                f.starts_with(dir.as_str())
            } else {
                true
            }
        })
        .take(limit)
        .map(|f| {
            let (language, symbols, loc) = entry_map
                .get(f.as_str())
                .map(|e| {
                    (
                        e.language.clone(),
                        e.symbols.iter().map(|s| s.name.clone()).collect(),
                        e.loc,
                    )
                })
                .unwrap_or_else(|| (String::new(), vec![], 0));
            DeadCodeEntry {
                file: (*f).clone(),
                language,
                symbols,
                loc,
            }
        })
        .collect();

    // Counts reported on the response. `totalDeadCodeFiles` keeps its name
    // for back-compat but reflects the filtered count to avoid overclaiming.
    let total_orphans = source_orphans.len();
    let total_files = report.total_files;
    let returned = results.len();
    let truncated = returned < total_orphans;

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        total_orphans = total_orphans,
        returned = returned,
        "find_dead_code: exit"
    );

    let pid = pack_id_from_dir(&pack_dir);

    // Evidence-aware findings — same orphan finding the health/summary
    // tools emit, with the root-config filter already applied. Only
    // included entries drive repo-context detection.
    let file_paths: Vec<String> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.path.clone())
        .collect();
    // Resolve the repo root: prefer explicit params.root, fall back to
    // deriving from pack dir so pack-only calls still pick up
    // manifest-derived indicator tags and the RustWorkspace tag.
    let derived_root = params
        .root
        .as_deref()
        .map(std::path::Path::new)
        .or_else(|| repolens_core::storage::RepoStoragePaths::repo_root_from_pack_dir(&pack_dir));
    let manifest_deps: Vec<String> = derived_root
        .map(|root| {
            repolens_profile::repo_context::extract_cargo_dependency_names_recursive(
                root,
                &file_paths,
            )
        })
        .unwrap_or_default();
    let is_cargo_workspace = derived_root
        .map(repolens_profile::repo_context::cargo_root_is_workspace)
        .unwrap_or(false);
    let repo_context =
        RepoContextDetector::detect_full(&profile, &file_paths, &manifest_deps, is_cargo_workspace);
    let graph_health = repolens_app::graph_health::build_graph_health_input_in_pack(
        &index,
        &profile,
        &graph,
        Some(&pack_dir),
    );
    let scan_summary = repolens_app::summary::compute_scan_summary_in_pack(&index, Some(&pack_dir));
    let referenced = repolens_app::references::referenced_files(&index, &pack_dir);
    let evidence_findings = EvidenceFindingsBuilder {
        pack_id: &pid,
        commit: index.manifest.head_commit.as_deref(),
        profile: &profile,
        repo_context: &repo_context,
        graph: Some(&graph),
        graph_health: Some(&graph_health),
        scan: Some(&scan_summary),
        lockfile_state: None,
        cargo_has_binary: None,
        referenced_files: Some(&referenced),
        dependency_risk: None,
    }
    .build()
    .into_iter()
    .filter(|f| f.id.as_str() == repolens_app::evidence_findings::F_ORPHAN_FILES)
    .collect::<Vec<_>>();

    let mut data = serde_json::json!({
        "deadCodeFiles": results,
        "totalDeadCodeFiles": total_orphans,
        "totalFiles": total_files,
        "returned": returned,
        "truncated": truncated,
        "percentDead": if total_files > 0 {
            ((total_orphans as f64 / total_files as f64) * 1000.0).round() / 10.0
        } else {
            0.0
        },
        "evidenceFindings": evidence_findings,
    });

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    data = triage::augment_data(
        data,
        &pid,
        &generated_at,
        &repo_context.tags,
        &["deadCodeFiles", "totalDeadCodeFiles", "percentDead"],
    );

    // Phase 3 promotion (see 2026-05-11-evidence-aware-findings.md §"Phase 3
    // amendments"): every metric in the substantive payload is backed by a
    // doc-06 §5 catalogue (orphan detection + included file presence), and
    // the legacy `deadCodeFiles`/`totalDeadCodeFiles`/`percentDead` keys are
    // direct derivations of those catalogued families. `derived` rather than
    // `verified` because the orphan-ratio threshold is a heuristic over the
    // catalogued inputs.
    //
    // The fallback path (missing or unreadable `graph.json`) does not get
    // the `Derived` grade — without a real graph there is no catalogued
    // observation to derive from, so the response keeps the default
    // `Unaudited` envelope grade even though the legacy fields still
    // emit their zero values.
    let grade = if graph_was_loaded {
        EvidenceGrade::Derived
    } else {
        EvidenceGrade::Unaudited
    };
    ok_epistemic_graded(
        KnowledgeLayerField::Multiple(vec![KnowledgeLayer::Observation, KnowledgeLayer::Heuristic]),
        EpistemicConfidence::Codified,
        grade,
        ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: pid.clone(),
                source: "graph.json".into(),
            },
            Provenance::HeuristicRule {
                rule: "deadCodeDetection".into(),
                threshold: "multiple".into(),
                pack_id: Some(pid),
            },
        ]),
        data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // GetImpactParams wire shape: the singular `file` alias exists because
    // sibling tools (get_callers, get_dependencies) take `file`, and with
    // deny_unknown_fields an agent reusing that spelling gets a hard error
    // instead of a working call (dogfood round 2, friction finding 3).

    #[test]
    fn get_impact_params_accept_singular_file_alias() {
        let params: GetImpactParams =
            serde_json::from_value(serde_json::json!({ "file": "src/a.rs" }))
                .expect("singular `file` alias must deserialize");
        assert_eq!(params.file.as_deref(), Some("src/a.rs"));
        assert!(params.files.is_empty(), "files must default to empty");
    }

    #[test]
    fn get_impact_params_accept_plural_files_without_file() {
        let params: GetImpactParams =
            serde_json::from_value(serde_json::json!({ "files": ["src/a.rs", "src/b.rs"] }))
                .unwrap();
        assert_eq!(params.files, vec!["src/a.rs", "src/b.rs"]);
        assert!(params.file.is_none());
    }

    #[test]
    fn get_impact_params_still_reject_unknown_fields() {
        let result = serde_json::from_value::<GetImpactParams>(serde_json::json!({
            "files": ["src/a.rs"],
            "fil": "typo"
        }));
        assert!(result.is_err(), "deny_unknown_fields must stay in force");
    }

    /// #42: `wasm-pack build` forms read for `--out-name`.
    #[test]
    fn wasm_pack_out_names_reads_the_crate_dir_and_name() {
        let got = |cwd: &str, t: &str| wasm_pack_out_names(cwd, t);
        assert_eq!(
            got("", "wasm-pack build --target web --out-name index"),
            vec![(String::new(), "index".to_string())]
        );
        assert_eq!(
            got(
                "www",
                "npx wasm-pack build ../crates/web -d pkg --out-name=bundle --release"
            ),
            vec![("crates/web".to_string(), "bundle".to_string())]
        );
        // Several on one line, and one without --out-name.
        assert_eq!(
            got(
                "",
                "wasm-pack build a --out-name x && wasm-pack build b && wasm-pack build c --out-name y"
            ),
            vec![
                ("a".to_string(), "x".to_string()),
                ("c".to_string(), "y".to_string())
            ]
        );
        // Cargo args after `--` aren't the crate dir; other tools are ignored.
        assert!(got("", "wasm-pack test --out-name x").is_empty());
        assert_eq!(
            got("", "wasm-pack build --out-name x -- --features f"),
            vec![(String::new(), "x".to_string())]
        );
    }

    fn not_stored(total: usize, stored: usize, matched: usize) -> UnresolvedImports {
        use repolens_core::model::graph::UnresolvedImport;
        UnresolvedImports {
            count: matched + (total - stored),
            sample: (0..matched)
                .map(|i| UnresolvedImport {
                    source: "www/app.js".into(),
                    import: format!("./pkg/m{i}.js"),
                })
                .collect(),
            unchecked: total - stored,
            total,
            stored,
        }
    }

    /// Display columns, with `…` counted as 2 (Design).
    fn cols(s: &str) -> usize {
        s.chars().map(|c| if c == '…' { 2 } else { 1 }).sum()
    }

    /// #42 L3: Design's wording when the pack didn't store every import.
    #[test]
    fn unresolved_line_says_what_was_not_stored() {
        let line = unresolved_line(&not_stored(5101, 5000, 1));
        assert_eq!(
            line,
            "5,101 unresolved imports (101 not stored); results may be incomplete."
        );
        assert!(cols(&line) <= 80);
        let line = unresolved_line(&not_stored(1_234_567, 5000, 0));
        assert_eq!(
            line,
            "1,234,567 unresolved imports (1,229,567 not stored); results may be incomplete."
        );
        assert!(cols(&line) <= 80);
        // Older pack: 50 stored, so a rescan fixes it.
        let line = unresolved_line(&not_stored(62, 50, 0));
        assert_eq!(
            line,
            "This pack stores only 50 unresolved imports. Run `repolens scan` to check all."
        );
        assert!(cols(&line) <= 80);
        // Huge totals still fit.
        for total in [12_345, 1_234_567, 9_999_999, 99_999_999] {
            let line = unresolved_line(&not_stored(total, 5000, 0));
            assert!(cols(&line) <= 80, "{line}");
            assert!(line.starts_with(&grouped(total)), "{line}");
            assert!(line.ends_with("; results may be incomplete."), "{line}");
            assert!(!line.contains("callers"), "{line}");
        }
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    /// #42 L1: trailing TOML comments don't hide `[lib]` or end up in a name.
    #[test]
    fn manifest_names_ignore_trailing_toml_comments() {
        let toml = "[package] # the crate\nname = \"gm-web\" # published name\n\n[lib]   # wasm\nname = 'gm_wasm' # what wasm-pack writes\n";
        assert_eq!(
            toml_value(toml, "[package]", "name").as_deref(),
            Some("gm-web")
        );
        assert_eq!(
            toml_value(toml, "[lib]", "name").as_deref(),
            Some("gm_wasm")
        );
        // A `#` inside quotes is part of the value.
        let toml = "[package]\nname = \"a#b\" # c\n";
        assert_eq!(
            toml_value(toml, "[package]", "name").as_deref(),
            Some("a#b")
        );
        assert_eq!(
            strip_toml_comment(r##"x = "q\"#" # c"##),
            r##"x = "q\"#" "##
        );
    }
}
