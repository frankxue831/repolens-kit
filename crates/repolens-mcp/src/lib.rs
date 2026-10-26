use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::io::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::info;

use repolens_app::discover::PackResolver;
use repolens_app::scan_inputs::DEFAULT_MAX_FILE_BYTES;
use repolens_app::search::SearchService;
use repolens_core::path_safety::resolve_existing_prefixed_subpath;

mod epistemic;
mod tools;
pub mod triage;

pub use epistemic::*;

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoverPacksParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFileContentParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,

    #[schemars(
        description = "Absolute path to the repository root (used to discover the latest pack)"
    )]
    pub root: Option<String>,

    #[schemars(description = "Relative file path within the pack")]
    pub path: String,

    #[serde(rename = "startLine")]
    #[schemars(description = "Start line (1-based inclusive). Defaults to 1.")]
    pub start_line: Option<usize>,

    #[serde(rename = "endLine")]
    #[schemars(description = "End line (1-based inclusive). Defaults to last line.")]
    pub end_line: Option<usize>,

    #[schemars(
        description = "Detail level: 'full' (default, complete source), 'outline' (symbols only, no source — cheapest), 'signatures' (declarations and imports only, bodies stripped)"
    )]
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchCodeParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,

    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,

    #[schemars(description = "Search query string")]
    pub query: String,

    #[schemars(description = "Search mode: 'path', 'content', or 'symbol'. Default: 'content'")]
    pub mode: Option<String>,

    #[schemars(description = "Maximum number of results (default 20, max 100)")]
    pub limit: Option<u32>,

    #[schemars(description = "Use regex matching (content mode only). Default: false")]
    pub regex: Option<bool>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn err_json(msg: &str) -> Result<CallToolResult, McpError> {
    let value = serde_json::json!({ "error": msg });
    Ok(CallToolResult::success(vec![Content::text(
        value.to_string(),
    )]))
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RepoLensMcpServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    resolver: Arc<PackResolver>,
    search: Arc<SearchService>,
}

impl std::fmt::Debug for RepoLensMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoLensMcpServer").finish()
    }
}

#[tool_router]
impl RepoLensMcpServer {
    pub fn new() -> Self {
        let resolver = PackResolver::new();
        let search = SearchService::new(PackResolver::new());
        Self {
            tool_router: Self::tool_router(),
            resolver: Arc::new(resolver),
            search: Arc::new(search),
        }
    }

    // -- discover_packs -----------------------------------------------------

    #[tool(
        name = "discover_packs",
        description = "Discover available scan packs for a repository. Returns the latest pack path and the full sorted list of available packs (each with `packId` and `path`). The `packId` form (`pack-YYYYMMDD-HHMMSS`) is accepted as a `beforePack` / `afterPack` alias on `compare_summaries` when called with `root`."
    )]
    fn discover_packs(
        &self,
        Parameters(params): Parameters<DiscoverPacksParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();
        info!(root = %params.root, "discover_packs: entry");

        let root = PathBuf::from(&params.root);
        let result = match self.resolver.discover(&root) {
            Ok(r) => r,
            Err(e) => {
                info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "discover_packs: error");
                return err_json(&e.to_string());
            }
        };

        // Build `availablePacks` entries: each carries the canonical pack-id
        // (the directory's file name) plus the absolute path. The pack-id
        // form is the recommended workflow input for `compare_summaries`
        // (rather than the absolute path) when `root` is in hand.
        let available_packs: Vec<serde_json::Value> = result
            .available_packs
            .iter()
            .map(|p| {
                let pack_id = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                serde_json::json!({
                    "packId": pack_id,
                    "path": p.to_string_lossy(),
                })
            })
            .collect();
        let latest_pack_id = result
            .latest_pack
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // `sessionId` was retired from the response shape in the 2026-05-13
        // discover_packs expansion — no MCP tool accepts it and no agent
        // workflow consumes it. The underlying session is still used
        // internally by `PackResolver::discover` to pick the active pack
        // when one is set; the field just no longer leaks into responses.
        let response = serde_json::json!({
            "latestPack": result.latest_pack.to_string_lossy(),
            "latestPackId": latest_pack_id,
            "availablePacks": available_packs,
            "_hints": {
                "nextCalls": [
                    "compare_summaries({ root, beforePack: <packId-or-path>, afterPack: <packId-or-path> })",
                    "get_summary({ root })",
                ]
            }
        });

        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            pack = %result.latest_pack.display(),
            available_count = result.available_packs.len(),
            "discover_packs: exit"
        );
        // evidenceGrade: Verified — pack-directory enumeration is a direct
        // filesystem read of `<root>/.repolens/pack-*` (the same source the
        // pack writer wrote to), with no heuristics layered over it.
        // `discover_packs` produces an observation, not an inference.
        ok_epistemic_graded(
            KnowledgeLayerField::Single(KnowledgeLayer::Observation),
            EpistemicConfidence::Mechanical,
            EvidenceGrade::Verified,
            ProvenanceField::Single(Provenance::RepositoryRead {
                source: ".repolens/".into(),
            }),
            response,
        )
    }

    // -- read_file_content --------------------------------------------------

    #[tool(
        name = "read_file_content",
        description = "Read file content from a scan pack. Supports detail levels: 'full' (default), 'outline' (symbols only, no file I/O), 'signatures' (declarations/imports only). Line range applies to full/signatures modes."
    )]
    fn read_file_content(
        &self,
        Parameters(params): Parameters<ReadFileContentParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();
        let detail = params.detail.as_deref().unwrap_or("full");
        info!(path = %params.path, detail = %detail, "read_file_content: entry");

        // Resolve pack
        let (pack_dir, index) = match self
            .resolver
            .resolve(params.pack.as_deref(), params.root.as_deref())
        {
            Ok(r) => r,
            Err(e) => {
                info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "read_file_content: resolve error");
                return err_json(&e.to_string());
            }
        };

        // Find file entry
        let entry = match index
            .entries
            .iter()
            .find(|e| e.included && e.path == params.path)
        {
            Some(e) => e,
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), "read_file_content: file not found");
                return err_json(&format!("File not found in pack: {}", params.path));
            }
        };

        // --- Outline mode: return symbols from pack index, no file I/O ---
        if detail == "outline" {
            let symbols: Vec<serde_json::Value> = entry
                .symbols
                .iter()
                .map(|s| {
                    let mut sym = serde_json::json!({
                        "type": s.symbol_type,
                        "name": s.name,
                        "qualifiedName": s.qualified_name,
                        "visibility": s.visibility,
                    });
                    // Insert only when captured: absent (not null) mirrors the
                    // pack wire shape, where None emits no key.
                    if let Some(line) = s.line {
                        sym["line"] = serde_json::json!(line);
                    }
                    sym
                })
                .collect();

            let response = serde_json::json!({
                "path": params.path,
                "language": entry.language,
                "loc": entry.loc,
                "detail": "outline",
                "symbols": symbols,
                "symbolCount": symbols.len(),
            });

            let pid = pack_id_from_dir(&pack_dir);
            info!(
                elapsed_ms = %start.elapsed().as_millis(),
                symbol_count = symbols.len(),
                "read_file_content: outline exit"
            );
            // evidenceGrade: Unaudited — raw `loc` is now catalogued, but
            // outline mode still mixes in uncatalogued raw symbol fields such
            // as `name`, `type`, `visibility`, and (when captured) `line`.
            // Mixed responses take the lowest grade present, so this cannot
            // be promoted above `unaudited` until the outline payload is
            // narrowed or those fields gain their own catalogue coverage.
            //
            // Supersedes the grade assigned in commit 071aa25, which held
            // `loc` as the sole uncatalogued field and treated the raw
            // symbol fields as covered by "exact-qualified-symbols". That
            // catalogue covers the qualified-name identity of a symbol, not
            // the `type`/`visibility` metadata this payload exposes, so the
            // original `derived` grade was too permissive. Closing the
            // `loc` gap made the remaining symbol-field gap the binding
            // constraint.
            return ok_epistemic(
                KnowledgeLayerField::Single(KnowledgeLayer::Observation),
                EpistemicConfidence::Mechanical,
                ProvenanceField::Single(Provenance::PackExtraction {
                    pack_id: pid,
                    source: "pack-index".into(),
                }),
                response,
            );
        }

        // --- Full and signatures modes need file content ---

        // Validate line range
        if let Some(s) = params.start_line {
            if s < 1 {
                info!(elapsed_ms = %start.elapsed().as_millis(), "read_file_content: invalid startLine");
                return err_json("startLine must be >= 1");
            }
        }
        if let (Some(s), Some(e)) = (params.start_line, params.end_line) {
            if e < s {
                info!(elapsed_ms = %start.elapsed().as_millis(), "read_file_content: endLine < startLine");
                return err_json("endLine must be >= startLine");
            }
        }

        // Get content path
        let content_path = match &entry.content_path {
            Some(cp) => match resolve_existing_prefixed_subpath(&pack_dir, cp, "files") {
                Ok(path) => path,
                Err(e) => {
                    info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "read_file_content: invalid content path");
                    return err_json(&format!("Invalid content path: {}", e));
                }
            },
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), "read_file_content: no content");
                return err_json(&format!("No content stored for: {}", params.path));
            }
        };

        let file_bytes = match fs::metadata(&content_path) {
            Ok(metadata) => metadata.len(),
            Err(e) => {
                info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "read_file_content: metadata error");
                return err_json(&format!("Cannot stat content file: {}", e));
            }
        };
        if file_bytes > DEFAULT_MAX_FILE_BYTES as u64 {
            info!(elapsed_ms = %start.elapsed().as_millis(), bytes = file_bytes, "read_file_content: oversized file");
            return err_json("File exceeds configured size limit");
        }

        // Read content
        let raw_content = match fs::read_to_string(&content_path) {
            Ok(c) => c,
            Err(e) => {
                info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "read_file_content: read error");
                return err_json(&format!("Failed to read file: {}", e));
            }
        };

        let lines: Vec<&str> = raw_content.lines().collect();
        let total_lines = lines.len();

        // --- Signatures mode: filter to declarations/imports only ---
        if detail == "signatures" {
            let filtered = tools::signatures::extract_signatures(&lines, &entry.language);
            let response = serde_json::json!({
                "path": params.path,
                "language": entry.language,
                "totalLines": total_lines,
                "detail": "signatures",
                "content": filtered,
            });

            let pid = pack_id_from_dir(&pack_dir);
            info!(
                elapsed_ms = %start.elapsed().as_millis(),
                total_lines = total_lines,
                "read_file_content: signatures exit"
            );
            return ok_epistemic(
                KnowledgeLayerField::Single(KnowledgeLayer::Observation),
                EpistemicConfidence::Mechanical,
                ProvenanceField::Single(Provenance::PackExtraction {
                    pack_id: pid,
                    source: "content".into(),
                }),
                response,
            );
        }

        // --- Full mode (default) ---

        // Apply line range (1-based inclusive, clamped to EOF)
        let start_line = params.start_line.unwrap_or(1).max(1);
        let end_line = params.end_line.unwrap_or(total_lines).min(total_lines);

        let selected: String = if start_line <= total_lines {
            lines[(start_line - 1)..end_line].join("\n")
        } else {
            String::new()
        };

        let response = serde_json::json!({
            "path": params.path,
            "language": entry.language,
            "totalLines": total_lines,
            "detail": "full",
            "content": selected,
        });

        let pid = pack_id_from_dir(&pack_dir);
        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            total_lines = total_lines,
            "read_file_content: exit"
        );
        ok_epistemic(
            KnowledgeLayerField::Single(KnowledgeLayer::Observation),
            EpistemicConfidence::Mechanical,
            ProvenanceField::Single(Provenance::PackExtraction {
                pack_id: pid,
                source: "content".into(),
            }),
            response,
        )
    }

    // -- search_code --------------------------------------------------------

    #[tool(
        name = "search_code",
        description = "Search pack entries by path, content (with optional regex), or symbol name."
    )]
    fn search_code(
        &self,
        Parameters(params): Parameters<SearchCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();
        let mode = params.mode.as_deref().unwrap_or("content").to_lowercase();
        let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
        let regex = params.regex.unwrap_or(false);

        info!(
            query = %params.query,
            mode = %mode,
            limit = limit,
            regex = regex,
            "search_code: entry"
        );

        let pack = params.pack.as_deref();
        let root = params.root.as_deref();

        // Failures route through err_json (the recoverable-tool-error
        // convention every other tool's resolve-error path already uses), so
        // a failed search is error-shaped and distinguishable from a valid
        // empty result, which stays a graded observation. Previously errors
        // were embedded as {error} data INSIDE the graded envelope, making
        // empty-because-failed indistinguishable from empty-because-no-match.
        let response = match mode.as_str() {
            "path" => match self.search.search_path(pack, root, &params.query, limit) {
                Ok(results) => serde_json::json!({
                    "query": params.query,
                    "mode": "path",
                    "results": results,
                }),
                Err(e) => {
                    info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "search_code: path search error");
                    return err_json(&e.to_string());
                }
            },
            "content" => {
                match self
                    .search
                    .search_content(pack, root, &params.query, limit, regex)
                {
                    Ok(results) => serde_json::json!({
                        "query": params.query,
                        "mode": "content",
                        "results": results,
                    }),
                    Err(e) => {
                        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "search_code: content search error");
                        return err_json(&e.to_string());
                    }
                }
            }
            "symbol" => match self.search.search_symbol(pack, root, &params.query, limit) {
                Ok(results) => serde_json::json!({
                    "query": params.query,
                    "mode": "symbol",
                    "results": results,
                }),
                Err(e) => {
                    info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "search_code: symbol search error");
                    return err_json(&e.to_string());
                }
            },
            other => {
                info!(elapsed_ms = %start.elapsed().as_millis(), mode = %other, "search_code: unknown mode");
                return err_json(&format!("Unknown search mode: {}", other));
            }
        };

        let pid = self
            .resolver
            .resolve(pack, root)
            .ok()
            .map(|(dir, _)| pack_id_from_dir(&dir))
            .unwrap_or_default();
        info!(elapsed_ms = %start.elapsed().as_millis(), "search_code: exit");
        ok_epistemic(
            KnowledgeLayerField::Single(KnowledgeLayer::Observation),
            EpistemicConfidence::Mechanical,
            ProvenanceField::Single(Provenance::PackExtraction {
                pack_id: pid,
                source: "pack-index".into(),
            }),
            response,
        )
    }

    // -- get_profile --------------------------------------------------------

    #[tool(
        name = "get_profile",
        description = "Get the project profile: build system, frameworks, entry points, language distribution, modules."
    )]
    fn get_profile(
        &self,
        Parameters(params): Parameters<tools::profile::GetProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::profile::handle_get_profile(&self.resolver, params)
    }

    // -- analyze_health -----------------------------------------------------

    #[tool(
        name = "analyze_health",
        description = "Analyze repository health: structure metrics, dependency risk, and signals."
    )]
    fn analyze_health(
        &self,
        Parameters(params): Parameters<tools::profile::AnalyzeHealthParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::profile::handle_analyze_health(&self.resolver, params)
    }

    // -- get_graph ----------------------------------------------------------

    #[tool(
        name = "get_graph",
        description = "Get the file dependency graph. Optionally extract a neighbor subgraph around a specific file."
    )]
    fn get_graph(
        &self,
        Parameters(params): Parameters<tools::graph::GetGraphParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::handle_get_graph(&self.resolver, params)
    }

    // -- get_callers -------------------------------------------------------

    #[tool(
        name = "get_callers",
        description = "Find files that directly depend on a given file (incoming edges). Entries are incoming dependency edges of six types (imports, extends, implements, calls, type_ref, declares); `calls`-typed edges are extracted for Java (method invocations) and Rust (bare and scoped function calls; receiver method calls excluded), so other languages' results reflect imports/type references, not call sites — see `edgeTypeBreakdown` and `callEdgeCoverage` in the response. Returns up to `limit` results (default 50, max 200); `totalCallers` is the exact pre-truncation count and `truncated` flags a capped response."
    )]
    fn get_callers(
        &self,
        Parameters(params): Parameters<tools::graph::GetCallersParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::handle_get_callers(&self.resolver, params)
    }

    // -- get_dependencies --------------------------------------------------

    #[tool(
        name = "get_dependencies",
        description = "Find all files that a given file directly depends on (outgoing edges)."
    )]
    fn get_dependencies(
        &self,
        Parameters(params): Parameters<tools::graph::GetDependenciesParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::handle_get_dependencies(&self.resolver, params)
    }

    // -- get_impact --------------------------------------------------------

    #[tool(
        name = "get_impact",
        description = "Given changed files, find all transitively affected downstream files with propagation paths. Seed with `files` (array) or `file` (single path, same spelling as get_callers/get_dependencies). `declares` edges (module-tree containment) are not traversed — see `excludedEdgeTypes` in the payload."
    )]
    fn get_impact(
        &self,
        Parameters(params): Parameters<tools::graph::GetImpactParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::handle_get_impact(&self.resolver, params)
    }

    // -- find_cycles --------------------------------------------------------

    #[tool(
        name = "find_cycles",
        description = "Detect dependency cycles (strongly connected components) in the file graph. Returns cycles with all internal structural edges. Optionally filter to cycles involving a specific file."
    )]
    fn find_cycles(
        &self,
        Parameters(params): Parameters<tools::graph::FindCyclesParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::handle_find_cycles(&self.resolver, params)
    }

    // -- find_dead_code -----------------------------------------------------

    #[tool(
        name = "find_dead_code",
        description = "Find files with zero inbound structural dependencies (imports/extends/implements/type_ref). These are potential dead code — nothing in the codebase references them. Entry points are excluded. Supports filtering by language and directory."
    )]
    fn find_dead_code(
        &self,
        Parameters(params): Parameters<tools::graph::FindDeadCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::handle_find_dead_code(&self.resolver, params)
    }

    // -- get_conventions ----------------------------------------------------

    #[tool(
        name = "get_conventions",
        description = "Read project conventions over a curated root-level allowlist (CLAUDE.md, AGENTS.md, GEMINI.md, .github/copilot-instructions.md, .cursorrules, CONVENTIONS.md, CONTRIBUTING.md, BACKLOG.md) plus .claude/rules/*.md, returned as a sources[] manifest that reports found and missing files."
    )]
    fn get_conventions(
        &self,
        Parameters(params): Parameters<tools::conventions::GetConventionsParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::conventions::handle_get_conventions(params)
    }

    // -- get_architecture ---------------------------------------------------

    #[tool(
        name = "get_architecture",
        description = "Get architecture overview (directory-level) or scoped subgraph with connectivity analysis."
    )]
    fn get_architecture(
        &self,
        Parameters(params): Parameters<tools::architecture::GetArchitectureParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::architecture::handle_get_architecture(&self.resolver, params)
    }

    // -- get_summary --------------------------------------------------------

    #[tool(
        name = "get_summary",
        description = "Get a tiered repository summary. Detail levels: 'compact' (~2KB), 'standard' (~8KB), 'full' (~25KB)."
    )]
    fn get_summary(
        &self,
        Parameters(params): Parameters<tools::summary::GetSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::summary::handle_get_summary(&self.resolver, params)
    }

    // -- compare_summaries --------------------------------------------------

    #[tool(
        name = "compare_summaries",
        description = "Compare two scan packs: fitness delta, metric changes, and recommendation diffs."
    )]
    fn compare_summaries(
        &self,
        Parameters(params): Parameters<tools::summary::CompareSummariesParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::summary::handle_compare_summaries(params)
    }

    // -- memorize_insight ---------------------------------------------------

    #[tool(
        name = "memorize_insight",
        description = "Store a memory about the repository. Returns created ID or duplicate detection."
    )]
    fn memorize_insight(
        &self,
        Parameters(params): Parameters<tools::memory::MemorizeInsightParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::memory::handle_memorize_insight(params)
    }

    // -- recall_longterm ----------------------------------------------------

    #[tool(
        name = "recall_longterm",
        description = "Search and rank stored memories by keyword relevance. Runs lazy decay if needed. Returns grounding status per result."
    )]
    fn recall_longterm(
        &self,
        Parameters(params): Parameters<tools::memory::RecallLongtermParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::memory::handle_recall_longterm(&self.resolver, params)
    }

    // -- forget_insight -----------------------------------------------------

    #[tool(
        name = "forget_insight",
        description = "Delete stored memories. Dry-run by default (confirm=true to actually delete)."
    )]
    fn forget_insight(
        &self,
        Parameters(params): Parameters<tools::memory::ForgetInsightParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::memory::handle_forget_insight(params)
    }

    // -- wiki_compile -------------------------------------------------------

    #[tool(
        name = "wiki_compile",
        description = "Create or update a wiki article in the compiled-knowledge layer. \
                       Assesses staleness against the active pack. Returns status, id, path, and staleness."
    )]
    fn wiki_compile(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiCompileParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_compile(&self.resolver, params)
    }

    // -- wiki_read ----------------------------------------------------------

    #[tool(
        name = "wiki_read",
        description = "Read a wiki article by ID, returning its metadata and Markdown body."
    )]
    fn wiki_read(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiReadParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_read(params)
    }

    // -- wiki_search --------------------------------------------------------

    #[tool(
        name = "wiki_search",
        description = "Search wiki articles using BM25 full-text search (with stemming and field boosts), \
                       with optional type/tag filters and phraseQuery for exact-phrase matching. \
                       Falls back to keyword search if the search index is unavailable."
    )]
    fn wiki_search(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_search(params)
    }

    // -- wiki_index ---------------------------------------------------------

    #[tool(
        name = "wiki_index",
        description = "List all wiki articles grouped by type. Optional articleType filter."
    )]
    fn wiki_index(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiIndexParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_index(params)
    }

    // -- wiki_lint ----------------------------------------------------------

    #[tool(
        name = "wiki_lint",
        description = "Lint the wiki: flag stale articles, detect broken memory references, \
                       and report coverage gaps (uncovered files vs wiki anchors)."
    )]
    fn wiki_lint(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiLintParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_lint(&self.resolver, params)
    }

    // -- wiki_delete --------------------------------------------------------

    #[tool(
        name = "wiki_delete",
        description = "Delete a wiki article by ID. Dry-run by default (confirm=true to actually delete)."
    )]
    fn wiki_delete(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiDeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_delete(params)
    }

    // -- wiki_suggest ----------------------------------------------------------

    #[tool(
        name = "wiki_suggest",
        description = "Suggest wiki articles for uncovered modules, ranked by importance (centrality, size, dependencies)."
    )]
    fn wiki_suggest(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiSuggestParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_suggest(&self.resolver, params)
    }

    // -- wiki_ingest_external --------------------------------------------------

    #[tool(
        name = "wiki_ingest_external",
        description = "Ingest pre-fetched external content (API docs, library guides) into the wiki \
                       with source attribution. The agent provides the content; RepoLens stores and indexes it."
    )]
    fn wiki_ingest_external(
        &self,
        Parameters(params): Parameters<tools::wiki::WikiIngestExternalParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::wiki::handle_wiki_ingest_external(&self.resolver, params)
    }
}

#[tool_handler]
impl ServerHandler for RepoLensMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("repolens", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "RepoLens MCP server provides access to scanned codebase packs. \
                 Use discover_packs to find available packs, then search_code or \
                 read_file_content to explore the codebase."
                    .to_string(),
            )
    }
}

impl Default for RepoLensMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the MCP server over STDIO transport.
pub async fn run_stdio_server() -> anyhow::Result<()> {
    info!("RepoLens MCP server starting");
    let server = RepoLensMcpServer::new();
    let service = server.serve(stdio()).await?;
    info!("RepoLens MCP server connected, waiting for requests");
    service.waiting().await?;
    info!("RepoLens MCP server shutting down");
    Ok(())
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "test-helpers"))]
impl RepoLensMcpServer {
    /// Create a server with a shared resolver for testing.
    pub fn with_resolver(resolver: PackResolver) -> Self {
        let search = SearchService::new(PackResolver::new());
        Self {
            tool_router: Self::tool_router(),
            resolver: Arc::new(resolver),
            search: Arc::new(search),
        }
    }

    pub fn call_discover_packs(&self, root: &str) -> String {
        let params = DiscoverPacksParams {
            root: root.to_string(),
        };
        let result = self.discover_packs(Parameters(params));
        extract_text(result)
    }

    pub fn call_discover_packs_envelope(&self, root: &str) -> serde_json::Value {
        let params = DiscoverPacksParams {
            root: root.to_string(),
        };
        extract_envelope(self.discover_packs(Parameters(params)))
    }

    pub fn call_read_file_content(
        &self,
        pack: &str,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> String {
        self.call_read_file_content_detail(pack, path, start_line, end_line, None)
    }

    pub fn call_read_file_content_detail(
        &self,
        pack: &str,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
        detail: Option<&str>,
    ) -> String {
        let params = ReadFileContentParams {
            pack: Some(pack.to_string()),
            root: None,
            path: path.to_string(),
            start_line,
            end_line,
            detail: detail.map(|s| s.to_string()),
        };
        let result = self.read_file_content(Parameters(params));
        extract_text(result)
    }

    pub fn call_search_code(
        &self,
        pack: &str,
        query: &str,
        mode: &str,
        limit: u32,
        regex: bool,
    ) -> String {
        let params = SearchCodeParams {
            pack: Some(pack.to_string()),
            root: None,
            query: query.to_string(),
            mode: Some(mode.to_string()),
            limit: Some(limit),
            regex: Some(regex),
        };
        let result = self.search_code(Parameters(params));
        extract_text(result)
    }

    /// Full-content variant of `call_search_code`: returns the parsed response
    /// without unwrapping `.data`, so tests can assert on envelope presence vs
    /// absence (a failed search is error-shaped, not a graded envelope).
    pub fn call_search_code_envelope(
        &self,
        pack: &str,
        query: &str,
        mode: &str,
        limit: u32,
        regex: bool,
    ) -> serde_json::Value {
        let params = SearchCodeParams {
            pack: Some(pack.to_string()),
            root: None,
            query: query.to_string(),
            mode: Some(mode.to_string()),
            limit: Some(limit),
            regex: Some(regex),
        };
        extract_envelope(self.search_code(Parameters(params)))
    }

    pub fn call_get_profile(&self, pack: &str) -> String {
        let params = tools::profile::GetProfileParams {
            pack: Some(pack.to_string()),
            root: None,
        };
        let result = self.get_profile(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_profile_envelope(&self, pack: &str) -> serde_json::Value {
        let params = tools::profile::GetProfileParams {
            pack: Some(pack.to_string()),
            root: None,
        };
        extract_envelope(self.get_profile(Parameters(params)))
    }

    pub fn call_analyze_health(&self, pack: &str) -> String {
        let params = tools::profile::AnalyzeHealthParams {
            pack: Some(pack.to_string()),
            root: None,
        };
        let result = self.analyze_health(Parameters(params));
        extract_text(result)
    }

    pub fn call_analyze_health_envelope(&self, pack: &str) -> serde_json::Value {
        let params = tools::profile::AnalyzeHealthParams {
            pack: Some(pack.to_string()),
            root: None,
        };
        extract_envelope(self.analyze_health(Parameters(params)))
    }

    pub fn call_get_graph(&self, pack: &str, file: Option<&str>) -> String {
        let params = tools::graph::GetGraphParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.map(|f| f.to_string()),
            depth: None,
        };
        let result = self.get_graph(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_callers(&self, pack: &str, file: &str, limit: Option<usize>) -> String {
        let params = tools::graph::GetCallersParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.to_string(),
            limit,
        };
        let result = self.get_callers(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_dependencies(&self, pack: &str, file: &str, limit: Option<usize>) -> String {
        let params = tools::graph::GetDependenciesParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.to_string(),
            limit,
        };
        let result = self.get_dependencies(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_impact(
        &self,
        pack: &str,
        files: Vec<&str>,
        max_depth: Option<usize>,
        limit: Option<usize>,
    ) -> String {
        let params = tools::graph::GetImpactParams {
            pack: Some(pack.to_string()),
            root: None,
            files: files.into_iter().map(|f| f.to_string()).collect(),
            file: None,
            max_depth,
            limit,
        };
        let result = self.get_impact(Parameters(params));
        extract_text(result)
    }

    /// Test helper: exercise the singular `file` alias path of get_impact
    /// (dogfood round 2, friction finding 3 — sibling tools take `file`).
    pub fn call_get_impact_file(&self, pack: &str, file: &str) -> String {
        let params = tools::graph::GetImpactParams {
            pack: Some(pack.to_string()),
            root: None,
            files: vec![],
            file: Some(file.to_string()),
            max_depth: None,
            limit: None,
        };
        let result = self.get_impact(Parameters(params));
        extract_text(result)
    }

    pub fn call_find_dead_code(
        &self,
        pack: &str,
        language: Option<&str>,
        directory: Option<&str>,
        limit: Option<usize>,
    ) -> String {
        let params = tools::graph::FindDeadCodeParams {
            pack: Some(pack.to_string()),
            root: None,
            language: language.map(|s| s.to_string()),
            directory: directory.map(|s| s.to_string()),
            limit,
        };
        let result = self.find_dead_code(Parameters(params));
        extract_text(result)
    }

    pub fn call_find_dead_code_envelope(&self, pack: &str) -> serde_json::Value {
        let params = tools::graph::FindDeadCodeParams {
            pack: Some(pack.to_string()),
            root: None,
            language: None,
            directory: None,
            limit: None,
        };
        let result = self.find_dead_code(Parameters(params));
        extract_envelope(result)
    }

    pub fn call_get_conventions(&self, root: &str) -> String {
        let params = tools::conventions::GetConventionsParams {
            root: root.to_string(),
        };
        let result = self.get_conventions(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_conventions_envelope(&self, root: &str) -> serde_json::Value {
        let params = tools::conventions::GetConventionsParams {
            root: root.to_string(),
        };
        extract_envelope(self.get_conventions(Parameters(params)))
    }

    pub fn call_get_architecture(&self, pack: &str, scope: Option<&str>) -> String {
        let params = tools::architecture::GetArchitectureParams {
            pack: Some(pack.to_string()),
            root: None,
            scope: scope.map(|s| s.to_string()),
        };
        let result = self.get_architecture(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_summary(&self, root: &str, detail: &str) -> String {
        let params = tools::summary::GetSummaryParams {
            root: root.to_string(),
            no_rescan: Some(true),
            detail: Some(detail.to_string()),
        };
        let result = self.get_summary(Parameters(params));
        extract_text(result)
    }

    pub fn call_get_summary_envelope(&self, root: &str, detail: &str) -> serde_json::Value {
        let params = tools::summary::GetSummaryParams {
            root: root.to_string(),
            no_rescan: Some(true),
            detail: Some(detail.to_string()),
        };
        extract_envelope(self.get_summary(Parameters(params)))
    }

    pub fn call_compare_summaries(&self, before: &str, after: &str) -> String {
        let params = tools::summary::CompareSummariesParams {
            before_pack: before.to_string(),
            after_pack: after.to_string(),
            root: None,
        };
        let result = self.compare_summaries(Parameters(params));
        extract_text(result)
    }

    pub fn call_compare_summaries_envelope(&self, before: &str, after: &str) -> serde_json::Value {
        let params = tools::summary::CompareSummariesParams {
            before_pack: before.to_string(),
            after_pack: after.to_string(),
            root: None,
        };
        let result = self.compare_summaries(Parameters(params));
        extract_envelope(result)
    }

    /// Test helper for `compare_summaries` with packId aliases — passes
    /// `root` so the handler can resolve `pack-YYYYMMDD-HHMMSS` against
    /// `<root>/.repolens/`. Mirrors `call_compare_summaries_envelope`
    /// otherwise.
    pub fn call_compare_summaries_with_root_envelope(
        &self,
        before: &str,
        after: &str,
        root: &str,
    ) -> serde_json::Value {
        let params = tools::summary::CompareSummariesParams {
            before_pack: before.to_string(),
            after_pack: after.to_string(),
            root: Some(root.to_string()),
        };
        let result = self.compare_summaries(Parameters(params));
        extract_envelope(result)
    }

    /// Test helper that surfaces the underlying `Err` from the
    /// `compare_summaries` handler (e.g., `InvalidParams` when packIds
    /// are passed without `root`). Returns the error message as a
    /// `Result::Err(String)` so tests can assert on the exact friction
    /// message agents will see.
    pub fn try_call_compare_summaries(
        &self,
        before: &str,
        after: &str,
        root: Option<&str>,
    ) -> Result<String, String> {
        let params = tools::summary::CompareSummariesParams {
            before_pack: before.to_string(),
            after_pack: after.to_string(),
            root: root.map(|s| s.to_string()),
        };
        match self.compare_summaries(Parameters(params)) {
            Ok(_) => Ok(String::new()),
            Err(e) => Err(format!("{e:?}")),
        }
    }

    pub fn call_memorize_insight(
        &self,
        root: &str,
        content: &str,
        memory_type: Option<&str>,
        importance: Option<f32>,
        tags: Option<Vec<&str>>,
        supersedes: Option<&str>,
    ) -> String {
        let params = tools::memory::MemorizeInsightParams {
            root: root.to_string(),
            content: content.to_string(),
            memory_type: memory_type.map(|t| t.to_string()),
            importance,
            anchors: None,
            tags: tags.map(|t| t.into_iter().map(|s| s.to_string()).collect()),
            supersedes: supersedes.map(|s| s.to_string()),
        };
        extract_text(self.memorize_insight(Parameters(params)))
    }

    pub fn call_recall_longterm(&self, root: &str, query: &str, limit: Option<usize>) -> String {
        let params = tools::memory::RecallLongtermParams {
            root: root.to_string(),
            query: query.to_string(),
            memory_type: None,
            tags: None,
            min_importance: None,
            limit,
        };
        extract_text(self.recall_longterm(Parameters(params)))
    }

    pub fn call_forget_insight(
        &self,
        root: &str,
        id: Option<&str>,
        query: Option<&str>,
        confirm: bool,
    ) -> String {
        let params = tools::memory::ForgetInsightParams {
            root: root.to_string(),
            id: id.map(|s| s.to_string()),
            query: query.map(|s| s.to_string()),
            confirm: Some(confirm),
        };
        extract_text(self.forget_insight(Parameters(params)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn call_wiki_compile(
        &self,
        root: &str,
        title: &str,
        article_type: &str,
        body: &str,
        summary: &str,
        id: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> String {
        let params = tools::wiki::WikiCompileParams {
            root: root.to_string(),
            title: title.to_string(),
            article_type: article_type.to_string(),
            body: body.to_string(),
            summary: summary.to_string(),
            anchors: None,
            memory_refs: None,
            tags: tags.map(|t| t.into_iter().map(|s| s.to_string()).collect()),
            confidence: None,
            id: id.map(|s| s.to_string()),
        };
        extract_text(self.wiki_compile(Parameters(params)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn call_wiki_compile_full(
        &self,
        root: &str,
        title: &str,
        article_type: &str,
        body: &str,
        summary: &str,
        anchors: Option<Vec<&str>>,
        memory_refs: Option<Vec<&str>>,
        tags: Option<Vec<&str>>,
        confidence: Option<f32>,
        id: Option<&str>,
    ) -> String {
        let params = tools::wiki::WikiCompileParams {
            root: root.to_string(),
            title: title.to_string(),
            article_type: article_type.to_string(),
            body: body.to_string(),
            summary: summary.to_string(),
            anchors: anchors.map(|a| a.into_iter().map(|s| s.to_string()).collect()),
            memory_refs: memory_refs.map(|m| m.into_iter().map(|s| s.to_string()).collect()),
            tags: tags.map(|t| t.into_iter().map(|s| s.to_string()).collect()),
            confidence,
            id: id.map(|s| s.to_string()),
        };
        extract_text(self.wiki_compile(Parameters(params)))
    }

    pub fn call_wiki_read(&self, root: &str, id: &str) -> String {
        let params = tools::wiki::WikiReadParams {
            root: root.to_string(),
            id: id.to_string(),
        };
        extract_text(self.wiki_read(Parameters(params)))
    }

    pub fn call_wiki_search(
        &self,
        root: &str,
        query: &str,
        article_type: Option<&str>,
        limit: Option<usize>,
    ) -> String {
        let params = tools::wiki::WikiSearchParams {
            root: root.to_string(),
            query: query.to_string(),
            article_type: article_type.map(|s| s.to_string()),
            tags: None,
            phrase_query: None,
            limit,
        };
        extract_text(self.wiki_search(Parameters(params)))
    }

    pub fn call_wiki_search_phrase(
        &self,
        root: &str,
        query: &str,
        phrase_query: bool,
        limit: Option<usize>,
    ) -> String {
        let params = tools::wiki::WikiSearchParams {
            root: root.to_string(),
            query: query.to_string(),
            article_type: None,
            tags: None,
            phrase_query: Some(phrase_query),
            limit,
        };
        extract_text(self.wiki_search(Parameters(params)))
    }

    pub fn call_wiki_index(&self, root: &str, article_type: Option<&str>) -> String {
        let params = tools::wiki::WikiIndexParams {
            root: root.to_string(),
            article_type: article_type.map(|s| s.to_string()),
        };
        extract_text(self.wiki_index(Parameters(params)))
    }

    pub fn call_wiki_lint(&self, root: &str) -> String {
        let params = tools::wiki::WikiLintParams {
            root: root.to_string(),
        };
        extract_text(self.wiki_lint(Parameters(params)))
    }

    pub fn call_wiki_delete(&self, root: &str, id: &str, confirm: bool) -> String {
        let params = tools::wiki::WikiDeleteParams {
            root: root.to_string(),
            id: id.to_string(),
            confirm: Some(confirm),
        };
        extract_text(self.wiki_delete(Parameters(params)))
    }

    pub fn call_wiki_suggest(
        &self,
        root: &str,
        limit: Option<usize>,
        directory: Option<&str>,
    ) -> String {
        let params = tools::wiki::WikiSuggestParams {
            root: root.to_string(),
            limit,
            directory: directory.map(|s| s.to_string()),
            min_importance: None,
        };
        extract_text(self.wiki_suggest(Parameters(params)))
    }

    pub fn call_wiki_suggest_envelope(
        &self,
        root: &str,
        limit: Option<usize>,
        directory: Option<&str>,
    ) -> serde_json::Value {
        let params = tools::wiki::WikiSuggestParams {
            root: root.to_string(),
            limit,
            directory: directory.map(|s| s.to_string()),
            min_importance: None,
        };
        extract_envelope(self.wiki_suggest(Parameters(params)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn call_wiki_ingest_external(
        &self,
        root: &str,
        title: &str,
        article_type: &str,
        body: &str,
        summary: &str,
        source_origin: &str,
    ) -> String {
        let params = tools::wiki::WikiIngestExternalParams {
            root: root.to_string(),
            title: title.to_string(),
            article_type: article_type.to_string(),
            body: body.to_string(),
            summary: summary.to_string(),
            source_origin: source_origin.to_string(),
            source_version: None,
            anchors: None,
            memory_refs: None,
            tags: None,
            confidence: None,
            id: None,
        };
        extract_text(self.wiki_ingest_external(Parameters(params)))
    }

    // --- Envelope-returning helpers --------------------------------------
    // These bypass `extract_text` so tests can assert on envelope-level
    // fields like `evidenceGrade`, `knowledgeLayer`, `confidence`, and
    // `provenance`. Used by tests that verify tool promotions from
    // `ok_epistemic` (unaudited default) to `ok_epistemic_graded(..., Verified)`.

    pub fn call_get_graph_envelope(&self, pack: &str, file: Option<&str>) -> serde_json::Value {
        let params = tools::graph::GetGraphParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.map(|f| f.to_string()),
            depth: None,
        };
        extract_envelope(self.get_graph(Parameters(params)))
    }

    pub fn call_get_callers_envelope(&self, pack: &str, file: &str) -> serde_json::Value {
        let params = tools::graph::GetCallersParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.to_string(),
            limit: None,
        };
        extract_envelope(self.get_callers(Parameters(params)))
    }

    pub fn call_get_dependencies_envelope(&self, pack: &str, file: &str) -> serde_json::Value {
        let params = tools::graph::GetDependenciesParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.to_string(),
            limit: None,
        };
        extract_envelope(self.get_dependencies(Parameters(params)))
    }

    pub fn call_get_impact_envelope(
        &self,
        pack: &str,
        files: Vec<&str>,
        max_depth: Option<usize>,
    ) -> serde_json::Value {
        let params = tools::graph::GetImpactParams {
            pack: Some(pack.to_string()),
            root: None,
            files: files.into_iter().map(|f| f.to_string()).collect(),
            file: None,
            max_depth,
            limit: None,
        };
        extract_envelope(self.get_impact(Parameters(params)))
    }

    pub fn call_find_cycles(&self, pack: &str, file: Option<&str>, limit: Option<usize>) -> String {
        let params = tools::graph::FindCyclesParams {
            pack: Some(pack.to_string()),
            root: None,
            file: file.map(|s| s.to_string()),
            limit,
        };
        let result = self.find_cycles(Parameters(params));
        extract_text(result)
    }

    pub fn call_find_cycles_envelope(&self, pack: &str) -> serde_json::Value {
        let params = tools::graph::FindCyclesParams {
            pack: Some(pack.to_string()),
            root: None,
            file: None,
            limit: None,
        };
        extract_envelope(self.find_cycles(Parameters(params)))
    }

    pub fn call_read_file_content_outline_envelope(
        &self,
        pack: &str,
        path: &str,
    ) -> serde_json::Value {
        let params = ReadFileContentParams {
            pack: Some(pack.to_string()),
            root: None,
            path: path.to_string(),
            start_line: None,
            end_line: None,
            detail: Some("outline".to_string()),
        };
        extract_envelope(self.read_file_content(Parameters(params)))
    }

    pub fn call_read_file_content_full_envelope(
        &self,
        pack: &str,
        path: &str,
    ) -> serde_json::Value {
        let params = ReadFileContentParams {
            pack: Some(pack.to_string()),
            root: None,
            path: path.to_string(),
            start_line: None,
            end_line: None,
            detail: None,
        };
        extract_envelope(self.read_file_content(Parameters(params)))
    }
}

#[cfg(any(test, feature = "test-helpers"))]
fn extract_raw_text(result: Result<CallToolResult, McpError>) -> String {
    let call_result = result.expect("tool call should not return protocol error");
    let content = call_result
        .content
        .first()
        .expect("should have at least one content item");
    content
        .as_text()
        .expect("content should be text")
        .text
        .clone()
}

/// Extract the `data` field from an epistemic envelope response.
/// Falls back to the raw text if it's not an envelope (e.g., error responses).
#[cfg(any(test, feature = "test-helpers"))]
fn extract_text(result: Result<CallToolResult, McpError>) -> String {
    let text = extract_raw_text(result);
    if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&text) {
        if envelope.get("data").is_some() && envelope.get("knowledgeLayer").is_some() {
            return serde_json::to_string_pretty(&envelope["data"]).unwrap_or(text);
        }
    }
    text
}

/// Extract the full epistemic envelope (including knowledgeLayer, confidence, provenance).
#[cfg(any(test, feature = "test-helpers"))]
pub fn extract_envelope(result: Result<CallToolResult, McpError>) -> serde_json::Value {
    let text = extract_raw_text(result);
    serde_json::from_str(&text).expect("response should be valid JSON")
}

#[cfg(test)]
mod strict_params_tests {
    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::*;

    fn assert_unknown_field_rejected<T>(value: serde_json::Value, field: &str)
    where
        T: DeserializeOwned,
    {
        let err = match serde_json::from_value::<T>(value) {
            Ok(_) => panic!("unknown MCP parameter field must be rejected"),
            Err(err) => err,
        };
        let message = err.to_string();
        assert!(
            message.contains(field),
            "error should name unknown field `{field}`, got: {message}"
        );
    }

    #[test]
    fn read_file_content_rejects_unknown_mode_parameter() {
        assert_unknown_field_rejected::<ReadFileContentParams>(
            json!({
                "root": "/repo",
                "path": "src/lib.rs",
                "mode": "outline"
            }),
            "mode",
        );
    }

    #[test]
    fn get_graph_rejects_unknown_focus_parameter() {
        assert_unknown_field_rejected::<tools::graph::GetGraphParams>(
            json!({
                "root": "/repo",
                "focus": "src/lib.rs",
                "depth": 1
            }),
            "focus",
        );
    }
}
