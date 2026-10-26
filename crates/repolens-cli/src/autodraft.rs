//! Day-one auto-drafting for `repolens init`.
//!
//! Fills still-untouched workspace templates (`known-constraints.md`,
//! `repo-overview.md`) with facts read from the repository itself: the
//! README, CI workflows, `Cargo.toml` / `rust-toolchain.toml` /
//! `package.json`, and (when a scan pack exists) the scan summary and top
//! findings.
//!
//! Rules:
//! - Only facts read from files are written; every drafted line carries the
//!   visible tag `(auto-drafted)` and cites the file it came from.
//! - Drafted constraints use ids `D-NNN`, which `validate-plan` does not
//!   enforce (it reads `C-NNN`). A human promotes a draft by checking it and
//!   renaming it to `C-NNN`.
//! - Output is deterministic: no timestamps, all lists sorted or in file
//!   order.
//! - A file is drafted only if it is byte-identical to the shipped template
//!   (or empty). Anything a user has edited is left alone.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use repolens_app::summary::{SummaryMemoryTier, SummaryService};
use repolens_core::model::evidence::FindingSeverity;
use repolens_core::pack_reader::PackReader;

/// Visible tag carried by every drafted line.
pub const TAG: &str = "(auto-drafted)";

const MAX_README_CHARS: usize = 600;
const MAX_FINDINGS: usize = 5;
const MAX_ENTRY_POINTS: usize = 8;

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq)]
pub struct RepoFacts {
    pub readme: Option<ReadmeFacts>,
    pub cargo: Option<CargoFacts>,
    pub toolchain_channel: Option<String>,
    pub package_json: Option<PackageJsonFacts>,
    pub ci: Vec<CiCommand>,
    /// Every workflow file read, whether or not a command was drafted
    /// from it (#13), sorted.
    pub ci_workflows: Vec<String>,
    pub scan: Option<ScanFacts>,
    pub top_dirs: Vec<String>,
    pub convention_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadmeFacts {
    pub path: String,
    pub title: Option<String>,
    pub first_paragraph: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct CargoFacts {
    pub name: Option<String>,
    pub edition: Option<String>,
    pub rust_version: Option<String>,
    /// `true` when the value came from `[workspace.package]`.
    pub rust_version_from_workspace: bool,
    pub is_workspace: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct PackageJsonFacts {
    pub name: Option<String>,
    pub node_engine: Option<String>,
    pub scripts: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CiCommand {
    pub file: String,
    pub line: usize,
    pub workflow: Option<String>,
    pub step: Option<String>,
    pub command: String,
    /// Other `file:line` locations running the identical command.
    pub also_in: Vec<String>,
    /// The workflow line ends in `${{ }}` values set per run (a matrix
    /// entry, an input); `command` is the part before them (#13).
    pub per_run_args: bool,
    /// `working-directory` of the step, or of the job's or workflow's
    /// `defaults.run`, without a leading `./` (QA B2 on #44). `None` for
    /// the repo root.
    pub working_dir: Option<String>,
    /// The folder is a `${{ }}` value set per run (QA B2a on #13).
    pub working_dir_per_run: bool,
    /// The step's `if:` condition, without a `${{ }}` wrapper (#13).
    pub condition: Option<String>,
}

impl CiCommand {
    /// " in `dir/`", " in a folder set per run", or "".
    fn where_phrase(&self) -> String {
        if self.working_dir_per_run {
            " in a folder set per run".into()
        } else {
            self.working_dir
                .as_deref()
                .map(|d| format!(" in `{d}/`"))
                .unwrap_or_default()
        }
    }

    /// " plus arguments set per run" or "".
    fn args_phrase(&self) -> &'static str {
        if self.per_run_args {
            " plus arguments set per run"
        } else {
            ""
        }
    }

    /// " (only when `cond`)" or "". The two status checks GitHub runs most
    /// often are said in words: `always()` and `failure()` (#13).
    fn condition_phrase(&self) -> String {
        match self.condition.as_deref() {
            None => String::new(),
            Some("always()") => " (runs even if earlier steps fail)".into(),
            Some("failure()") => " (runs only if an earlier step failed)".into(),
            Some(c) => format!(" (only when `{c}`)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScanFacts {
    pub pack_id: String,
    pub included_files: i32,
    pub languages: Vec<(String, usize)>,
    pub test_file_count: i32,
    pub source_file_count: i32,
    pub entry_points: Vec<(String, String)>,
    pub findings: Vec<(String, String, String)>,
    /// The findings as `repolens findings` shows them, so the `init`
    /// screen counts and words them the same way (#32).
    pub plain: crate::findings::PlainFindings,
}

/// Collect facts from `root`. `pack_dir` is the active/latest scan pack, if any.
pub fn gather(root: &Path, pack_dir: Option<&Path>) -> RepoFacts {
    let (ci, ci_workflows) = read_ci_commands(root);
    RepoFacts {
        readme: read_readme(root),
        cargo: read_cargo(root),
        toolchain_channel: read_toolchain_channel(root),
        package_json: read_package_json(root),
        ci,
        ci_workflows,
        scan: pack_dir.and_then(|p| read_scan(root, p)),
        top_dirs: read_top_dirs(root),
        convention_files: read_convention_files(root),
    }
}

fn read_readme(root: &Path) -> Option<ReadmeFacts> {
    let name = ["README.md", "Readme.md", "readme.md", "README"]
        .into_iter()
        .find(|n| root.join(n).is_file())?;
    let text = fs::read_to_string(root.join(name)).ok()?;
    let mut title = None;
    let mut para: Vec<String> = Vec::new();
    let mut chosen: Option<Vec<String>> = None;
    let mut in_fence = false;
    // Close the current paragraph; keep it unless it is only a link or badge.
    let finish = |para: &mut Vec<String>, chosen: &mut Option<Vec<String>>| {
        if !para.is_empty() && chosen.is_none() && !is_link_only(&para.join(" ")) {
            *chosen = Some(para.clone());
        }
        para.clear();
    };
    for line in text.lines() {
        if chosen.is_some() {
            break;
        }
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            finish(&mut para, &mut chosen);
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(h) = t.strip_prefix("# ") {
            if title.is_none() && para.is_empty() && chosen.is_none() {
                title = Some(h.trim().to_string());
                continue;
            }
        }
        if t.is_empty() {
            finish(&mut para, &mut chosen);
            continue;
        }
        // Headings, badges, images, HTML and rules end a paragraph.
        let skip = t.starts_with('#')
            || t.starts_with("[![")
            || t.starts_with("![")
            || t.starts_with('<')
            || t == "---";
        if skip {
            finish(&mut para, &mut chosen);
            continue;
        }
        let t = t.strip_prefix('>').map(str::trim).unwrap_or(t);
        if !t.is_empty() {
            para.push(t.to_string());
        }
    }
    finish(&mut para, &mut chosen);
    let first_paragraph = chosen.map(|para| {
        let joined = para.join(" ");
        if joined.chars().count() > MAX_README_CHARS {
            let cut: String = joined.chars().take(MAX_README_CHARS).collect();
            format!("{cut}…")
        } else {
            joined
        }
    });
    Some(ReadmeFacts {
        path: name.to_string(),
        title,
        first_paragraph,
    })
}

/// True for a README paragraph that is only a link or badge line, e.g.
/// `**Live:** https://example.com (redirects here)`: it contains a link or
/// URL, and once links, URLs and code spans are removed no sentence is left
/// (no `.`, `!`, `?` or `。`).
pub fn is_link_only(p: &str) -> bool {
    let has_link = p.contains("http://") || p.contains("https://") || p.contains("](");
    if !has_link {
        return false;
    }
    let mut rest = String::new();
    // Odd-numbered pieces between backticks are code spans.
    for (i, w) in p.split('`').enumerate() {
        if i % 2 == 0 {
            rest.push_str(w);
            rest.push(' ');
        }
    }
    let rest: String = rest
        .split_whitespace()
        .filter(|w| !w.contains("://") && !w.contains("]("))
        .collect::<Vec<_>>()
        .join(" ");
    !rest.contains(['.', '!', '?', '。', '！', '？'])
}

/// Minimal line-based TOML reader: `[section]` headers and `key = "value"`.
fn toml_sections(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let t = line.split('#').next().unwrap_or("").trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = t.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            out.entry(section.clone()).or_default();
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v);
            out.entry(section.clone())
                .or_default()
                .insert(k.trim().to_string(), v.to_string());
        }
    }
    out
}

fn read_cargo(root: &Path) -> Option<CargoFacts> {
    let text = fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let s = toml_sections(&text);
    let pkg = s.get("package");
    let wpkg = s.get("workspace.package");
    let get = |key: &str| -> (Option<String>, bool) {
        if let Some(v) = pkg.and_then(|p| p.get(key)) {
            return (Some(v.clone()), false);
        }
        if pkg
            .and_then(|p| p.get(&format!("{key}.workspace")))
            .is_some()
            || pkg.is_none()
        {
            if let Some(v) = wpkg.and_then(|p| p.get(key)) {
                return (Some(v.clone()), true);
            }
        }
        (None, false)
    };
    let (edition, _) = get("edition");
    let (rust_version, from_ws) = get("rust-version");
    Some(CargoFacts {
        name: pkg.and_then(|p| p.get("name")).cloned(),
        edition,
        rust_version,
        rust_version_from_workspace: from_ws,
        is_workspace: s.contains_key("workspace"),
    })
}

fn read_toolchain_channel(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("rust-toolchain.toml")).ok()?;
    toml_sections(&text)
        .get("toolchain")
        .and_then(|t| t.get("channel"))
        .cloned()
}

fn read_package_json(root: &Path) -> Option<PackageJsonFacts> {
    let text = fs::read_to_string(root.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let scripts = v
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|m| {
            let mut out: Vec<(String, String)> = m
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            out.sort();
            out
        })
        .unwrap_or_default();
    Some(PackageJsonFacts {
        name: v.get("name").and_then(|n| n.as_str()).map(String::from),
        node_engine: v
            .pointer("/engines/node")
            .and_then(|n| n.as_str())
            .map(String::from),
        scripts,
    })
}

/// True for commands that build, test, lint or format (not installs/setup).
fn is_check_command(cmd: &str) -> bool {
    // Quote-aware, so `RUSTFLAGS="-D warnings" cargo build` keeps its env
    // assignment as one word (#13).
    let mut words: Vec<&str> = shell_words(cmd);
    // `RICHGO_FORCE_COLOR=1 PATH=... make richtest`: skip env assignments.
    while words.first().is_some_and(|w| is_env_assignment(w)) {
        words.remove(0);
    }
    // `bundle exec jekyll build`: judge the command bundler runs.
    if words.len() > 2 && words[0] == "bundle" && words[1] == "exec" {
        words.drain(..2);
    }
    // `cargo +nightly fuzz build`: skip the toolchain override.
    if words.first() == Some(&"cargo") && words.get(1).is_some_and(|w| w.starts_with('+')) {
        words.remove(1);
    }
    let (Some(first), second) = (words.first(), words.get(1).copied()) else {
        return false;
    };
    let args = || words.iter().skip(1).filter(|w| !w.starts_with('-'));
    match *first {
        "cargo" => match second {
            Some(
                "test" | "clippy" | "fmt" | "build" | "check" | "doc" | "nextest" | "bench"
                | "miri" | "semver-checks" | "llvm-cov",
            ) => true,
            Some("fuzz") => matches!(words.get(2).copied(), Some("build" | "run" | "check")),
            _ => false,
        },
        "npm" | "pnpm" | "yarn" => matches!(
            second,
            Some("test" | "run" | "run-script" | "build" | "lint" | "check")
        ),
        "go" => matches!(second, Some("test" | "vet" | "build")),
        // Maven lifecycle phases that compile or test.
        "mvn" | "./mvnw" | "mvnw" => args().any(|w| {
            matches!(
                *w,
                "compile" | "test-compile" | "test" | "package" | "verify" | "install"
            )
        }),
        "gradle" | "./gradlew" | "gradlew" => {
            args().any(|w| matches!(*w, "build" | "test" | "check" | "assemble"))
        }
        "jekyll" => matches!(second, Some("build" | "doctor")),
        "rake" | "rspec" => true,
        "pytest" | "make" | "just" | "tox" | "ruff" | "mypy" | "black" | "flake8" | "pylint" => {
            true
        }
        "pre-commit" => second == Some("run"),
        "python" | "python3" if second == Some("-m") => matches!(
            words.get(2).copied(),
            Some(
                "pytest"
                    | "unittest"
                    | "mypy"
                    | "ruff"
                    | "black"
                    | "flake8"
                    | "pylint"
                    | "tox"
                    | "build"
            )
        ),
        // `ruby scripts/validate_site.rb`: an in-repo script whose name says
        // it checks, tests, validates, lints, verifies or builds.
        "python" | "python3" | "ruby" | "node" | "bash" | "sh" => {
            second.is_some_and(is_check_script)
        }
        _ => false,
    }
}

fn is_env_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// A relative script path whose file name names a check, e.g.
/// `scripts/check_images.rb` or `.github/scripts/test_x.py`.
fn is_check_script(arg: &str) -> bool {
    const EXTS: &[&str] = &[".py", ".rb", ".js", ".mjs", ".cjs", ".sh", ".bash"];
    if arg.starts_with('-') || arg.starts_with('/') || !EXTS.iter().any(|e| arg.ends_with(e)) {
        return false;
    }
    let name = arg.rsplit('/').next().unwrap_or(arg).to_ascii_lowercase();
    ["check", "test", "validate", "lint", "verify", "build"]
        .iter()
        .any(|k| {
            name.starts_with(k)
                || name.contains(&format!("_{k}"))
                || name.contains(&format!("-{k}"))
        })
}

/// Last name in a `${{ }}` expression: `matrix.extra-mvn-args || ''` ->
/// `extra-mvn-args`.
fn expr_name(expr: &str) -> String {
    let first = expr
        .split(|c: char| c.is_whitespace() || c == '|')
        .next()
        .unwrap_or("");
    first
        .rsplit('.')
        .next()
        .unwrap_or(first)
        .to_ascii_lowercase()
}

/// True when `flag` is the option whose value `name` holds.
fn flag_takes(flag: &str, name: &str) -> bool {
    if flag.contains('=') || name.is_empty() {
        return false;
    }
    let norm = |s: &str| s.replace('_', "-");
    if let Some(long) = flag.strip_prefix("--") {
        return norm(long) == norm(name);
    }
    match flag.strip_prefix('-') {
        Some(short) if short.chars().count() == 1 => name.starts_with(short),
        _ => false,
    }
}

/// Words of a shell command line, split on whitespace outside quotes and
/// `${{ }}` expressions. Quotes stay in the words.
fn shell_words(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut quote: Option<char> = None;
    let mut in_expr = false;
    let b = s.as_bytes();
    for (i, c) in s.char_indices() {
        if in_expr {
            if c == '}' && b.get(i + 1) == Some(&b'}') {
                in_expr = false;
            }
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c.is_whitespace() {
            if let Some(st) = start.take() {
                out.push(&s[st..i]);
            }
            continue;
        }
        if start.is_none() {
            start = Some(i);
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '$' && s[i..].starts_with("${{") {
            in_expr = true;
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// The part of a shell line that runs the command: an inline `# comment`
/// and output piped onward (`|tee log`, `| tee log`) are cut, outside
/// quotes and `${{ }}`. `||` is kept.
fn strip_comment_and_pipe(s: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut in_expr = false;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_expr {
            if c == b'}' && b.get(i + 1) == Some(&b'}') {
                in_expr = false;
                i += 1;
            }
        } else if let Some(q) = quote {
            if c as char == q {
                quote = None;
            }
        } else {
            match c {
                b'\'' | b'"' => quote = Some(c as char),
                b'$' if s[i..].starts_with("${{") => {
                    in_expr = true;
                    i += 2;
                }
                b'#' if i == 0 || b[i - 1].is_ascii_whitespace() => return s[..i].trim_end(),
                b'|' if b.get(i + 1) == Some(&b'|') => i += 1,
                b'|' => return s[..i].trim_end(),
                _ => {}
            }
        }
        i += 1;
    }
    s.trim_end()
}

/// Output redirects (`> out.log`, `>>log`, `2>&1`, `&>/dev/null`) are not
/// part of the command (#13).
fn drop_redirects(s: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut words = shell_words(s).into_iter();
    while let Some(w) = words.next() {
        let op = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        let Some(target) = op.strip_prefix(">>").or_else(|| op.strip_prefix('>')) else {
            out.push(w);
            continue;
        };
        if w.len() - op.len() > 1 {
            out.push(w);
            continue;
        }
        // `2>&1` names its target; a bare `>` takes the next word.
        if target.is_empty() {
            words.next();
        }
    }
    out.join(" ")
}

/// Options whose next word is their value, for cutting a per-run value
/// whose name doesn't match the flag (`--features ${{ matrix.f }}`).
const VALUE_FLAGS: &[&str] = &[
    "--features",
    "-F",
    "--target",
    "--package",
    "-p",
    "--bin",
    "--example",
    "--test",
    "--bench",
    "--profile",
    "--manifest-path",
    "--target-dir",
    "--exclude",
    "--projects",
    "-pl",
    "--activate-profiles",
    "-P",
];

/// The command text to draft from one workflow line, and whether `${{ }}`
/// values set per run were cut from its end. Comments, output piped onward
/// (`… 2>&1 | tee log`) and redirects are dropped. `None` when a value that
/// can't be expanded sits before the end of the command.
fn ci_command_text(line: &str, env: &BTreeMap<String, String>) -> Option<(String, bool)> {
    let line = drop_redirects(strip_comment_and_pipe(line));
    if let Some(c) = expand_expressions(&line, env) {
        return Some((c, false));
    }
    // Cut at the first expression that can't be expanded; everything after
    // it must be more expressions (the run-time argument list).
    let mut rest = line.as_str();
    let mut head = String::new();
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let end = after.find("}}")?;
        let expr = after[..end].trim();
        match expr.strip_prefix("env.").and_then(|n| env.get(n)) {
            Some(v) => {
                head.push_str(&rest[..start]);
                head.push_str(v);
                rest = &after[end + 2..];
            }
            None => {
                head.push_str(&rest[..start]);
                let mut tail = &rest[start..];
                while let Some(t) = tail.trim_start().strip_prefix("${{") {
                    tail = &t[t.find("}}")? + 2..];
                }
                if !tail.trim().is_empty() || !rest[..start].ends_with(char::is_whitespace) {
                    return None;
                }
                let mut words: Vec<&str> = head.split_whitespace().collect();
                // The flag goes too only when the value is its argument,
                // which we take from the names: `--target ${{ matrix.target }}`,
                // `-p ${{ matrix.package }}`. A standalone flag before an
                // argument list (`--release ${{ matrix.flags }}`) stays.
                let name = expr_name(expr);
                if words
                    .last()
                    .is_some_and(|w| flag_takes(w, &name) || VALUE_FLAGS.contains(w))
                {
                    words.pop();
                }
                return Some((words.join(" "), true));
            }
        }
    }
    None
}

fn read_ci_commands(root: &Path) -> (Vec<CiCommand>, Vec<String>) {
    let dir = root.join(".github/workflows");
    let Ok(rd) = fs::read_dir(&dir) else {
        return (vec![], vec![]);
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
        .collect();
    files.sort();
    let mut out: Vec<CiCommand> = Vec::new();
    let mut read: Vec<String> = Vec::new();
    for f in files {
        let Ok(text) = fs::read_to_string(&f) else {
            continue;
        };
        let rel = format!(
            ".github/workflows/{}",
            f.file_name().unwrap().to_string_lossy()
        );
        read.push(rel.clone());
        let mut workflow = None;
        let mut step: Option<String> = None;
        let lines: Vec<&str> = text.lines().collect();
        let env = static_workflow_env(&lines);
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let t = line.trim();
            if let Some(n) = line.strip_prefix("name:") {
                workflow = Some(unquote(n.trim()));
            }
            if let Some(n) = t.strip_prefix("- name:") {
                step = Some(unquote(n.trim()));
            } else if t.starts_with("- ") {
                step = None;
            }
            let run = t.strip_prefix("- run:").or_else(|| t.strip_prefix("run:"));
            if let Some(r) = run {
                let r = r.trim();
                let (working_dir, working_dir_per_run) =
                    match step_working_dir(&lines, i).or_else(|| defaults_working_dir(&lines, i)) {
                        Some(WorkDir::Dir(d)) => (Some(d), false),
                        Some(WorkDir::PerRun) => (None, true),
                        Some(WorkDir::Root) | None => (None, false),
                    };
                let condition = step_key(&lines, i, "if").and_then(|v| clean_condition(&v));
                let mut cmds: Vec<(usize, String)> = Vec::new();
                if r.starts_with('|') || r.starts_with('>') {
                    let folded = r.starts_with('>');
                    let indent = line.len() - line.trim_start().len();
                    let mut j = i + 1;
                    // Shell lines of the block: `>` folds lines at the
                    // block's own indentation into one; a blank or
                    // more-indented line keeps its line break (#13).
                    let mut body: Vec<(usize, String)> = Vec::new();
                    let mut base: Option<usize> = None;
                    let mut fold_open = false;
                    while j < lines.len() {
                        let l = lines[j];
                        let li = l.len() - l.trim_start().len();
                        if !l.trim().is_empty() && li <= indent {
                            break;
                        }
                        let text = l.trim();
                        if text.is_empty() {
                            fold_open = false;
                        } else {
                            let at_base = *base.get_or_insert(li) == li;
                            match body.last_mut() {
                                Some((_, prev)) if folded && fold_open && at_base => {
                                    prev.push(' ');
                                    prev.push_str(text);
                                }
                                _ => body.push((j + 1, text.to_string())),
                            }
                            fold_open = at_base;
                        }
                        j += 1;
                    }
                    i = j;
                    let mut heredoc: Option<String> = None;
                    for (n, text) in body {
                        // A heredoc's body is data, not commands (#13).
                        if let Some(end) = &heredoc {
                            if text == *end {
                                heredoc = None;
                            }
                            continue;
                        }
                        // A trailing backslash continues the shell command
                        // on the next line: join them, keeping the first
                        // line's number (#17).
                        match cmds.last_mut() {
                            Some((_, prev)) if prev.ends_with('\\') => {
                                prev.pop();
                                let prev_trimmed = prev.trim_end().len();
                                prev.truncate(prev_trimmed);
                                prev.push(' ');
                                prev.push_str(&text);
                            }
                            _ => cmds.push((n, text.clone())),
                        }
                        heredoc = heredoc_delimiter(&text);
                    }
                } else {
                    cmds.push((i + 1, unquote(r)));
                    i += 1;
                }
                for (lineno, c) in cmds {
                    // Expand `${{ env.X }}` where X has one fixed value in
                    // this file; drop commands with anything left unexpanded
                    // (matrix values, secrets, env set at run time).
                    let Some((c, per_run_args)) = ci_command_text(&c, &env) else {
                        continue;
                    };
                    if c.is_empty() || c.starts_with('#') || !is_check_command(&c) {
                        continue;
                    }
                    if let Some(prev) = out.iter_mut().find(|o| {
                        o.command == c
                            && o.working_dir == working_dir
                            && o.working_dir_per_run == working_dir_per_run
                            && o.condition == condition
                    }) {
                        prev.also_in.push(format!("{rel}:{lineno}"));
                        continue;
                    }
                    out.push(CiCommand {
                        file: rel.clone(),
                        line: lineno,
                        workflow: workflow.clone(),
                        step: step.clone(),
                        command: c,
                        also_in: vec![],
                        per_run_args,
                        working_dir: working_dir.clone(),
                        working_dir_per_run,
                        condition: condition.clone(),
                    });
                }
                continue;
            }
            i += 1;
        }
    }
    (out, read)
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// A `working-directory:` value.
#[derive(Debug, Clone, PartialEq)]
enum WorkDir {
    /// `.` or `./`: the repo root.
    Root,
    Dir(String),
    /// A `${{ }}` value set per run.
    PerRun,
}

fn clean_dir(v: &str) -> WorkDir {
    let v = unquote(v.split(" #").next().unwrap_or(v));
    if v.contains("${{") {
        return WorkDir::PerRun;
    }
    let v = v.trim_start_matches("./").trim_end_matches('/');
    if v.is_empty() || v == "." {
        WorkDir::Root
    } else {
        WorkDir::Dir(v.to_string())
    }
}

/// `if:` value of a step, without a `${{ }}` wrapper or a trailing comment.
fn clean_condition(v: &str) -> Option<String> {
    let v = unquote(strip_value_comment(v));
    let v = v
        .strip_prefix("${{")
        .and_then(|x| x.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(&v)
        .to_string();
    (!v.is_empty()).then_some(v)
}

/// A value with a trailing ` # comment` cut (outside quotes).
fn strip_value_comment(v: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in v.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '#' && (i == 0 || v[..i].ends_with(char::is_whitespace)) => {
                return v[..i].trim();
            }
            None => {}
        }
    }
    v.trim()
}

/// Delimiter of a heredoc started on this shell line (`<<EOF`, `<<-'EOF'`,
/// `<< "EOF"`), not a `<<<` here-string.
fn heredoc_delimiter(line: &str) -> Option<String> {
    let mut rest = line;
    while let Some(k) = rest.find("<<") {
        let after = &rest[k + 2..];
        if after.starts_with('<') {
            rest = after.trim_start_matches('<');
            continue;
        }
        let after = after.strip_prefix('-').unwrap_or(after).trim_start();
        let word: String = after
            .chars()
            .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | '>' | ')'))
            .filter(|c| *c != '\'' && *c != '"' && *c != '\\')
            .collect();
        return (!word.is_empty()).then_some(word);
    }
    None
}

/// Raw value of `key:` among the keys of the step holding the `run:` on
/// line `i`, before or after it.
fn step_key(lines: &[&str], i: usize, key_name: &str) -> Option<String> {
    let line = lines[i];
    let dash = line.trim_start().starts_with("- ");
    // Column of the step's keys.
    let key = indent_of(line) + if dash { 2 } else { 0 };
    let is_key = |l: &str| {
        let t = l.trim_start();
        (indent_of(l) == key && !t.starts_with("- "))
            || (indent_of(l) + 2 == key && t.starts_with("- "))
    };
    let prefix = format!("{key_name}:");
    let value = |l: &str| {
        let t = l.trim_start();
        t.strip_prefix("- ")
            .unwrap_or(t)
            .strip_prefix(prefix.as_str())
            .map(|v| v.trim().to_string())
    };
    // Backward to the step's `- ` line.
    let mut j = i;
    loop {
        let l = lines[j];
        if !l.trim().is_empty() && is_key(l) {
            if let Some(d) = value(l) {
                return Some(d);
            }
            if l.trim_start().starts_with("- ") {
                break;
            }
        } else if !l.trim().is_empty() && indent_of(l) < key.saturating_sub(2) {
            break;
        }
        if j == 0 {
            break;
        }
        j -= 1;
    }
    // Forward to the next step or the end of the steps list.
    for l in &lines[i + 1..] {
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        if indent_of(l) < key || l.trim_start().starts_with("- ") && indent_of(l) + 2 <= key {
            break;
        }
        if indent_of(l) == key {
            if let Some(d) = value(l) {
                return Some(d);
            }
        }
    }
    None
}

/// `working-directory:` of the step holding the `run:` on line `i`.
fn step_working_dir(lines: &[&str], i: usize) -> Option<WorkDir> {
    step_key(lines, i, "working-directory").map(|v| clean_dir(&v))
}

/// `defaults: run: working-directory:` of the job holding line `i`, else of
/// the workflow.
fn defaults_working_dir(lines: &[&str], i: usize) -> Option<WorkDir> {
    // Parents of each `working-directory:` line, by indentation.
    let parent = |k: usize| -> Option<usize> {
        let ind = indent_of(lines[k]);
        (0..k)
            .rev()
            .find(|&j| !lines[j].trim().is_empty() && indent_of(lines[j]) < ind)
    };
    // Job of a line: its ancestor whose parent is `jobs:`.
    let job_of = |k: usize| -> Option<usize> {
        let mut cur = k;
        while let Some(p) = parent(cur) {
            if lines[p].trim() == "jobs:" {
                return Some(cur);
            }
            cur = p;
        }
        None
    };
    let my_job = job_of(i);
    let mut workflow_level = None;
    for (k, l) in lines.iter().enumerate() {
        let Some(v) = l.trim().strip_prefix("working-directory:") else {
            continue;
        };
        let Some(run) = parent(k).filter(|&p| lines[p].trim() == "run:") else {
            continue;
        };
        let Some(defaults) = parent(run).filter(|&p| lines[p].trim() == "defaults:") else {
            continue;
        };
        if indent_of(lines[defaults]) == 0 {
            workflow_level = Some(clean_dir(v));
        } else if my_job.is_some() && job_of(defaults) == my_job {
            return Some(clean_dir(v));
        }
    }
    workflow_level
}

/// `env:` values in a workflow file that are fixed: defined with one literal
/// value (no `${{ }}`) and never rewritten at run time via `$GITHUB_ENV`.
fn static_workflow_env(lines: &[&str]) -> BTreeMap<String, String> {
    let mut defs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut env_indent: Option<usize> = None;
    for line in lines {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(ei) = env_indent {
            if indent > ei {
                if let Some((k, v)) = t.split_once(':') {
                    let v = v.split(" #").next().unwrap_or(v);
                    defs.entry(k.trim().to_string())
                        .or_default()
                        .push(unquote(v.trim()));
                }
                continue;
            }
            env_indent = None;
        }
        if t == "env:" || t == "- env:" {
            env_indent = Some(indent);
        }
    }
    let rewritten = |k: &str| {
        lines
            .iter()
            .any(|l| l.contains("GITHUB_ENV") && l.contains(&format!("{k}=")))
    };
    defs.into_iter()
        .filter_map(|(k, mut vs)| {
            vs.dedup();
            let ok = vs.len() == 1 && !vs[0].contains("${{") && !rewritten(&k);
            ok.then(|| (k, vs.remove(0)))
        })
        .collect()
}

/// Replace `${{ env.X }}` with its fixed value. `None` if any `${{ }}`
/// expression can't be resolved that way.
fn expand_expressions(cmd: &str, env: &BTreeMap<String, String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = cmd;
    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let end = after.find("}}")?;
        let expr = after[..end].trim();
        let name = expr.strip_prefix("env.")?;
        out.push_str(env.get(name)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Some(out.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(s)
        .to_string()
}

fn read_scan(root: &Path, pack_dir: &Path) -> Option<ScanFacts> {
    let index = PackReader::read(pack_dir).ok()?;
    let mut langs: BTreeMap<String, usize> = BTreeMap::new();
    for e in index.entries.iter().filter(|e| e.included) {
        *langs.entry(e.language.clone()).or_default() += 1;
    }
    let mut languages: Vec<(String, usize)> = langs.into_iter().collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let summary = SummaryService::summarize(pack_dir, Some(root), SummaryMemoryTier::Skip).ok()?;
    let mut entry_points: Vec<(String, String)> = summary
        .profile
        .entry_points
        .iter()
        .map(|e| (e.path.clone(), e.entry_type.clone()))
        .collect();
    entry_points.sort();
    entry_points.dedup();
    entry_points.truncate(MAX_ENTRY_POINTS);

    let rank = |s: &FindingSeverity| match s {
        FindingSeverity::High => 0,
        FindingSeverity::Medium => 1,
        FindingSeverity::Low => 2,
        FindingSeverity::None => 3,
    };
    let mut findings: Vec<_> = summary
        .evidence_findings
        .iter()
        .filter(|f| !matches!(f.severity, FindingSeverity::None))
        .map(|f| {
            (
                rank(&f.severity),
                f.id.as_str().to_string(),
                f.title.clone(),
                format!("{:?}", f.severity),
            )
        })
        .collect();
    findings.sort();
    let plain = crate::findings::PlainFindings::from_findings(&summary.evidence_findings);
    let findings = findings
        .into_iter()
        .take(MAX_FINDINGS)
        .map(|(_, id, title, sev)| (id, title, sev))
        .collect();

    Some(ScanFacts {
        pack_id: pack_dir
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default(),
        included_files: summary.scan.included_count,
        languages,
        test_file_count: summary.scan.test_file_count,
        source_file_count: summary.scan.source_file_count,
        entry_points,
        findings,
        plain,
    })
}

fn read_top_dirs(root: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(root) else {
        return vec![];
    };
    let mut dirs: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.') && n != "target" && n != "node_modules")
        .collect();
    dirs.sort();
    dirs
}

fn read_convention_files(root: &Path) -> Vec<String> {
    [
        "AGENTS.md",
        "CLAUDE.md",
        "CONTRIBUTING.md",
        "CONVENTIONS.md",
        "GEMINI.md",
        ".cursorrules",
        ".github/copilot-instructions.md",
    ]
    .into_iter()
    .filter(|n| root.join(n).is_file())
    .map(String::from)
    .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

const HEADER: &str = "<!--
Auto-drafted by `repolens init` from files in this repository.
Every drafted section heading is marked (auto-drafted); each line cites the
file it was read from. Nothing here is verified by a human yet.
`repolens init` never overwrites this file once you have edited it
(`--force` first copies it to .repolens/backup/).
-->";

/// A drafted constraint: (short name, statement, evidence).
struct Draft {
    name: String,
    statement: String,
    evidence: String,
}

fn constraint_drafts(f: &RepoFacts) -> Vec<Draft> {
    let mut out = Vec::new();
    if let Some(c) = &f.cargo {
        if let Some(rv) = &c.rust_version {
            let section = if c.rust_version_from_workspace {
                "[workspace.package]"
            } else {
                "[package]"
            };
            out.push(Draft {
                name: format!("Minimum Rust version {rv}"),
                statement: format!("`Cargo.toml` declares `rust-version = \"{rv}\"`; code must build with Rust {rv} or newer."),
                evidence: format!("`Cargo.toml` {section} `rust-version`"),
            });
        }
        if let Some(ed) = &c.edition {
            out.push(Draft {
                name: format!("Rust edition {ed}"),
                statement: format!("`Cargo.toml` sets `edition = \"{ed}\"`."),
                evidence: "`Cargo.toml` `edition`".into(),
            });
        }
    }
    if let Some(ch) = &f.toolchain_channel {
        out.push(Draft {
            name: format!("Pinned Rust toolchain {ch}"),
            statement: format!("`rust-toolchain.toml` pins `channel = \"{ch}\"`."),
            evidence: "`rust-toolchain.toml` `[toolchain] channel`".into(),
        });
    }
    if let Some(p) = &f.package_json {
        if let Some(n) = &p.node_engine {
            out.push(Draft {
                name: format!("Node version {n}"),
                statement: format!("`package.json` declares `engines.node = \"{n}\"`."),
                evidence: "`package.json` `engines.node`".into(),
            });
        }
    }
    for c in &f.ci {
        let wf = c
            .workflow
            .as_deref()
            .map(|w| format!("workflow \"{w}\""))
            .unwrap_or_else(|| "a CI workflow".into());
        let step = c
            .step
            .as_deref()
            .map(|s| format!(", step \"{s}\""))
            .unwrap_or_default();
        out.push(Draft {
            name: format!("CI runs `{}`{}", c.command, c.where_phrase()),
            statement: format!(
                "CI {wf}{step} runs `{}`{}{}{}. Check whether it gates merges before treating it as required.",
                c.command,
                c.args_phrase(),
                c.where_phrase(),
                c.condition_phrase()
            ),
            evidence: std::iter::once(format!("{}:{}", c.file, c.line))
                .chain(c.also_in.iter().cloned())
                .map(|l| format!("`{l}`"))
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    out
}

/// The tool a CI command runs, for grouping its drafts (#45): the first
/// word after env assignments, `sudo` and `bundle exec`, path removed, so
/// `RUSTFLAGS=x cargo +nightly test` is `cargo`.
pub fn ci_tool(cmd: &str) -> String {
    let mut words = cmd.split_whitespace().peekable();
    while words
        .peek()
        .is_some_and(|w| is_env_assignment(w) || *w == "sudo")
    {
        words.next();
    }
    let first = words.next().unwrap_or("");
    let first = if first == "bundle" && words.peek() == Some(&"exec") {
        words.nth(1).unwrap_or(first)
    } else {
        first
    };
    // `/usr/bin/make` is `make`; a repo script keeps its path.
    if first.starts_with('/') {
        first.rsplit('/').next().unwrap_or(first).to_string()
    } else {
        first.to_string()
    }
}

/// CI drafts grouped by tool: `(tool, draft numbers)`, most drafts first,
/// ties in the order the tool first appears. Numbers are the `D-NNN` ids
/// `render_known_constraints` gives, so each tool appears once (#45).
pub fn ci_draft_groups(f: &RepoFacts) -> Vec<(String, Vec<usize>)> {
    let first_ci = constraint_drafts(f).len() - f.ci.len() + 1;
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, c) in f.ci.iter().enumerate() {
        let tool = ci_tool(&c.command);
        match groups.iter_mut().find(|(t, _)| *t == tool) {
            Some((_, ids)) => ids.push(first_ci + i),
            None => groups.push((tool, vec![first_ci + i])),
        }
    }
    // Stable sort keeps first-appearance order among equal counts.
    groups.sort_by_key(|g| std::cmp::Reverse(g.1.len()));
    groups
}

/// The ids as `D-NNN` runs, a run of 2+ written `D-003–D-009`, each with
/// how many ids it covers.
fn id_ranges(ids: &[usize]) -> Vec<(String, usize)> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[j] + 1 {
            j += 1;
        }
        parts.push(if j > i {
            (format!("D-{:03}\u{2013}D-{:03}", ids[i], ids[j]), j - i + 1)
        } else {
            (format!("D-{:03}", ids[i]), 1)
        });
        i = j + 1;
    }
    parts
}

/// Widest summary line, in terminal columns.
const SUMMARY_WIDTH: usize = 80;

/// Columns of `s`, with `…` counted as 2 (it is wide in CJK terminals).
fn summary_cols(s: &str) -> usize {
    crate::findings::cols(s) + s.matches('\u{2026}').count()
}

/// `- cargo: 7 drafts (D-003–D-009)`. When the ids interleave with another
/// tool's and the line would pass 80 columns, the first runs are listed
/// and the rest counted: `(D-003, D-005 … and 12 more)`.
fn ci_summary_line(tool: &str, ids: &[usize]) -> String {
    let n = ids.len();
    let drafts = if n == 1 { "draft" } else { "drafts" };
    let head = format!("- {tool}: {n} {drafts} (");
    let parts = id_ranges(ids);
    let all: Vec<&str> = parts.iter().map(|(p, _)| p.as_str()).collect();
    let full = format!("{head}{})", all.join(", "));
    if summary_cols(&full) <= SUMMARY_WIDTH {
        return full;
    }
    let mut keep = parts.len() - 1;
    loop {
        let shown: usize = parts[..keep].iter().map(|(_, c)| c).sum();
        let list = all[..keep].join(", ");
        let sep = if keep == 0 { "" } else { " " };
        let line = format!("{head}{list}{sep}\u{2026} and {} more)", n - shown);
        if keep == 0 || summary_cols(&line) <= SUMMARY_WIDTH {
            return line;
        }
        keep -= 1;
    }
}

/// One line per tool, e.g. `- cargo: 7 drafts (D-003–D-009)`, for the top
/// of the drafted constraints. Empty when CI ran nothing drafted.
fn ci_summary(f: &RepoFacts) -> String {
    let groups = ci_draft_groups(f);
    if groups.is_empty() {
        return String::new();
    }
    let mut s = String::from("CI commands by tool (each one is drafted in full below):\n\n");
    for (tool, ids) in &groups {
        s.push_str(&ci_summary_line(tool, ids));
        s.push('\n');
    }
    s.push('\n');
    s
}

/// Render the drafted `known-constraints.md`, or `None` when the repo offers
/// nothing to draft from.
pub fn render_known_constraints(f: &RepoFacts) -> Option<(String, usize)> {
    let drafts = constraint_drafts(f);
    let findings = f
        .scan
        .as_ref()
        .map(|s| s.findings.as_slice())
        .unwrap_or(&[]);
    if drafts.is_empty() && findings.is_empty() {
        return None;
    }
    let mut s = String::new();
    s.push_str(HEADER);
    s.push_str("\n\n# Known Constraints\n\n");
    s.push_str(
        "> Drafted from this repo's files. Drafts use ids `D-NNN`; `repolens validate-plan` \
         only enforces `C-NNN`. To promote a draft, check it, rename it to `C-NNN`, and set \
         **Verified by:** human.\n\n---\n\n",
    );
    s.push_str(&format!("## Drafted constraints {TAG}\n\n"));
    if drafts.is_empty() {
        s.push_str("- No manifest or CI facts found to draft from.\n\n");
    }
    s.push_str(&ci_summary(f));
    for (i, d) in drafts.iter().enumerate() {
        s.push_str(&format!("### D-{:03} {} {TAG}\n\n", i + 1, d.name));
        s.push_str(&format!("- **Statement:** {}\n", d.statement));
        s.push_str(&format!("- **Evidence:** {}\n", d.evidence));
        s.push_str("- **Verified by:** nobody yet (drafted by repolens init)\n");
        s.push_str(&format!(
            "- **Status:** draft {TAG}, not enforced until renamed to `C-NNN`\n\n"
        ));
    }
    if let Some(scan) = &f.scan {
        if !scan.findings.is_empty() {
            s.push_str(&format!(
                "## Signals from the latest scan {TAG}\n\n\
                 These are findings from `{}`, not constraints. Run `repolens findings` for evidence \
                 and what each one does not prove.\n\n",
                scan.pack_id
            ));
            for (id, title, sev) in &scan.findings {
                s.push_str(&format!("- `{id}` ({sev}): {title}\n"));
            }
            s.push('\n');
        }
    }
    s.push_str(
        "---\n\n## Active constraints\n\n\
         Human-verified constraints (`C-NNN`) go here.\n\n\
         ## Retired constraints\n\nNone yet.\n",
    );
    Some((s, drafts.len()))
}

/// Render the drafted `repo-overview.md`.
pub fn render_repo_overview(f: &RepoFacts) -> Option<String> {
    let has_any = f.readme.is_some()
        || f.cargo.is_some()
        || f.package_json.is_some()
        || f.scan.is_some()
        || !f.ci.is_empty();
    if !has_any {
        return None;
    }
    let mut s = String::new();
    s.push_str(HEADER);
    s.push_str(&format!(
        "\n\n# Repo Overview\n\n## Facts read from the repo {TAG}\n\n"
    ));

    let name = f
        .cargo
        .as_ref()
        .and_then(|c| c.name.clone().map(|n| (n, "`Cargo.toml` `[package] name`")))
        .or_else(|| {
            f.package_json
                .as_ref()
                .and_then(|p| p.name.clone().map(|n| (n, "`package.json` `name`")))
        });
    if let Some((n, src)) = name {
        s.push_str(&format!("- **Package name:** `{n}` (from {src})\n"));
    }
    if let Some(r) = &f.readme {
        if let Some(t) = &r.title {
            s.push_str(&format!("- **README title:** {t} (from `{}`)\n", r.path));
        }
    }
    if let Some(scan) = &f.scan {
        let langs: Vec<String> = scan
            .languages
            .iter()
            .map(|(l, n)| {
                if l == "Unknown" {
                    format!("no language detected ({n})")
                } else {
                    format!("{l} ({n})")
                }
            })
            .collect();
        s.push_str(&format!(
            "- **Files by language:** {} (from scan `{}`)\n",
            langs.join(", "),
            scan.pack_id
        ));
        s.push_str(&format!(
            "- **Scan totals:** {} included files; {} (from scan `{}`)\n",
            scan.included_files,
            tests_phrase(scan.test_file_count, scan.source_file_count),
            scan.pack_id
        ));
    } else {
        s.push_str(
            "- **Scan:** none found. Run `repolens scan --root .`, then `repolens init` again to \
             add scan facts.\n",
        );
    }

    s.push_str(&format!("\n## Description {TAG}\n\n"));
    match f
        .readme
        .as_ref()
        .and_then(|r| r.first_paragraph.as_ref().map(|p| (r, p)))
    {
        Some((r, p)) => {
            s.push_str(&format!(
                "First prose paragraph of `{}`, quoted verbatim:\n\n> {p}\n",
                r.path
            ));
        }
        None => s.push_str("No description found in README.md.\n"),
    }

    if !f.top_dirs.is_empty() {
        s.push_str(&format!("\n## Top-level directories {TAG}\n\n"));
        for d in &f.top_dirs {
            s.push_str(&format!("- `{d}/`\n"));
        }
    }

    let has_build = !f.ci.is_empty()
        || f.package_json
            .as_ref()
            .is_some_and(|p| !p.scripts.is_empty());
    if has_build {
        s.push_str(&format!("\n## Build & test commands {TAG}\n\n"));
        for c in &f.ci {
            let locs: Vec<String> = std::iter::once(format!("{}:{}", c.file, c.line))
                .chain(c.also_in.iter().cloned())
                .map(|l| format!("`{l}`"))
                .collect();
            let extra = format!(
                "{}{}{}",
                c.args_phrase(),
                c.where_phrase(),
                c.condition_phrase()
            );
            s.push_str(&format!(
                "- `{}`{extra} (from {})\n",
                c.command,
                locs.join(", ")
            ));
        }
        if let Some(p) = &f.package_json {
            for (k, v) in &p.scripts {
                s.push_str(&format!(
                    "- `npm run {k}` runs `{v}` (from `package.json` `scripts`)\n"
                ));
            }
        }
    }

    if let Some(scan) = &f.scan {
        if !scan.entry_points.is_empty() {
            s.push_str(&format!("\n## Entry points detected by scan {TAG}\n\n"));
            for (p, t) in &scan.entry_points {
                s.push_str(&format!("- `{p}` ({t})\n"));
            }
        }
    }

    if !f.convention_files.is_empty() {
        s.push_str(&format!("\n## Convention files present {TAG}\n\n"));
        for c in &f.convention_files {
            s.push_str(&format!("- `{c}`\n"));
        }
    }

    s.push_str("\n## Notes from humans\n\nEmpty. Add anything every agent should know.\n");
    Some(s)
}

/// "1 of 21 files has tests" / "3 of 21 files have tests".
pub fn tests_phrase(tests: i32, sources: i32) -> String {
    let verb = if tests == 1 { "has" } else { "have" };
    format!("{tests} of {sources} code files {verb} tests")
}

/// F-TEST-INVENTORY-RATIO line, shared by `init` and `findings` so both
/// say the same thing. The caveat sits on the line: it counts files, it
/// does not measure coverage.
pub fn test_ratio_line(tests: u64, sources: u64) -> String {
    format!(
        "{} (file count)",
        tests_phrase(tests as i32, sources as i32)
    )
}

/// F-ORPHAN-FILES line, shared by `init` and `findings`: a heuristic, so
/// "may", and the caveat says only imports (plus script tags / CI) are read.
pub const ORPHAN_LINE: &str = "Some files may be unused (imports only)";

/// Hint under the findings list, shared by `init` and `findings`.
pub const FINDINGS_HINT: &str = "Findings are heuristics; each one lists what it doesn't check.";

/// True when `current` is still the untouched shipped template (or empty).
pub fn is_untouched(current: &str, template: &str) -> bool {
    current == template || current.trim().is_empty()
}

/// Record of what `init` last drafted: file name -> sha256 of the drafted
/// bytes. Lets a later `init` refresh a draft the user has not touched,
/// while leaving any edited file alone.
pub const LEDGER: &str = ".autodraft.json";

pub fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub fn load_ledger(storage_dir: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(storage_dir.join(LEDGER))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_ledger(storage_dir: &Path, ledger: &BTreeMap<String, String>) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(ledger).map_err(std::io::Error::other)?;
    repolens_core::atomic_write::write_atomic(
        &storage_dir.join(LEDGER),
        format!("{json}\n").as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_command_filter() {
        assert!(is_check_command("cargo test --release --locked"));
        assert!(is_check_command("cargo clippy --workspace -- -D warnings"));
        assert!(is_check_command("npm run build"));
        assert!(!is_check_command("cargo install wasm-bindgen-cli"));
        assert!(!is_check_command("npm ci"));
        assert!(!is_check_command("echo hi"));
    }

    /// #13: build/test commands from Maven, Gradle, Jekyll, unittest,
    /// cargo bench and in-repo check scripts are recognised.
    #[test]
    fn check_command_filter_covers_more_ecosystems() {
        for c in [
            // gson
            "mvn verify javadoc:jar",
            "mvn clean test --projects gson --activate-profiles gson-subset",
            "./mvnw -B package",
            "./gradlew build",
            "gradle check",
            // frankxue831.github.io
            "bundle exec jekyll doctor",
            "bundle exec jekyll build",
            "ruby scripts/validate_site.rb",
            "ruby scripts/check_images.rb",
            "bundle exec rake test",
            // ManimSimpleRender
            "python -m unittest discover -s tests",
            "python3 -m unittest discover -s tests/e2e -t .",
            "black --check app explainer tests main.py",
            "flake8 app explainer tests main.py",
            "python -m pytest -q",
            // cobra: env assignments before the command
            "RICHGO_FORCE_COLOR=1 PATH=$HOME/go/bin/:$PATH make richtest",
            // gm-crypto-rs dudect / fuzz
            "cargo bench --bench timing_leaks --features \"$FEATURES\"",
            "cargo +nightly fuzz build",
            "cargo +1.85 test --workspace",
            "python3 .github/scripts/check_disclosure_boundary.py --self-test",
            "bash scripts/test.sh",
            "node scripts/lint-docs.mjs",
        ] {
            assert!(is_check_command(c), "{c}");
        }
        for c in [
            "mvn org.codehaus.mojo:versions-maven-plugin:2.16.2:set \"-DnewVersion=0\"",
            "mvn artifact:check-buildplan",
            "bundle install",
            "bundle exec jekyll serve",
            "ruby scripts/deploy.rb",
            "python3 .github/scripts/native_gfni.py --output x",
            "python -m pip install --upgrade pip",
            "python3 - <<'PY'",
            "cargo fuzz --version",
            "cargo +nightly install cargo-fuzz",
            "FOO=1",
            "node run.cjs",
        ] {
            assert!(!is_check_command(c), "{c}");
        }
    }

    #[test]
    fn commands_drop_pipes_and_trailing_run_values() {
        let env = BTreeMap::new();
        // dudect: output is piped to a log; the command is the part before.
        assert_eq!(
            ci_command_text("cargo bench --bench t 2>&1 | tee \"d-$i.log\"", &env),
            Some(("cargo bench --bench t".to_string(), false))
        );
        // gson: a matrix value at the end is cut and flagged.
        assert_eq!(
            ci_command_text(
                "mvn verify javadoc:jar ${{ matrix.extra-mvn-args || '' }}",
                &env
            ),
            Some(("mvn verify javadoc:jar".to_string(), true))
        );
        // A flag left without its value is cut with it.
        assert_eq!(
            ci_command_text("cargo build --release --target ${{ matrix.target }}", &env),
            Some(("cargo build --release".to_string(), true))
        );
        // QA B1: a standalone flag before a run-time argument list stays.
        assert_eq!(
            ci_command_text(
                "mvn test --projects test-graal-native-image --also-make ${{ matrix.extra-mvn-args || '' }}",
                &env
            ),
            Some((
                "mvn test --projects test-graal-native-image --also-make".to_string(),
                true
            ))
        );
        assert_eq!(
            ci_command_text("cargo test --release ${{ matrix.flags }}", &env),
            Some(("cargo test --release".to_string(), true))
        );
        // A short flag whose value the expression names goes too.
        assert_eq!(
            ci_command_text("cargo test -p ${{ matrix.package }}", &env),
            Some(("cargo test".to_string(), true))
        );
        // A value in the middle can't be cut away: skip the command.
        assert_eq!(ci_command_text("${{ env.CARGO }} test", &env), None);
        assert_eq!(
            ci_command_text("cargo test -p ${{ matrix.p }} --locked", &env),
            None
        );
        assert_eq!(
            ci_command_text("cargo test --workspace", &env),
            Some(("cargo test --workspace".to_string(), false))
        );
    }

    /// QA B2: `working-directory` on the step (before or after `run:`) and
    /// in job or workflow `defaults.run` is kept and shown.
    #[test]
    fn working_directory_is_kept_and_shown() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".github/workflows");
        fs::create_dir_all(&wf).unwrap();
        fs::write(
            wf.join("ci.yml"),
            "name: CI\n\
             jobs:\n\
             \x20 live:\n\
             \x20   steps:\n\
             \x20     - name: Live check\n\
             \x20       working-directory: pw\n\
             \x20       run: node live-check.cjs\n\
             \x20     - name: Verify fuzz targets build\n\
             \x20       run: cargo check\n\
             \x20       working-directory: ./fuzz\n\
             \x20     - run: cargo test\n\
             \x20 site:\n\
             \x20   defaults:\n\
             \x20     run:\n\
             \x20       working-directory: site\n\
             \x20   steps:\n\
             \x20     - run: bundle exec jekyll build\n\
             \x20     - run: cargo test\n\
             \x20       working-directory: other\n",
        )
        .unwrap();
        let facts = gather(dir.path(), None);
        let got: Vec<(&str, Option<&str>)> = facts
            .ci
            .iter()
            .map(|c| (c.command.as_str(), c.working_dir.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("node live-check.cjs", Some("pw")),
                ("cargo check", Some("fuzz")),
                ("cargo test", None),
                ("bundle exec jekyll build", Some("site")),
                ("cargo test", Some("other")),
            ]
        );
        let overview = render_repo_overview(&facts).unwrap();
        assert!(
            overview
                .contains("- `node live-check.cjs` in `pw/` (from `.github/workflows/ci.yml:7`)"),
            "{overview}"
        );
        let (kc, _) = render_known_constraints(&facts).unwrap();
        assert!(
            kc.contains("step \"Verify fuzz targets build\" runs `cargo check` in `fuzz/`."),
            "{kc}"
        );
    }

    #[test]
    fn ci_tool_is_the_command_word() {
        assert_eq!(ci_tool("cargo +nightly fuzz build"), "cargo");
        assert_eq!(ci_tool("RUSTFLAGS=-Dwarnings cargo test"), "cargo");
        assert_eq!(ci_tool("sudo make install-check"), "make");
        assert_eq!(ci_tool("bundle exec jekyll build"), "jekyll");
        assert_eq!(ci_tool("/usr/bin/make test"), "make");
        assert_eq!(ci_tool("./scripts/ci.sh --all"), "./scripts/ci.sh");
        assert_eq!(ci_tool("python3 -m pytest"), "python3");
    }

    fn ci_facts(cmds: &[&str]) -> RepoFacts {
        RepoFacts {
            cargo: Some(CargoFacts {
                edition: Some("2021".into()),
                ..Default::default()
            }),
            ci: cmds
                .iter()
                .enumerate()
                .map(|(i, c)| CiCommand {
                    file: ".github/workflows/ci.yml".into(),
                    line: i + 1,
                    workflow: None,
                    step: None,
                    command: (*c).into(),
                    also_in: vec![],
                    per_run_args: false,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    /// #45: the drafted constraints open with one line per CI tool and a
    /// count; the D-NNN ids and the full list below are unchanged.
    #[test]
    fn ci_drafts_are_summed_up_per_tool_above_the_full_list() {
        let facts = ci_facts(&[
            "cargo test",
            "cargo clippy",
            "npm test",
            "cargo +nightly fuzz build",
            "make check",
            "npm run lint",
            "cargo fmt --check",
        ]);
        let (kc, n) = render_known_constraints(&facts).unwrap();
        assert_eq!(n, 8);
        let summary: Vec<&str> = kc
            .lines()
            .filter(|l| l.starts_with("- ") && !l.starts_with("- **"))
            .collect();
        assert_eq!(
            summary,
            vec![
                "- cargo: 4 drafts (D-002\u{2013}D-003, D-005, D-008)",
                "- npm: 2 drafts (D-004, D-007)",
                "- make: 1 draft (D-006)",
            ]
        );
        // The full list keeps the ids it had without the summary.
        let headings: Vec<&str> = kc.lines().filter(|l| l.starts_with("### D-")).collect();
        assert_eq!(headings.len(), 8);
        assert!(headings[0].starts_with("### D-001 Rust edition 2021"));
        assert!(headings[1].starts_with("### D-002 CI runs `cargo test`"));
        assert!(headings[3].starts_with("### D-004 CI runs `npm test`"));
        assert!(headings[7].starts_with("### D-008 CI runs `cargo fmt --check`"));
        // The summary sits above the first draft.
        let at = |needle: &str| kc.find(needle).unwrap();
        assert!(at("- cargo: 4 drafts") < at("### D-001"));
    }

    /// #45 QA: 30 commands alternating between two tools give each tool
    /// 15 single ids; the line lists ids up to 80 columns and counts the
    /// rest instead of running on.
    #[test]
    fn ci_summary_line_stops_at_80_columns() {
        let cmds: Vec<String> = (0..30)
            .map(|i| {
                if i % 2 == 0 {
                    format!("pytest tests/test_{i}.py")
                } else {
                    format!("cargo test -p crate{i}")
                }
            })
            .collect();
        let refs: Vec<&str> = cmds.iter().map(String::as_str).collect();
        let (kc, n) = render_known_constraints(&ci_facts(&refs)).unwrap();
        assert_eq!(n, 31);
        let summary: Vec<&str> = kc
            .lines()
            .filter(|l| l.starts_with("- ") && !l.starts_with("- **"))
            .collect();
        assert_eq!(summary.len(), 2);
        for line in &summary {
            assert!(
                summary_cols(line) <= 80,
                "{} cols: {line}",
                summary_cols(line)
            );
            let (listed, rest) = line.split_once(" \u{2026} and ").expect(line);
            let more: usize = rest.trim_end_matches(" more)").parse().unwrap();
            let shown = listed.split_once('(').unwrap().1.split(", ").count();
            assert_eq!(shown + more, 15, "{line}");
        }
        assert!(summary[0].starts_with("- pytest: 15 drafts (D-002, D-004, D-006, "));
        assert!(summary[1].starts_with("- cargo: 15 drafts (D-003, D-005, D-007, "));
        // A line that fits is left whole.
        assert_eq!(
            ci_summary_line("make", &[3, 4, 5, 9]),
            "- make: 4 drafts (D-003\u{2013}D-005, D-009)"
        );
    }

    #[test]
    fn no_ci_drafts_means_no_summary() {
        let facts = ci_facts(&[]);
        let (kc, _) = render_known_constraints(&facts).unwrap();
        assert!(!kc.contains("CI commands by tool"), "{kc}");
    }

    /// #17: a `run: |` line ending in a backslash continues on the next
    /// line; the drafted command is the joined line, never one ending in
    /// a literal backslash.
    #[test]
    fn backslash_continuations_in_run_blocks_are_joined() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".github/workflows");
        fs::create_dir_all(&wf).unwrap();
        fs::write(
            wf.join("ci.yml"),
            "name: CI\njobs:\n  t:\n    steps:\n      - name: Interop\n        run: |\n          \
             cargo test -p core --test interop --features aead \\\n            \
             -- --nocapture 2>&1 | tee interop.txt\n          cargo build \\\n            --release \\\n            \
             --locked\n          cargo clippy\n",
        )
        .unwrap();
        let facts = gather(dir.path(), None);
        let cmds: Vec<(&str, usize)> = facts
            .ci
            .iter()
            .map(|c| (c.command.as_str(), c.line))
            .collect();
        assert_eq!(
            cmds,
            vec![
                (
                    "cargo test -p core --test interop --features aead -- --nocapture",
                    7
                ),
                ("cargo build --release --locked", 9),
                ("cargo clippy", 12),
            ]
        );
        assert!(facts.ci.iter().all(|c| !c.command.ends_with('\\')));
    }

    #[test]
    fn every_workflow_file_is_recorded_and_matrix_commands_kept() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".github/workflows");
        fs::create_dir_all(&wf).unwrap();
        fs::write(
            wf.join("build.yml"),
            "name: Build\njobs:\n  b:\n    steps:\n      - name: Build with Maven\n        \
             run: mvn verify javadoc:jar ${{ matrix.extra-mvn-args || '' }}\n      \
             - run: mvn clean test --projects gson\n",
        )
        .unwrap();
        fs::write(wf.join("scorecard.yml"), "name: Scorecard\njobs: {}\n").unwrap();
        let facts = gather(dir.path(), None);
        assert_eq!(
            facts.ci_workflows,
            vec![
                ".github/workflows/build.yml",
                ".github/workflows/scorecard.yml"
            ]
        );
        let cmds: Vec<(&str, bool)> = facts
            .ci
            .iter()
            .map(|c| (c.command.as_str(), c.per_run_args))
            .collect();
        assert_eq!(
            cmds,
            vec![
                ("mvn verify javadoc:jar", true),
                ("mvn clean test --projects gson", false)
            ]
        );
        let overview = render_repo_overview(&facts).unwrap();
        assert!(
            overview.contains("- `mvn verify javadoc:jar` plus arguments set per run (from `.github/workflows/build.yml:6`)"),
            "{overview}"
        );
    }

    /// #13 follow-ups from QA: heredoc bodies, quoted env prefixes, inline
    /// comments, folded blocks, unspaced pipes and redirects, and step
    /// conditions.
    #[test]
    fn ci_steps_are_drafted_as_they_run() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".github/workflows");
        fs::create_dir_all(&wf).unwrap();
        fs::write(
            wf.join("ci.yml"),
            "name: CI\n\
             jobs:\n\
             \x20 t:\n\
             \x20   steps:\n\
             \x20     - name: Notes\n\
             \x20       run: |\n\
             \x20         cat <<'EOF' > notes.md\n\
             \x20         make release-notes\n\
             \x20         EOF\n\
             \x20         cargo fmt --check # style\n\
             \x20     - name: Strict build\n\
             \x20       run: RUSTFLAGS=\"-D warnings\" cargo build --locked\n\
             \x20     - name: Folded\n\
             \x20       run: >\n\
             \x20         cargo test\n\
             \x20         --workspace\n\
             \x20         --all-features\n\
             \n\
             \x20         cargo doc --no-deps\n\
             \x20     - name: Logged\n\
             \x20       run: |\n\
             \x20         cargo clippy --all-targets 2>&1|tee clippy.log\n\
             \x20         cargo bench --no-run > out.log\n\
             \x20         cargo check >>check.log 2>&1\n\
             \x20     - name: Java only\n\
             \x20       if: ${{ matrix.language == 'java' }}\n\
             \x20       run: mvn compile\n\
             \x20     - name: PR only\n\
             \x20       run: python3 scripts/check_x.py --range \"$a..$b\"\n\
             \x20       if: github.event_name == 'pull_request' # PRs\n",
        )
        .unwrap();
        let facts = gather(dir.path(), None);
        let got: Vec<(&str, Option<&str>)> = facts
            .ci
            .iter()
            .map(|c| (c.command.as_str(), c.condition.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("cargo fmt --check", None),
                ("RUSTFLAGS=\"-D warnings\" cargo build --locked", None),
                ("cargo test --workspace --all-features", None),
                ("cargo doc --no-deps", None),
                ("cargo clippy --all-targets", None),
                ("cargo bench --no-run", None),
                ("cargo check", None),
                ("mvn compile", Some("matrix.language == 'java'")),
                (
                    "python3 scripts/check_x.py --range \"$a..$b\"",
                    Some("github.event_name == 'pull_request'")
                ),
            ]
        );
        let lines: Vec<usize> = facts.ci.iter().map(|c| c.line).collect();
        assert_eq!(lines, vec![10, 12, 15, 19, 22, 23, 24, 27, 29]);
        let (kc, _) = render_known_constraints(&facts).unwrap();
        assert!(
            kc.contains(
                "step \"Java only\" runs `mvn compile` (only when `matrix.language == 'java'`)."
            ),
            "{kc}"
        );
        let overview = render_repo_overview(&facts).unwrap();
        assert!(
            overview.contains("- `mvn compile` (only when `matrix.language == 'java'`) (from"),
            "{overview}"
        );
    }

    /// #13: `if: always()` and `if: failure()` are said in words; any
    /// other condition is quoted as written, in parentheses.
    #[test]
    fn always_and_failure_conditions_are_said_in_words() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".github/workflows");
        fs::create_dir_all(&wf).unwrap();
        fs::write(
            wf.join("ci.yml"),
            "name: CI\njobs:\n  t:\n    steps:\n      \
             - name: Upload\n        if: ${{ always() }}\n        run: cargo test --no-run\n      \
             - name: Logs\n        if: failure()\n        run: make logs-check\n      \
             - name: Not\n        if: \"!cancelled()\"\n        run: cargo check\n      \
             - name: Both\n        if: always() && github.ref == 'refs/heads/main'\n        \
             run: cargo build\n",
        )
        .unwrap();
        let facts = gather(dir.path(), None);
        let overview = render_repo_overview(&facts).unwrap();
        let want = [
            "- `cargo test --no-run` (runs even if earlier steps fail) (from",
            "- `make logs-check` (runs only if an earlier step failed) (from",
            "- `cargo check` (only when `!cancelled()`) (from",
            "- `cargo build` (only when `always() && github.ref == 'refs/heads/main'`) (from",
        ];
        for w in want {
            assert!(overview.contains(w), "missing {w}\n{overview}");
        }
        let (kc, _) = render_known_constraints(&facts).unwrap();
        assert!(
            kc.contains(
                "step \"Upload\" runs `cargo test --no-run` (runs even if earlier steps fail)."
            ),
            "{kc}"
        );
        assert!(
            kc.contains(
                "step \"Logs\" runs `make logs-check` (runs only if an earlier step failed)."
            ),
            "{kc}"
        );
        assert!(
            kc.contains("step \"Not\" runs `cargo check` (only when `!cancelled()`)."),
            "{kc}"
        );
        // The worded part fits 80 columns; only the `(from file:line)`
        // citation, as on any line, can run past it.
        for l in overview.lines().filter(|l| l.contains("(runs ")) {
            let said = l.split(" (from ").next().unwrap();
            assert!(crate::findings::cols(said) <= 80, "too wide: {said}");
        }
    }

    /// #13 QA B1b, B2a, B2b.
    #[test]
    fn per_run_flags_and_folders_are_described() {
        let env = BTreeMap::new();
        // B1b: the flag goes with its per-run value even when the names
        // don't match.
        assert_eq!(
            ci_command_text("cargo test --features ${{ matrix.f }}", &env),
            Some(("cargo test".to_string(), true))
        );
        assert_eq!(
            ci_command_text("cargo test --release ${{ matrix.f }}", &env),
            Some(("cargo test --release".to_string(), true))
        );
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".github/workflows");
        fs::create_dir_all(&wf).unwrap();
        fs::write(
            wf.join("ci.yml"),
            "name: CI\n\
             jobs:\n\
             \x20 t:\n\
             \x20   defaults:\n\
             \x20     run:\n\
             \x20       working-directory: site\n\
             \x20   steps:\n\
             \x20     - run: cargo test\n\
             \x20       working-directory: ${{ matrix.dir }}\n\
             \x20     - run: cargo build\n\
             \x20       working-directory: .\n\
             \x20     - run: cargo check\n",
        )
        .unwrap();
        let facts = gather(dir.path(), None);
        let got: Vec<(&str, Option<&str>, bool)> = facts
            .ci
            .iter()
            .map(|c| {
                (
                    c.command.as_str(),
                    c.working_dir.as_deref(),
                    c.working_dir_per_run,
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("cargo test", None, true),
                ("cargo build", None, false),
                ("cargo check", Some("site"), false),
            ]
        );
        let (kc, _) = render_known_constraints(&facts).unwrap();
        assert!(
            kc.contains("### D-001 CI runs `cargo test` in a folder set per run"),
            "{kc}"
        );
        assert!(
            kc.contains("### D-002 CI runs `cargo build` (auto-drafted)"),
            "{kc}"
        );
    }

    #[test]
    fn shell_line_helpers() {
        assert_eq!(
            shell_words("A=\"x y\" cargo  build '--a b'"),
            vec!["A=\"x y\"", "cargo", "build", "'--a b'"]
        );
        assert_eq!(strip_comment_and_pipe("make a|tee x"), "make a");
        assert_eq!(strip_comment_and_pipe("make a || true"), "make a || true");
        assert_eq!(
            strip_comment_and_pipe("echo \"#1 | x\" # c"),
            "echo \"#1 | x\""
        );
        assert_eq!(
            strip_comment_and_pipe("mvn ${{ a || '' }}"),
            "mvn ${{ a || '' }}"
        );
        assert_eq!(drop_redirects("make a > b.log 2>&1"), "make a");
        assert_eq!(drop_redirects("make a &>/dev/null"), "make a");
        assert_eq!(drop_redirects("echo \"a > b\""), "echo \"a > b\"");
        assert_eq!(heredoc_delimiter("cat <<-'END' > x"), Some("END".into()));
        assert_eq!(heredoc_delimiter("cat << \"EOF\""), Some("EOF".into()));
        assert_eq!(heredoc_delimiter("grep x <<< \"$y\""), None);
    }

    #[test]
    fn toml_reader_handles_workspace_package() {
        let s = toml_sections(
            "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nedition = \"2021\" # c\nrust-version = \"1.88\"\n",
        );
        assert_eq!(s["workspace.package"]["rust-version"], "1.88");
        assert_eq!(s["workspace.package"]["edition"], "2021");
    }

    #[test]
    fn ci_expressions_are_expanded_or_skipped() {
        let wf = "name: CI\nenv:\n  CARGO_TERM_COLOR: always\n  FLAGS: --locked\n  EMPTY:\njobs:\n  t:\n    env:\n      CARGO: cargo\n    steps:\n      - run: echo \"CARGO=cross\" >> $GITHUB_ENV\n";
        let lines: Vec<&str> = wf.lines().collect();
        let env = static_workflow_env(&lines);
        assert_eq!(env.get("FLAGS").map(String::as_str), Some("--locked"));
        assert_eq!(env.get("EMPTY").map(String::as_str), Some(""));
        assert!(!env.contains_key("CARGO"), "rewritten at run time");
        assert_eq!(
            expand_expressions("cargo test ${{ env.FLAGS }} ${{ env.EMPTY }}", &env).as_deref(),
            Some("cargo test --locked")
        );
        assert_eq!(expand_expressions("${{ env.CARGO }} test", &env), None);
        assert_eq!(
            expand_expressions("cargo build --target ${{ matrix.target }}", &env),
            None
        );
        assert_eq!(
            expand_expressions("cargo test", &env).as_deref(),
            Some("cargo test")
        );
    }

    #[test]
    fn untouched_detection() {
        assert!(is_untouched("T", "T"));
        assert!(is_untouched("  \n", "T"));
        assert!(!is_untouched("T edited", "T"));
    }
}
