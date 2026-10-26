use anyhow::Result;
use clap::error::ErrorKind;

// Every `print!`/`println!` in this binary goes through `out::print`, which
// treats a closed stdout (BrokenPipe, e.g. `| head -1`) as a quiet end of
// output instead of panicking. Defined before the `mod` lines so every module
// uses them in place of std's macros.
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::out::print(format_args!($($arg)*))
    };
}
macro_rules! println {
    () => {
        $crate::out::print(format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::out::print(format_args!("{}\n", format_args!($($arg)*)))
    };
}

mod autodraft;
mod brief;
mod compare;
mod eval;
mod findings;
mod init;
mod memory;
mod out;
mod remember;
mod state;
mod task_match;
mod validate_plan;
use clap::{Args, Parser, Subcommand, ValueEnum};
use repolens_app::discover::list_pack_dirs;
use repolens_app::summary::{ComparisonService, SummaryMemoryTier, SummaryService};
use repolens_core::storage::RepoStoragePaths;
use std::ffi::OsString;
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Parser)]
#[command(
    name = "repolens",
    version,
    about = "Understand and control your codebase"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set up the `.repolens/` workspace files, drafted from your repo
    Init(init::InitArgs),
    /// Generate a deterministic preflight brief from the `.repolens/` workspace
    Brief(brief::BriefArgs),
    /// Record session handoff markdown; does not write typed memory
    #[command(
        about = "Record session handoff markdown; does not write typed memory",
        long_about = "Record a session summary into .repolens/session-memory.md as workspace markdown only.\n\n.repolens/memory/memory.json is not modified. Use repolens state to inspect scan packs, typed memory, and session handoff surfaces."
    )]
    Remember(remember::RememberArgs),
    /// Show scan packs, typed memory, and session handoff state (read-only)
    #[command(
        about = "Show scan packs, typed memory, and session handoff state (read-only)",
        long_about = "Show a read-only inventory of RepoLens persistence surfaces: scan packs, typed memory graph, and session handoff markdown.\n\nReads local workspace files only; does not scan, call MCP, or modify files."
    )]
    State(state::StateArgs),
    /// Save, recall, list and forget typed memories (same store as MCP)
    Memory {
        #[command(subcommand)]
        command: memory::MemoryCommands,
    },
    /// Agent evals: list the built-in test cases or start a new run
    Eval(eval::EvalArgs),
    /// Lint a plan markdown file (warnings only — does not prove plans valid)
    ///
    /// Applies a small set of deterministic regex/substring checks (no LLM,
    /// no AST, no MCP) and emits conservative warnings about possible plan
    /// risks. False negatives are acceptable. This is a guardrail, not a
    /// replacement for tests or human review.
    ValidatePlan(validate_plan::ValidatePlanArgs),
    /// Scan a repository and produce a structured pack
    Scan(ScanArgs),
    /// Start the MCP server over STDIO
    Mcp,
    /// Remove old packs beyond the retention limit
    Clean(CleanArgs),
    /// Compare two RepoLens packs and print the delta
    CompareSummaries(CompareSummariesArgs),
    /// What the last scan found, one line each (--verbose for evidence)
    Findings(findings::FindingsArgs),
    /// Bootstrap runtime commands
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapCommands,
    },
    /// Wiki management commands
    Wiki {
        #[command(subcommand)]
        command: WikiCommands,
    },
}

#[derive(Args)]
struct ScanArgs {
    /// Path to the repository root
    #[arg(long)]
    root: Option<PathBuf>,
    /// Force a full scan even if an incremental base is available
    #[arg(long, default_value_t = false)]
    full: bool,
    /// Number of packs to keep after scan (default 3, minimum 1); the newest clean pack is always kept as the incremental base
    #[arg(long, default_value_t = repolens_app::cleanup::DEFAULT_KEEP_COUNT, value_parser = parse_keep)]
    keep: usize,
}

#[derive(Args)]
struct CleanArgs {
    /// Path to the repository root
    #[arg(long)]
    root: PathBuf,
    /// Number of packs to keep (default 3, minimum 1); the newest clean pack is always kept as the incremental base
    #[arg(long, default_value_t = repolens_app::cleanup::DEFAULT_KEEP_COUNT, value_parser = parse_keep)]
    keep: usize,
}

#[derive(Args)]
struct CompareSummariesArgs {
    /// Path to the repository root
    #[arg(long)]
    root: Option<PathBuf>,
    /// Older pack identifier (pack-YYYYMMDD-HHMMSS or bare YYYYMMDD-HHMMSS)
    #[arg(long)]
    before: Option<String>,
    /// Newer pack identifier (pack-YYYYMMDD-HHMMSS or bare YYYYMMDD-HHMMSS)
    #[arg(long)]
    after: Option<String>,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Markdown)]
    format: OutputFormat,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum OutputFormat {
    Markdown,
    Json,
}

#[derive(Subcommand)]
enum WikiCommands {
    /// Rebuild the tantivy search index from wiki articles
    RebuildIndex(WikiRootArgs),
    /// Lint the wiki: flag stale articles, broken memory refs, coverage gaps
    Lint(WikiRootArgs),
}

#[derive(Subcommand)]
enum BootstrapCommands {
    /// Start a bounded bootstrap run and write the frozen plan snapshot
    Start(BootstrapStartArgs),
    /// Finish the active bootstrap run and write a terminal report
    Finish(BootstrapFinishArgs),
    /// Abort the active bootstrap run and write a terminal report
    Abort(BootstrapAbortArgs),
}

#[derive(Args)]
struct WikiRootArgs {
    /// Path to the repository root
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Args)]
struct BootstrapStartArgs {
    /// Path to the repository root
    #[arg(long)]
    root: PathBuf,
    /// Bootstrap task family
    #[arg(long = "task-type")]
    task_type: String,
    /// Human-readable task goal
    #[arg(long)]
    goal: String,
    /// Repo-relative intended path glob
    #[arg(long = "intended-path", required = true)]
    intended_paths: Vec<String>,
}

#[derive(Args)]
struct BootstrapFinishArgs {
    /// Path to the repository root
    #[arg(long)]
    root: PathBuf,
    /// Explicit bootstrap run id
    #[arg(long)]
    run: Option<String>,
}

#[derive(Args)]
struct BootstrapAbortArgs {
    /// Path to the repository root
    #[arg(long)]
    root: PathBuf,
    /// Explicit bootstrap run id
    #[arg(long)]
    run: Option<String>,
}

fn parse_keep(s: &str) -> std::result::Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("'{}' is not a valid number", s))?;
    if n < 1 {
        return Err("--keep must be at least 1".to_string());
    }
    Ok(n)
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    let cli = match Cli::try_parse_from(args.clone()) {
        Ok(cli) => cli,
        Err(err) => {
            if args.get(1).is_some_and(|arg| arg == "bootstrap")
                && !matches!(
                    err.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                )
            {
                emit_bootstrap_error(&err.to_string());
                std::process::exit(1);
            }
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                // clap's `exit()` ignores a failed write; a full stdout
                // must not look like success (#59).
                use std::io::Write;
                match err.print().and_then(|()| std::io::stdout().flush()) {
                    Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => out::fail(&e),
                    _ => std::process::exit(0),
                }
            }
            err.exit();
        }
    };

    let result = run(cli);
    // Output still buffered must reach stdout, or the command fails (#59).
    out::flush();
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

/// Wire `tracing_subscriber` to stderr. All non-MCP subcommands use the default
/// env (RUST_LOG, falls back to `warn`); MCP uses `REPOLENS_LOG` (default `info`)
/// and disables ANSI to keep stdout clean for protocol frames.
fn init_stderr_tracing(env_key: Option<&str>, default_level: &str, ansi: bool) {
    let filter = match env_key {
        Some(key) => tracing_subscriber::EnvFilter::try_from_env(key),
        None => tracing_subscriber::EnvFilter::try_from_default_env(),
    }
    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(ansi && stderr_wants_color())
        .with_env_filter(filter)
        .init();
}

/// Colour log lines only when stderr is a terminal and `NO_COLOR` is unset
/// or empty (#36): redirected or piped output never gets ANSI escapes.
fn stderr_wants_color() -> bool {
    use std::io::IsTerminal;
    color_allowed(
        std::io::stderr().is_terminal(),
        std::env::var_os("NO_COLOR").as_deref(),
    )
}

fn color_allowed(is_terminal: bool, no_color: Option<&std::ffi::OsStr>) -> bool {
    is_terminal && no_color.is_none_or(|v| v.is_empty())
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Init(args) => {
            init_stderr_tracing(None, "warn", true);
            init::run_init(args)
        }
        Commands::Brief(args) => {
            init_stderr_tracing(None, "warn", true);
            brief::run_brief(args)
        }
        Commands::Remember(args) => {
            init_stderr_tracing(None, "warn", true);
            remember::run_remember(args)
        }
        Commands::State(args) => {
            init_stderr_tracing(None, "warn", true);
            state::run_state(args)
        }
        Commands::Memory { command } => {
            init_stderr_tracing(None, "warn", true);
            memory::run_memory(command)
        }
        Commands::Eval(args) => {
            init_stderr_tracing(None, "warn", true);
            eval::run_eval(args)
        }
        Commands::ValidatePlan(args) => {
            init_stderr_tracing(None, "warn", true);
            validate_plan::run_validate_plan(args)
        }
        Commands::Scan(args) => {
            init_stderr_tracing(None, "warn", true);
            run_scan(args)
        }
        Commands::Mcp => {
            init_stderr_tracing(Some("REPOLENS_LOG"), "info", false);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(repolens_mcp::run_stdio_server())
        }
        Commands::Clean(args) => {
            init_stderr_tracing(None, "warn", true);
            run_clean(args)
        }
        Commands::CompareSummaries(args) => {
            init_stderr_tracing(None, "warn", true);
            run_compare_summaries(args)
        }
        Commands::Findings(args) => {
            init_stderr_tracing(None, "warn", true);
            findings::run_findings(args)
        }
        Commands::Bootstrap { command } => {
            init_stderr_tracing(None, "warn", true);
            if let Err(err) = run_bootstrap_command(command) {
                emit_bootstrap_error(&err.to_string());
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::Wiki { command } => {
            init_stderr_tracing(None, "warn", true);
            match command {
                WikiCommands::RebuildIndex(args) => run_wiki_rebuild_index(args),
                WikiCommands::Lint(args) => run_wiki_lint(args),
            }
        }
    }
}

fn canonicalize_root(root: PathBuf) -> Result<PathBuf> {
    root.canonicalize()
        .map_err(|e| anyhow::anyhow!("Cannot resolve --root '{}': {}", root.display(), e))
}

fn resolve_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Cannot resolve current directory: {}", e))?;
    resolve_root_from(None, &cwd)
}

fn resolve_root_from(root: Option<PathBuf>, cwd: &std::path::Path) -> Result<PathBuf> {
    if let Some(root) = root {
        return canonicalize_root(root);
    }

    let cwd = cwd.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "Cannot resolve current directory '{}': {}",
            cwd.display(),
            e
        )
    })?;

    if let Ok(repo) = gix::discover(&cwd) {
        if let Some(workdir) = repo.workdir() {
            return canonicalize_root(workdir.to_path_buf());
        }
    }

    Ok(cwd)
}

fn emit_bootstrap_error(err: &str) {
    eprintln!("{}", serde_json::json!({ "error": err }));
}

fn run_scan(args: ScanArgs) -> Result<()> {
    let root = match args.root {
        Some(root) => canonicalize_root(root)?,
        None => resolve_root()?,
    };

    info!(root = %root.display(), full = args.full, keep = args.keep, "Starting scan");

    let result = repolens_app::scan::ScanService::new().run(&root, args.full, args.keep)?;

    let mode = &result.scan_mode;
    println!(
        "Scan complete ({mode}): {} files ({} included, {} excluded) in {:.2}s",
        result.total_files,
        result.included_files,
        result.excluded_files,
        result.duration.as_secs_f64(),
    );

    if let Some(reason) = &result.fell_back_reason {
        if reason == repolens_app::incremental::NO_CLEAN_BASE_REASON {
            println!("  Fell back to a full scan: no earlier scan ran on a clean worktree.");
            println!(
                "  For incremental scans, commit or ignore your changes, or run git stash -u,"
            );
            println!("  then scan again. That scan is still full; later scans are incremental.");
        } else {
            println!("  Fell back to a full scan: no earlier pack is a usable base ({reason}).");
        }
    }

    if mode == "incremental" {
        println!(
            "  Reprocessed: {}, deleted: {}, carried forward: {}",
            result.reprocessed_files,
            result.deleted_files.unwrap_or(0),
            result.carried_forward_files.unwrap_or(0),
        );
    }

    println!("Pack written to: {}", result.pack_dir.display());

    if let Some(n) = refresh_memory_grounding(&root, &result.pack_dir) {
        let label = if n == 1 { "memory" } else { "memories" };
        println!("Refreshed grounding for {n} {label} against the new pack.");
    }

    Ok(())
}

/// Best-effort: ground every memory against the just-written pack and persist
/// the grounding snapshot. Returns `Some(n)` if it grounded and saved `n`
/// nodes, `None` if it skipped (no memory, or any I/O error). Never fails the
/// scan — all errors are warned and swallowed. No decay is run here.
fn refresh_memory_grounding(root: &std::path::Path, pack_dir: &std::path::Path) -> Option<usize> {
    let pack = match repolens_core::pack_reader::PackReader::read(pack_dir) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "scan: pack read failed; skipping grounding refresh");
            return None;
        }
    };
    let mut guard = match repolens_memory::store::MemoryStore::lock(root) {
        Ok(g) => g,
        Err(e) => {
            warn!(error = %e, "scan: memory load failed; skipping grounding refresh");
            return None;
        }
    };
    if guard.graph().nodes.is_empty() {
        return None;
    }
    let now = time::OffsetDateTime::now_utc();
    let n = repolens_app::grounding::ground_all_memories(guard.graph_mut(), &pack, now);
    guard.graph_mut().updated_at = now;
    if let Err(e) = guard.save() {
        warn!(error = %e, "scan: failed to save grounding snapshot");
        return None;
    }
    Some(n)
}

fn run_compare_summaries(args: CompareSummariesArgs) -> Result<()> {
    let root = match args.root {
        Some(root) => canonicalize_root(root)?,
        None => resolve_root()?,
    };

    let storage_dir = RepoStoragePaths::storage_dir(&root);
    let packs = list_pack_dirs(&root)?;

    let (before_pack, after_pack) = compare::resolve_pack_pair(
        &packs,
        args.before.as_deref(),
        args.after.as_deref(),
        &storage_dir,
    )?;

    let before_id = before_pack
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Pack path has no filename: {}", before_pack.display()))?
        .to_string();
    let after_id = after_pack
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Pack path has no filename: {}", after_pack.display()))?
        .to_string();

    // compare is read-only; use Skip so loading the memory graph for lazy
    // decay does not mutate .repolens/memory/memory.json as a side effect.
    let before_summary =
        SummaryService::summarize(&before_pack, Some(root.as_path()), SummaryMemoryTier::Skip)
            .map_err(|e| anyhow::anyhow!("Failed to build summary for {before_id}: {e}"))?;
    let after_summary =
        SummaryService::summarize(&after_pack, Some(root.as_path()), SummaryMemoryTier::Skip)
            .map_err(|e| anyhow::anyhow!("Failed to build summary for {after_id}: {e}"))?;

    // Phase 4B Step 1 (2026-05-11): fitness scores were retired, so the
    // comparison is now infallible at this layer. Phase 4B Step 3
    // (2026-05-13) retired the `recommendations` delta field; the markdown
    // render now skips its former "Recommendations" section.
    let comparison = ComparisonService::compare_with_pack_ids(
        &before_summary,
        &after_summary,
        &before_id,
        &after_id,
    );

    match args.format {
        OutputFormat::Markdown => {
            // renderer already appends a trailing newline via its final writeln!
            let md = compare::render_comparison_markdown(&comparison, &before_id, &after_id);
            print!("{md}");
        }
        OutputFormat::Json => {
            // to_string_pretty does not include a trailing newline; add one
            let json = serde_json::to_string_pretty(&comparison)?;
            println!("{json}");
        }
    }

    Ok(())
}

fn run_bootstrap_start(args: BootstrapStartArgs) -> Result<()> {
    let root = canonicalize_root(args.root)?;
    let response = repolens_app::bootstrap::BootstrapService::new().start(
        repolens_app::bootstrap::StartBootstrapRunRequest {
            root,
            task_type: args.task_type,
            goal: args.goal,
            intended_paths: args.intended_paths,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

fn run_bootstrap_finish(args: BootstrapFinishArgs) -> Result<()> {
    let root = canonicalize_root(args.root)?;
    let report = repolens_app::bootstrap::BootstrapService::new().finish(
        repolens_app::bootstrap::FinishBootstrapRunRequest {
            root,
            run_id: args.run,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn run_bootstrap_abort(args: BootstrapAbortArgs) -> Result<()> {
    let root = canonicalize_root(args.root)?;
    let report = repolens_app::bootstrap::BootstrapService::new()
        .abort(root.as_path(), args.run.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn run_bootstrap_command(command: BootstrapCommands) -> Result<()> {
    match command {
        BootstrapCommands::Start(args) => run_bootstrap_start(args),
        BootstrapCommands::Finish(args) => run_bootstrap_finish(args),
        BootstrapCommands::Abort(args) => run_bootstrap_abort(args),
    }
}

fn run_wiki_rebuild_index(args: WikiRootArgs) -> Result<()> {
    let root = match args.root {
        Some(root) => canonicalize_root(root)?,
        None => resolve_root()?,
    };

    let index = repolens_wiki::store::WikiStore::load_index(&root)?;
    let tantivy = repolens_wiki::index::WikiSearchIndex::open(&root)?;
    tantivy.rebuild(&index, &root)?;

    println!(
        "Wiki search index rebuilt: {} articles indexed",
        index.articles.len()
    );
    Ok(())
}

fn run_wiki_lint(args: WikiRootArgs) -> Result<()> {
    let root = match args.root {
        Some(root) => canonicalize_root(root)?,
        None => resolve_root()?,
    };

    let mut index = repolens_wiki::store::WikiStore::load_index(&root)?;

    // Discover pack
    let resolver = repolens_app::discover::PackResolver::new();
    let pack = resolver
        .resolve(None, Some(root.to_str().unwrap_or_default()))
        .ok()
        .map(|(_, idx)| idx);

    // Flag stale
    repolens_wiki::staleness::flag_stale_articles(&mut index, pack.as_ref());

    let stale: Vec<&str> = index
        .articles
        .iter()
        .filter(|a| a.staleness == repolens_core::model::wiki::Staleness::Stale)
        .map(|a| a.title.as_str())
        .collect();

    // Coverage
    let coverage_line = if let Some(ref p) = pack {
        let cov = repolens_wiki::coverage::compute_coverage(&index, p);
        format!(
            "  Coverage:            {}/{} files ({:.1}%)",
            cov.total_files - cov.uncovered_files,
            cov.total_files,
            cov.coverage_percent,
        )
    } else {
        "  Coverage:            N/A (no pack available)".to_string()
    };

    // Broken memory refs (Err: memory.json could not be read)
    let broken_refs = repolens_memory::store::MemoryStore::load(&root).map(|graph| {
        let node_ids: std::collections::HashSet<&str> =
            graph.nodes.iter().map(|n| n.id.as_str()).collect();
        index
            .articles
            .iter()
            .flat_map(|a| {
                a.memory_refs
                    .iter()
                    .filter(|r| !node_ids.contains(r.as_str()))
                    .map(move |r| format!("{} in article {}", r, a.title))
            })
            .collect::<Vec<_>>()
    });

    // Save updated index
    index.updated_at = time::OffsetDateTime::now_utc();
    repolens_wiki::store::WikiStore::save_index(&root, &index)?;

    // Print results
    println!("Wiki lint complete:");
    if stale.is_empty() {
        println!("  Stale articles:      0");
    } else {
        println!(
            "  Stale articles:      {}  ({})",
            stale.len(),
            stale.join(", ")
        );
    }
    match &broken_refs {
        Err(e) => println!("  Broken memory refs:  unavailable ({e})"),
        Ok(refs) if refs.is_empty() => println!("  Broken memory refs:  0"),
        Ok(refs) => println!(
            "  Broken memory refs:  {}  ({})",
            refs.len(),
            refs.join(", ")
        ),
    }
    println!("{coverage_line}");

    // Exit code
    if !stale.is_empty() || broken_refs.as_ref().is_ok_and(|r| !r.is_empty()) {
        std::process::exit(1);
    }

    Ok(())
}

fn run_clean(args: CleanArgs) -> Result<()> {
    let root = args
        .root
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Cannot resolve --root '{}': {}", args.root.display(), e))?;

    let result = repolens_app::cleanup::cleanup_old_packs(&root, args.keep)?;

    if result.deleted.is_empty() && result.failed.is_empty() {
        println!("Nothing to clean ({} packs kept)", result.kept);
    } else if result.failed.is_empty() {
        println!(
            "Cleaned {} packs, kept {}",
            result.deleted.len(),
            result.kept
        );
    } else {
        println!(
            "Cleaned {} packs, failed to delete {}, kept {}",
            result.deleted.len(),
            result.failed.len(),
            result.kept,
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn color_only_on_a_terminal_without_no_color() {
        use std::ffi::OsStr;
        assert!(super::color_allowed(true, None));
        assert!(super::color_allowed(true, Some(OsStr::new(""))));
        assert!(!super::color_allowed(false, None));
        assert!(!super::color_allowed(true, Some(OsStr::new("1"))));
    }

    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn scan_defaults_root_to_current_directory() {
        let cli = Cli::try_parse_from(["repolens", "scan"]).unwrap();
        match cli.command {
            Commands::Scan(args) => assert_eq!(args.root, None),
            _ => panic!("expected scan command"),
        }
    }

    #[test]
    fn wiki_lint_defaults_root_to_current_directory() {
        let cli = Cli::try_parse_from(["repolens", "wiki", "lint"]).unwrap();
        match cli.command {
            Commands::Wiki {
                command: WikiCommands::Lint(args),
            } => assert_eq!(args.root, None),
            _ => panic!("expected wiki lint command"),
        }
    }

    #[test]
    fn wiki_rebuild_index_defaults_root_to_current_directory() {
        let cli = Cli::try_parse_from(["repolens", "wiki", "rebuild-index"]).unwrap();
        match cli.command {
            Commands::Wiki {
                command: WikiCommands::RebuildIndex(args),
            } => assert_eq!(args.root, None),
            _ => panic!("expected wiki rebuild-index command"),
        }
    }

    #[test]
    fn scan_explicit_root_overrides_default() {
        let cli = Cli::try_parse_from(["repolens", "scan", "--root", "/tmp"]).unwrap();
        match cli.command {
            Commands::Scan(args) => assert_eq!(args.root, Some(PathBuf::from("/tmp"))),
            _ => panic!("expected scan command"),
        }
    }

    #[test]
    fn resolve_implicit_root_uses_enclosing_git_repo_root() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path();
        let init = Command::new("git")
            .args(["init"])
            .current_dir(repo_root)
            .output()
            .unwrap();
        assert!(init.status.success());
        let nested = repo_root.join("crates/repolens-cli");
        fs::create_dir_all(&nested).unwrap();
        let resolved = resolve_root_from(None, &nested).unwrap();

        assert_eq!(resolved, repo_root.canonicalize().unwrap());
    }

    #[test]
    fn compare_summaries_defaults_all_flags_to_none() {
        let cli = Cli::try_parse_from(["repolens", "compare-summaries"]).unwrap();
        match cli.command {
            Commands::CompareSummaries(args) => {
                assert_eq!(args.root, None);
                assert_eq!(args.before, None);
                assert_eq!(args.after, None);
                assert!(matches!(args.format, OutputFormat::Markdown));
            }
            _ => panic!("expected compare-summaries command"),
        }
    }

    #[test]
    fn compare_summaries_parses_all_explicit_flags() {
        let cli = Cli::try_parse_from([
            "repolens",
            "compare-summaries",
            "--root",
            "/tmp",
            "--before",
            "pack-20260420-112702",
            "--after",
            "pack-20260421-091533",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command {
            Commands::CompareSummaries(args) => {
                assert_eq!(args.root, Some(PathBuf::from("/tmp")));
                assert_eq!(args.before.as_deref(), Some("pack-20260420-112702"));
                assert_eq!(args.after.as_deref(), Some("pack-20260421-091533"));
                assert!(matches!(args.format, OutputFormat::Json));
            }
            _ => panic!("expected compare-summaries command"),
        }
    }

    #[test]
    fn compare_summaries_passes_raw_before_value_through_the_parser() {
        let cli = Cli::try_parse_from([
            "repolens",
            "compare-summaries",
            "--before",
            "20260420-112702",
        ])
        .unwrap();
        match cli.command {
            Commands::CompareSummaries(args) => {
                assert_eq!(args.before.as_deref(), Some("20260420-112702"));
            }
            _ => panic!("expected compare-summaries command"),
        }
    }

    #[test]
    fn compare_summaries_rejects_unknown_format_value() {
        let result = Cli::try_parse_from(["repolens", "compare-summaries", "--format", "yaml"]);
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected parse to fail for unknown format value"),
        };
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn init_defaults_root_to_none_and_force_to_false() {
        let cli = Cli::try_parse_from(["repolens", "init"]).unwrap();
        match cli.command {
            Commands::Init(args) => {
                assert_eq!(args.root, None);
                assert!(!args.force);
            }
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn init_parses_explicit_root_and_force() {
        let cli = Cli::try_parse_from(["repolens", "init", "--root", "/tmp", "--force"]).unwrap();
        match cli.command {
            Commands::Init(args) => {
                assert_eq!(args.root, Some(PathBuf::from("/tmp")));
                assert!(args.force);
            }
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn brief_parses_without_task() {
        // Dogfood round 2 friction finding 1: both eval agents ran bare
        // `brief` and bounced off the required --task (one never retried).
        // A task-less brief now parses; run_brief substitutes an honest
        // default task string.
        let cli = Cli::try_parse_from(["repolens", "brief"]).expect("bare `brief` must parse");
        match cli.command {
            Commands::Brief(args) => assert_eq!(args.task, None),
            _ => panic!("expected brief command"),
        }
    }

    #[test]
    fn brief_parses_task_and_root() {
        let cli = Cli::try_parse_from([
            "repolens",
            "brief",
            "--task",
            "add a flag",
            "--root",
            "/tmp",
        ])
        .unwrap();
        match cli.command {
            Commands::Brief(args) => {
                assert_eq!(args.task.as_deref(), Some("add a flag"));
                assert_eq!(args.root, Some(PathBuf::from("/tmp")));
            }
            _ => panic!("expected brief command"),
        }
    }

    #[test]
    fn remember_requires_file_arg() {
        let result = Cli::try_parse_from(["repolens", "remember"]);
        assert!(
            result.is_err(),
            "remember without --file should fail to parse"
        );
    }

    #[test]
    fn remember_parses_file_and_root() {
        let cli = Cli::try_parse_from([
            "repolens",
            "remember",
            "--file",
            "session-summary.md",
            "--root",
            "/tmp",
        ])
        .unwrap();
        match cli.command {
            Commands::Remember(args) => {
                assert_eq!(args.file, PathBuf::from("session-summary.md"));
                assert_eq!(args.root, Some(PathBuf::from("/tmp")));
            }
            _ => panic!("expected remember command"),
        }
    }

    #[test]
    fn eval_list_parses() {
        let cli = Cli::try_parse_from(["repolens", "eval", "list"]).unwrap();
        match cli.command {
            Commands::Eval(args) => match args.command {
                eval::EvalCommands::List(list) => {
                    assert_eq!(list.root, None);
                }
                _ => panic!("expected eval list"),
            },
            _ => panic!("expected eval command"),
        }
    }

    #[test]
    fn eval_list_accepts_root_for_surface_consistency() {
        let cli =
            Cli::try_parse_from(["repolens", "eval", "list", "--root", "/tmp/does-not-exist"])
                .unwrap();
        match cli.command {
            Commands::Eval(args) => match args.command {
                eval::EvalCommands::List(list) => {
                    assert_eq!(list.root, Some(PathBuf::from("/tmp/does-not-exist")));
                }
                _ => panic!("expected eval list"),
            },
            _ => panic!("expected eval command"),
        }
    }

    #[test]
    fn eval_start_parses_case_and_run_id() {
        let cli = Cli::try_parse_from([
            "repolens",
            "eval",
            "start",
            "--case",
            "ACE-010",
            "--run-id",
            "stale-checkout-001",
            "--root",
            "/tmp",
        ])
        .unwrap();
        match cli.command {
            Commands::Eval(args) => match args.command {
                eval::EvalCommands::Start(start) => {
                    assert_eq!(start.case, "ACE-010");
                    assert_eq!(start.run_id, "stale-checkout-001");
                    assert_eq!(start.root, Some(PathBuf::from("/tmp")));
                }
                _ => panic!("expected eval start"),
            },
            _ => panic!("expected eval command"),
        }
    }

    #[test]
    fn eval_start_requires_case_and_run_id() {
        assert!(Cli::try_parse_from(["repolens", "eval", "start"]).is_err());
        assert!(Cli::try_parse_from(["repolens", "eval", "start", "--case", "ACE-010"]).is_err());
        assert!(Cli::try_parse_from([
            "repolens",
            "eval",
            "start",
            "--run-id",
            "stale-checkout-001"
        ])
        .is_err());
    }

    #[test]
    fn refresh_memory_grounding_stamps_contradicted_for_missing_anchor() {
        use repolens_core::model::memory::{
            GroundingStatus, MemoryGraph, MemoryNode, MemorySource, MemoryType,
        };
        use repolens_memory::store::MemoryStore;
        use time::OffsetDateTime;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/keep.rs"), "pub fn keep() {}\n").unwrap();

        // Real scan → real pack.
        let result = repolens_app::scan::ScanService::new()
            .run(root, true, 3)
            .unwrap();

        // Memory anchored to a path NOT in the pack.
        let now = OffsetDateTime::now_utc();
        let node = MemoryNode {
            id: "m1".to_string(),
            content: "about a deleted module".to_string(),
            memory_type: MemoryType::Decision,
            importance: 0.8,
            created: now,
            last_accessed: now,
            access_count: 0,
            anchors: vec!["src/deleted.rs".to_string()],
            tags: vec![],
            source: MemorySource::Explicit,
            provenance: "test".to_string(),
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256: Default::default(),
        };
        let graph = MemoryGraph {
            schema_version: "1.0".to_string(),
            repo_root: root.to_string_lossy().into_owned(),
            nodes: vec![node],
            edges: vec![],
            last_decay_run: Some(now),
            updated_at: now,
        };
        MemoryStore::save(root, &graph).unwrap();

        let n = refresh_memory_grounding(root, &result.pack_dir);
        assert_eq!(n, Some(1));

        let reloaded = MemoryStore::load(root).unwrap();
        assert_eq!(
            reloaded.nodes[0].last_grounding,
            Some(GroundingStatus::Contradicted)
        );
        assert!(reloaded.nodes[0].last_grounded_at.is_some());
    }

    #[test]
    fn refresh_memory_grounding_skips_when_no_memory() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/keep.rs"), "pub fn keep() {}\n").unwrap();
        let result = repolens_app::scan::ScanService::new()
            .run(root, true, 3)
            .unwrap();
        // No memory.json written → empty graph → skip.
        assert_eq!(refresh_memory_grounding(root, &result.pack_dir), None);
    }

    #[test]
    fn grounding_flips_after_staged_git_mv_and_back_in_incremental_scans() {
        use repolens_core::model::memory::{
            GroundingStatus, MemoryGraph, MemoryNode, MemorySource, MemoryType,
        };
        use repolens_memory::store::MemoryStore;
        use time::OffsetDateTime;

        let git = |root: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("run git");
            assert!(out.status.success(), "git {args:?} failed");
        };
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.com"]);
        git(root, &["config", "user.name", "T"]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn alpha() {}\n").unwrap();
        std::fs::write(root.join("src/keep.rs"), "pub fn keep() {}\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);
        repolens_app::scan::ScanService::new()
            .run(root, false, 100)
            .unwrap();

        let now = OffsetDateTime::now_utc();
        let graph = MemoryGraph {
            schema_version: "1.0".to_string(),
            repo_root: root.to_string_lossy().into_owned(),
            nodes: vec![MemoryNode {
                id: "m1".to_string(),
                content: "alpha note".to_string(),
                memory_type: MemoryType::Decision,
                importance: 0.8,
                created: now,
                last_accessed: now,
                access_count: 0,
                anchors: vec!["src/a.rs".to_string()],
                tags: vec![],
                source: MemorySource::Explicit,
                provenance: "test".to_string(),
                last_grounding: None,
                last_grounded_at: None,
                last_decayed_at: None,
                importance_before_grounding: None,
                anchor_sha256: Default::default(),
            }],
            edges: vec![],
            last_decay_run: Some(now),
            updated_at: now,
        };
        MemoryStore::save(root, &graph).unwrap();

        let scan_and_ground = |root: &std::path::Path| {
            std::thread::sleep(std::time::Duration::from_millis(1100));
            let r = repolens_app::scan::ScanService::new()
                .run(root, false, 100)
                .unwrap();
            assert_eq!(r.scan_mode, "incremental");
            refresh_memory_grounding(root, &r.pack_dir);
            MemoryStore::load(root).unwrap().nodes[0]
                .last_grounding
                .clone()
        };

        git(root, &["mv", "src/a.rs", "src/b.rs"]);
        assert_eq!(scan_and_ground(root), Some(GroundingStatus::Contradicted));
        git(root, &["mv", "src/b.rs", "src/a.rs"]);
        assert_eq!(scan_and_ground(root), Some(GroundingStatus::Confirmed));
        git(root, &["rm", "-q", "src/a.rs"]);
        assert_eq!(scan_and_ground(root), Some(GroundingStatus::Contradicted));
    }
}
