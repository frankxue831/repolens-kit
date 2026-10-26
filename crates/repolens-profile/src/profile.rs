use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::profile::{
    BuildSystem, EntryPoint, Framework, ModuleInfo, ProjectProfile, ProjectType,
};
use repolens_core::pack_reader::PackIndex;
use repolens_core::path_safety::resolve_existing_subpath;
use repolens_core::storage::RepoStoragePaths;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use tracing::info;

/// Analyzes a `PackIndex` to produce a `ProjectProfile`.
pub struct ProfileAnalyzer;

impl ProfileAnalyzer {
    /// Analyze the pack index and produce a `ProjectProfile`.
    pub fn analyze(index: &PackIndex, pack_dir: &Path) -> ProjectProfile {
        info!(
            pack_dir = %pack_dir.display(),
            entry_count = index.entries.len(),
            "ProfileAnalyzer::analyze starting"
        );

        let included: Vec<&FileEntry> = index.entries.iter().filter(|e| e.included).collect();

        let name = Self::detect_name(&included, pack_dir);
        let build_system = Self::detect_build_system(&included, pack_dir);
        let frameworks = Self::detect_frameworks(&included, pack_dir);
        let entry_points = Self::detect_entry_points(&included, pack_dir);
        let language_distribution = Self::compute_language_distribution(&included);
        let modules = Self::compute_modules(&included);
        let manifest_type = Self::manifest_project_type(&included, pack_dir);
        let project_type = Self::classify_project_type(&frameworks, &entry_points, manifest_type);

        info!(
            name = %name,
            project_type = ?project_type,
            framework_count = frameworks.len(),
            entry_point_count = entry_points.len(),
            "ProfileAnalyzer::analyze complete"
        );

        ProjectProfile {
            name,
            build_system,
            frameworks,
            project_type,
            entry_points,
            description: None,
            language_distribution,
            modules,
        }
    }

    fn detect_name(included: &[&FileEntry], pack_dir: &Path) -> String {
        // Try package.json "name" field
        for entry in included.iter() {
            if entry.path == "package.json" {
                if let Some(content) = Self::read_content(entry, pack_dir) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                            if !name.is_empty() {
                                return name.to_string();
                            }
                        }
                    }
                }
            }
        }

        // Cargo.toml precedence:
        //   1. Root [package] name (single-crate repo).
        //   2. If root is a [workspace] without [package], use the workspace
        //      directory name (the parent of `.repolens/`). This matches what
        //      `cargo` users reach for when describing the project — the repo
        //      directory, not an arbitrary member crate.
        //   3. Binary member crate ([package] with a sibling src/main.rs),
        //      then library member crate — alphabetical within each group.
        //      Binary crates are preferred because they're what a workspace
        //      actually produces; alphabetical order picks an arbitrary library.
        let root_cargo = included
            .iter()
            .find(|e| e.path == "Cargo.toml")
            .and_then(|e| Self::read_content(e, pack_dir));
        if let Some(content) = root_cargo.as_deref() {
            if let Some(name) = parse_cargo_package_name(content) {
                return name;
            }
            if cargo_toml_is_workspace_only(content) {
                if let Some(name) = workspace_dir_name_from_pack_dir(pack_dir) {
                    return name;
                }
            }
        }

        let bin_crate_dirs: std::collections::HashSet<&str> = included
            .iter()
            .filter_map(|e| {
                if e.path == "src/main.rs" {
                    Some("")
                } else {
                    e.path.strip_suffix("/src/main.rs")
                }
            })
            .collect();
        let mut member_cargo_entries: Vec<&&FileEntry> = included
            .iter()
            .filter(|e| e.path != "Cargo.toml" && e.path.ends_with("/Cargo.toml"))
            .collect();
        member_cargo_entries.sort_by_key(|e| {
            let parent = e.path.strip_suffix("/Cargo.toml").unwrap_or("");
            let is_bin_crate = bin_crate_dirs.contains(parent);
            (!is_bin_crate, e.path.clone())
        });
        for entry in member_cargo_entries {
            if let Some(content) = Self::read_content(entry, pack_dir) {
                if let Some(name) = parse_cargo_package_name(&content) {
                    return name;
                }
            }
        }

        if Self::detect_build_system(included, pack_dir).is_some() {
            if let Some(name) = workspace_dir_name_from_pack_dir(pack_dir) {
                return name;
            }
        }

        "unknown".to_string()
    }

    fn detect_build_system(included: &[&FileEntry], pack_dir: &Path) -> Option<BuildSystem> {
        for entry in included.iter() {
            match entry.path.as_str() {
                "pom.xml" => {
                    return Some(BuildSystem {
                        name: "Maven".to_string(),
                        details: None,
                        config_file: "pom.xml".to_string(),
                    })
                }
                "build.gradle" | "build.gradle.kts" => {
                    return Some(BuildSystem {
                        name: "Gradle".to_string(),
                        details: None,
                        config_file: entry.path.clone(),
                    })
                }
                "package.json" => {
                    return Some(BuildSystem {
                        name: "npm".to_string(),
                        details: None,
                        config_file: "package.json".to_string(),
                    })
                }
                "Cargo.toml" => {
                    return Some(BuildSystem {
                        name: "Cargo".to_string(),
                        details: None,
                        config_file: "Cargo.toml".to_string(),
                    })
                }
                "go.mod" => {
                    return Some(BuildSystem {
                        name: "Go".to_string(),
                        details: None,
                        config_file: "go.mod".to_string(),
                    })
                }
                "pyproject.toml" | "setup.py" | "Pipfile" => {
                    return Some(BuildSystem {
                        name: "Python".to_string(),
                        details: Self::python_lock_tool(included, pack_dir),
                        config_file: entry.path.clone(),
                    })
                }
                _ => {}
            }
        }
        None
    }

    fn detect_frameworks(included: &[&FileEntry], pack_dir: &Path) -> Vec<Framework> {
        let mut frameworks: Vec<Framework> = Vec::new();

        for entry in included.iter() {
            match entry.path.as_str() {
                "package.json" => {
                    if let Some(content) = Self::read_content(entry, pack_dir) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            // Check dependencies and devDependencies
                            let dep_keys = ["dependencies", "devDependencies"];
                            for dep_key in &dep_keys {
                                if let Some(deps) = json.get(dep_key).and_then(|v| v.as_object()) {
                                    for (dep_name, dep_version) in deps {
                                        let version =
                                            dep_version.as_str().unwrap_or("").to_string();
                                        match dep_name.as_str() {
                                            "react"
                                                if !frameworks
                                                    .iter()
                                                    .any(|f: &Framework| f.name == "React") =>
                                            {
                                                frameworks.push(Framework {
                                                    name: "React".to_string(),
                                                    version,
                                                    category: "frontend".to_string(),
                                                });
                                            }
                                            "express"
                                                if !frameworks
                                                    .iter()
                                                    .any(|f: &Framework| f.name == "Express") =>
                                            {
                                                frameworks.push(Framework {
                                                    name: "Express".to_string(),
                                                    version,
                                                    category: "backend".to_string(),
                                                });
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                "pom.xml" => {
                    if let Some(content) = Self::read_content(entry, pack_dir) {
                        if content.contains("spring-boot") {
                            if !frameworks
                                .iter()
                                .any(|f: &Framework| f.name == "Spring Boot")
                            {
                                frameworks.push(Framework {
                                    name: "Spring Boot".to_string(),
                                    version: "".to_string(),
                                    category: "backend".to_string(),
                                });
                            }
                        } else if content.contains("org.springframework")
                            && !frameworks.iter().any(|f: &Framework| f.name == "Spring")
                        {
                            frameworks.push(Framework {
                                name: "Spring".to_string(),
                                version: "".to_string(),
                                category: "backend".to_string(),
                            });
                        }
                    }
                }
                _ => {
                    if entry.path.ends_with(".py") {
                        let has_django = frameworks.iter().any(|f: &Framework| f.name == "Django");
                        let has_flask = frameworks.iter().any(|f: &Framework| f.name == "Flask");
                        if has_django && has_flask {
                            continue;
                        }
                        if let Some(content) = Self::read_content(entry, pack_dir) {
                            if !has_django
                                && (content.contains("from django")
                                    || content.contains("import django"))
                            {
                                frameworks.push(Framework {
                                    name: "Django".to_string(),
                                    version: "".to_string(),
                                    category: "backend".to_string(),
                                });
                            }
                            if !has_flask
                                && (content.contains("from flask")
                                    || content.contains("import flask"))
                            {
                                frameworks.push(Framework {
                                    name: "Flask".to_string(),
                                    version: "".to_string(),
                                    category: "backend".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        frameworks
    }

    fn detect_entry_points(included: &[&FileEntry], pack_dir: &Path) -> Vec<EntryPoint> {
        let mut entry_points: Vec<EntryPoint> = Vec::new();
        let mut seen_paths: HashSet<String> = HashSet::new();
        let rust_bin_targets = collect_rust_bin_targets(included, pack_dir);

        for entry in included.iter() {
            let path = &entry.path;
            if !is_runtime_entry_point_candidate(path) {
                continue;
            }

            // Java: files with "public static void main" in content
            if entry.language == "Java" {
                if let Some(content) = Self::read_content(entry, pack_dir) {
                    if content.contains("public static void main") {
                        entry_points.push(EntryPoint {
                            entry_type: "main".to_string(),
                            path: path.clone(),
                            description: "Java main method".to_string(),
                        });
                    }
                }
            }

            // Python: __main__.py or files with "if __name__"
            if entry.language == "Python" {
                let file_name = Path::new(path.as_str())
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if file_name == "__main__.py" {
                    entry_points.push(EntryPoint {
                        entry_type: "main".to_string(),
                        path: path.clone(),
                        description: "Python __main__ module".to_string(),
                    });
                } else if let Some(content) = Self::read_content(entry, pack_dir) {
                    if content.contains("if __name__") {
                        entry_points.push(EntryPoint {
                            entry_type: "main".to_string(),
                            path: path.clone(),
                            description: "Python entry point".to_string(),
                        });
                    }
                }
            }

            // Rust: Cargo binary targets — src/main.rs at any crate root
            if entry.language == "Rust" {
                let is_main_rs = path == "src/main.rs" || path.ends_with("/src/main.rs");
                let is_src_bin_rs = is_rust_src_bin(path);
                let is_explicit_bin = rust_bin_targets.contains(path);
                if is_main_rs || is_src_bin_rs || is_explicit_bin {
                    if !seen_paths.insert(path.clone()) {
                        continue;
                    }
                    let description = if is_explicit_bin {
                        "Rust binary ([[bin]] target)"
                    } else if is_src_bin_rs {
                        "Rust binary (src/bin/*)"
                    } else {
                        "Rust binary (src/main.rs)"
                    };
                    entry_points.push(EntryPoint {
                        entry_type: "main".to_string(),
                        path: path.clone(),
                        description: description.to_string(),
                    });
                }
            }

            // Go: files with "func main()"
            if entry.language == "Go" {
                if let Some(content) = Self::read_content(entry, pack_dir) {
                    if content.contains("func main()") {
                        entry_points.push(EntryPoint {
                            entry_type: "main".to_string(),
                            path: path.clone(),
                            description: "Go main function".to_string(),
                        });
                    }
                }
            }

            // JS/TS: root-level or src/ index.js/ts, main.js/ts, app.js/ts (depth <= 1)
            let is_js_ts = entry.language == "JavaScript"
                || entry.language == "TypeScript"
                || entry.language == "JSX"
                || entry.language == "TSX";
            if is_js_ts {
                let file_name = Path::new(path.as_str())
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let depth = path.chars().filter(|&c| c == '/').count();
                let is_entry_file = matches!(
                    file_name,
                    "index.js"
                        | "index.ts"
                        | "index.jsx"
                        | "index.tsx"
                        | "main.js"
                        | "main.ts"
                        | "app.js"
                        | "app.ts"
                );
                if is_entry_file && depth <= 1 {
                    entry_points.push(EntryPoint {
                        entry_type: "main".to_string(),
                        path: path.clone(),
                        description: "JS/TS entry point".to_string(),
                    });
                }
            }
        }

        entry_points
    }

    fn compute_language_distribution(included: &[&FileEntry]) -> BTreeMap<String, i32> {
        let mut dist: BTreeMap<String, i32> = BTreeMap::new();
        for entry in included.iter() {
            if entry.language.is_empty() || entry.language == "Unknown" {
                continue;
            }
            *dist.entry(entry.language.clone()).or_insert(0) += entry.loc;
        }
        dist
    }

    fn compute_modules(included: &[&FileEntry]) -> Vec<ModuleInfo> {
        // Group by first path component; skip root files (no '/' in path)
        let mut module_map: HashMap<String, (i32, HashMap<String, i32>)> = HashMap::new();

        for entry in included.iter() {
            if let Some(slash_pos) = entry.path.find('/') {
                let dir = &entry.path[..slash_pos];
                if dir.starts_with('.') {
                    continue;
                }
                let module = module_map
                    .entry(dir.to_string())
                    .or_insert((0, HashMap::new()));
                module.0 += 1;
                *module.1.entry(entry.language.clone()).or_insert(0) += entry.loc;
            }
        }

        let mut modules: Vec<ModuleInfo> = module_map
            .into_iter()
            .map(|(name, (file_count, lang_locs))| {
                // Deterministic tie-break: most LOC wins; on an exact tie the
                // lexicographically-greater language name wins (e.g. TypeScript
                // over JavaScript), matching the captured Java contract. Plain
                // `max_by_key` over a HashMap resolved ties by (per-instance)
                // iteration order, making `primaryLanguage` nondeterministic
                // across scans of the same repo.
                let primary_language = lang_locs
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then_with(|| a.0.cmp(b.0)))
                    .map(|(lang, _)| lang.clone())
                    .unwrap_or_default();
                let total_loc: i32 = lang_locs.values().sum();
                ModuleInfo {
                    path: name.clone(),
                    name,
                    file_count,
                    loc: total_loc,
                    primary_language,
                }
            })
            .collect();

        modules.sort_by(|a, b| a.name.cmp(&b.name));
        modules
    }

    /// The Python tool that keeps a lockfile for this repo (`poetry`, `pdm`,
    /// `uv` or `pipenv`), read from root `[tool.*]` tables in
    /// `pyproject.toml` or a root `Pipfile`. `None` means plain
    /// pip/setuptools, which has no standard lockfile (#25).
    fn python_lock_tool(included: &[&FileEntry], pack_dir: &Path) -> Option<String> {
        if let Some(content) = included
            .iter()
            .find(|e| e.path == "pyproject.toml")
            .and_then(|e| Self::read_content(e, pack_dir))
        {
            let tables = toml_table_headers(&content);
            for tool in ["poetry", "pdm", "uv"] {
                let prefix = format!("tool.{tool}");
                if tables
                    .iter()
                    .any(|t| *t == prefix || t.starts_with(&format!("{prefix}.")))
                {
                    return Some(tool.to_string());
                }
            }
        }
        if included.iter().any(|e| e.path == "Pipfile") {
            return Some("pipenv".to_string());
        }
        None
    }

    /// What the root manifests say about the repo (#25): a Node `bin` or
    /// Python console scripts make it an app; Node `main`/`exports` with no
    /// `bin` make it a library. `None` when the manifests don't say.
    fn manifest_project_type(included: &[&FileEntry], pack_dir: &Path) -> Option<ProjectType> {
        let root = |name: &str| {
            included
                .iter()
                .find(|e| e.path == name)
                .and_then(|e| Self::read_content(e, pack_dir))
        };
        let python_scripts = root("pyproject.toml").is_some_and(|c| {
            toml_table_headers(&c).iter().any(|t| {
                matches!(
                    t.as_str(),
                    "project.scripts"
                        | "project.gui-scripts"
                        | "tool.poetry.scripts"
                        | "project.entry-points.console_scripts"
                        | "project.entry-points.gui_scripts"
                )
            })
        }) || ["setup.py", "setup.cfg"].iter().any(|m| {
            root(m).is_some_and(|c| c.contains("console_scripts") || c.contains("gui_scripts"))
        });
        if python_scripts {
            return Some(ProjectType::Cli);
        }
        let json = root("package.json")
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())?;
        let has_bin = match json.get("bin") {
            Some(serde_json::Value::String(s)) => !s.is_empty(),
            Some(serde_json::Value::Object(m)) => !m.is_empty(),
            _ => false,
        };
        if has_bin {
            Some(ProjectType::Cli)
        } else if json.get("main").is_some() || json.get("exports").is_some() {
            Some(ProjectType::Library)
        } else {
            None
        }
    }

    fn classify_project_type(
        frameworks: &[Framework],
        entry_points: &[EntryPoint],
        manifest_type: Option<ProjectType>,
    ) -> ProjectType {
        let has_web_framework = frameworks
            .iter()
            .any(|f| f.category == "frontend" || f.category == "backend");
        if has_web_framework {
            return ProjectType::WebApp;
        }
        if let Some(t) = manifest_type {
            return t;
        }
        if !entry_points.is_empty() {
            return ProjectType::Cli;
        }
        ProjectType::Library
    }

    fn read_content(entry: &FileEntry, pack_dir: &Path) -> Option<String> {
        let content_path = entry.content_path.as_deref()?;
        let full_path = resolve_existing_subpath(pack_dir, content_path).ok()?;
        std::fs::read_to_string(&full_path).ok()
    }
}

fn is_runtime_entry_point_candidate(path: &str) -> bool {
    // Only match at the top two path components so a real source file like
    // `crates/foo/src/fixtures/loader.rs` isn't suppressed — fixture-shaped
    // subtrees this repo scans (e.g. `fixtures/inputs/*`, `fixtures/test-data/*`)
    // always sit at depth 0 or 1.
    let leading: Vec<String> = Path::new(path)
        .components()
        .take(2)
        .filter_map(|c| c.as_os_str().to_str())
        .map(|s| s.to_ascii_lowercase())
        .collect();

    !leading.iter().any(|part| {
        matches!(
            part.as_str(),
            "fixtures" | "fixture" | "test-data" | "testdata" | "__fixtures__"
        )
    })
}

/// True when a Cargo.toml has `[workspace]` and no `[package]` section —
/// i.e. the root manifest of a virtual workspace, where the workspace itself
/// is what the user means by "the project" rather than any single crate.
fn cargo_toml_is_workspace_only(content: &str) -> bool {
    let mut has_workspace = false;
    let mut has_package = false;
    for line in content.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
            continue;
        }
        match trimmed {
            "[package]" => has_package = true,
            "[workspace]" => has_workspace = true,
            _ => {}
        }
    }
    has_workspace && !has_package
}

/// Workspace directory name (the parent of `.repolens/`) for a pack_dir that
/// follows the standard storage layout. `None` when the layout doesn't match
/// (e.g. tests passing a bare temp dir as `pack_dir`).
fn workspace_dir_name_from_pack_dir(pack_dir: &Path) -> Option<String> {
    let name = RepoStoragePaths::repo_root_from_pack_dir(pack_dir)?
        .file_name()?
        .to_str()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Extract `name = "..."` from the `[package]` section of a Cargo.toml.
/// Ignores `[workspace]`, `[workspace.package]`, `[dependencies]`, etc.
fn parse_cargo_package_name(content: &str) -> Option<String> {
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let name = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Table headers (`[a.b]` and `[[a.b]]`) in a TOML file, without brackets,
/// quotes or spaces, e.g. `project.scripts`.
fn toml_table_headers(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let t = line.split('#').next().unwrap_or("").trim();
            let inner = t.strip_prefix('[')?.strip_suffix(']')?;
            let inner = inner.trim_start_matches('[').trim_end_matches(']');
            Some(
                inner
                    .chars()
                    .filter(|c| !matches!(c, ' ' | '"' | '\''))
                    .collect(),
            )
        })
        .collect()
}

fn collect_rust_bin_targets(included: &[&FileEntry], pack_dir: &Path) -> HashSet<String> {
    let mut targets = HashSet::new();

    for entry in included {
        if !entry.path.ends_with("Cargo.toml") {
            continue;
        }
        let Some(content) = ProfileAnalyzer::read_content(entry, pack_dir) else {
            continue;
        };
        targets.extend(parse_cargo_bin_paths(&content, &entry.path));
    }

    targets
}

fn parse_cargo_bin_paths(content: &str, manifest_path: &str) -> Vec<String> {
    let manifest_dir = Path::new(manifest_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut in_bin = false;
    let mut paths = Vec::new();

    for line in content.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_bin = trimmed == "[[bin]]";
            continue;
        }
        if !in_bin {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "path" {
            continue;
        }
        let rel = value.trim().trim_matches('"').trim_matches('\'');
        if rel.is_empty() {
            continue;
        }
        let joined = if manifest_dir.as_os_str().is_empty() {
            Path::new(rel).to_path_buf()
        } else {
            manifest_dir.join(rel)
        };
        paths.push(normalize_repo_path(&joined));
    }

    paths
}

fn normalize_repo_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_rust_src_bin(path: &str) -> bool {
    fn matches_tail(tail: &str) -> bool {
        tail.ends_with(".rs") && !tail.contains('/')
    }

    if let Some(tail) = path.strip_prefix("src/bin/") {
        return matches_tail(tail);
    }
    if let Some((_, tail)) = path.rsplit_once("/src/bin/") {
        return matches_tail(tail);
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Evidence for profile observation invariants named in:
    //! - `docs/superpowers/specs/2026-04-13-observation-invariant-rollout-design.md`
    //! - `docs/superpowers/specs/2026-04-14-java-entry-point-catalogue.md`
    //! - `docs/superpowers/specs/2026-04-14-language-and-profile-catalogue.md`
    //! - `docs/superpowers/specs/2026-04-15-profile-signal-catalogue.md`
    //! - `docs/superpowers/specs/2026-04-15-entry-point-catalogue-expansion.md`
    //!
    //! Any rename, deletion, or `#[ignore]`-ing of a cited test here MUST be
    //! accompanied by a tier demotion of the affected family in
    //! `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`
    //! §5 and a corresponding catalogue update.

    use super::*;
    use repolens_core::model::manifest::Manifest;
    use repolens_core::serde_util::java_epoch_seconds::JavaEpochSeconds;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn make_manifest() -> Manifest {
        Manifest {
            schema_version: "1".to_string(),
            schema_url: "".to_string(),
            timestamp: JavaEpochSeconds(Decimal::from_str("1000000.0").unwrap()),
            chunks: vec![],
            scan_mode: None,
            scan_scope: None,
            base_pack_id: None,
            base_head_commit: None,
            head_commit: None,
            worktree_dirty: None,
            scan_config_hash: None,
            reprocessed_files: None,
            deleted_files: None,
            carried_forward_files: None,
        }
    }

    fn make_entry(path: &str, language: &str, loc: i32, content_path: Option<&str>) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            sha256: "abc123".to_string(),
            language: language.to_string(),
            loc,
            included: true,
            reason: "included".to_string(),
            content_path: content_path.map(|s| s.to_string()),
            symbols: vec![],
        }
    }

    fn make_index(entries: Vec<FileEntry>) -> PackIndex {
        PackIndex {
            manifest: make_manifest(),
            entries,
        }
    }

    fn write_content_file(tmp: &tempfile::TempDir, rel: &str, content: &str) -> String {
        let full = tmp.path().join("content").join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
        format!("content/{rel}")
    }

    fn entry_point_paths(profile: &ProjectProfile) -> Vec<&str> {
        profile
            .entry_points
            .iter()
            .map(|ep| ep.path.as_str())
            .collect()
    }

    #[test]
    fn primary_language_is_deterministic_on_loc_tie() {
        // index.ts (8 LOC) and util.js (8 LOC) tie within the "src" module.
        // The winner must be deterministic; per the captured Java contract
        // (profile_contract_test, mixed-small) TypeScript wins the tie over
        // JavaScript. compute_modules picked the primary language via max_by_key
        // over a HashMap whose per-instance iteration order varied -> the winner
        // was nondeterministic. Loop many times (each call builds a fresh
        // HashMap) so a single lucky ordering cannot mask a regression.
        for _ in 0..64 {
            let entries = [
                make_entry("src/index.ts", "TypeScript", 8, None),
                make_entry("src/util.js", "JavaScript", 8, None),
            ];
            let refs: Vec<&FileEntry> = entries.iter().collect();
            let modules = ProfileAnalyzer::compute_modules(&refs);
            let src = modules
                .iter()
                .find(|m| m.name == "src")
                .expect("expected 'src' module");
            assert_eq!(
                src.primary_language, "TypeScript",
                "LOC tie must resolve deterministically to the greater language name"
            );
        }
    }

    #[test]
    fn detects_maven_build_system() {
        let index = make_index(vec![make_entry("pom.xml", "XML", 50, None)]);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(profile.build_system.is_some());
        assert_eq!(profile.build_system.unwrap().name, "Maven");
    }

    #[test]
    fn detects_npm_build_system() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(&tmp, "package.json", r#"{"name":"my-app"}"#);
        let index = make_index(vec![make_entry(
            "package.json",
            "JSON",
            1,
            Some(&content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(profile.name, "my-app");
        assert!(profile.build_system.is_some());
        assert_eq!(profile.build_system.unwrap().name, "npm");
    }

    #[test]
    fn detects_gradle_build_system_from_supported_manifests() {
        for manifest in ["build.gradle", "build.gradle.kts"] {
            let index = make_index(vec![make_entry(manifest, "Unknown", 5, None)]);
            let tmp = tempfile::tempdir().unwrap();
            let profile = ProfileAnalyzer::analyze(&index, tmp.path());
            let build_system = profile.build_system.expect("expected build system");
            assert_eq!(build_system.name, "Gradle");
            assert_eq!(build_system.config_file, manifest);
        }
    }

    #[test]
    fn detects_cargo_build_system() {
        let index = make_index(vec![make_entry("Cargo.toml", "TOML", 3, None)]);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        let build_system = profile.build_system.expect("expected build system");
        assert_eq!(build_system.name, "Cargo");
        assert_eq!(build_system.config_file, "Cargo.toml");
    }

    #[test]
    fn detects_go_build_system() {
        let index = make_index(vec![make_entry("go.mod", "Go", 3, None)]);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        let build_system = profile.build_system.expect("expected build system");
        assert_eq!(build_system.name, "Go");
        assert_eq!(build_system.config_file, "go.mod");
    }

    #[test]
    fn detects_python_build_system_from_supported_manifests() {
        for manifest in ["pyproject.toml", "setup.py"] {
            let index = make_index(vec![make_entry(manifest, "Python", 3, None)]);
            let tmp = tempfile::tempdir().unwrap();
            let profile = ProfileAnalyzer::analyze(&index, tmp.path());
            let build_system = profile.build_system.expect("expected build system");
            assert_eq!(build_system.name, "Python");
            assert_eq!(build_system.config_file, manifest);
        }
    }

    #[test]
    fn no_build_system_returns_none() {
        let index = make_index(vec![make_entry("README.md", "Markdown", 10, None)]);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(profile.build_system.is_none());
    }

    #[test]
    fn detects_react_framework_from_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = "content/package.json";
        let full = tmp.path().join("content");
        std::fs::create_dir_all(&full).unwrap();
        std::fs::write(
            full.join("package.json"),
            r#"{"name":"my-app","dependencies":{"react":"^18"}}"#,
        )
        .unwrap();

        let index = make_index(vec![make_entry(
            "package.json",
            "JSON",
            1,
            Some(content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(
            profile.frameworks.iter().any(|f| f.name == "React"),
            "Expected React framework, got: {:?}",
            profile.frameworks
        );
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    #[test]
    fn detects_express_framework_from_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(
            &tmp,
            "package.json",
            r#"{"name":"api","dependencies":{"express":"^5.0.0"}}"#,
        );
        let index = make_index(vec![make_entry(
            "package.json",
            "JSON",
            1,
            Some(&content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        let framework = profile
            .frameworks
            .iter()
            .find(|f| f.name == "Express")
            .expect("expected Express framework");
        assert_eq!(framework.category, "backend");
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    #[test]
    fn detects_spring_boot_framework_from_pom_xml() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(
            &tmp,
            "pom.xml",
            "<project><dependency>spring-boot-starter-web</dependency></project>",
        );
        let index = make_index(vec![make_entry("pom.xml", "XML", 3, Some(&content_path))]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(profile.frameworks.iter().any(|f| f.name == "Spring Boot"));
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    #[test]
    fn detects_spring_framework_from_pom_xml() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(
            &tmp,
            "pom.xml",
            "<project><groupId>org.springframework</groupId></project>",
        );
        let index = make_index(vec![make_entry("pom.xml", "XML", 3, Some(&content_path))]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(profile.frameworks.iter().any(|f| f.name == "Spring"));
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    #[test]
    fn detects_django_framework_from_python_import() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path =
            write_content_file(&tmp, "src/app.py", "from django.http import HttpResponse\n");
        let index = make_index(vec![make_entry(
            "src/app.py",
            "Python",
            1,
            Some(&content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(profile.frameworks.iter().any(|f| f.name == "Django"));
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    #[test]
    fn detects_flask_framework_from_python_import() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(&tmp, "src/app.py", "from flask import Flask\n");
        let index = make_index(vec![make_entry(
            "src/app.py",
            "Python",
            1,
            Some(&content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(profile.frameworks.iter().any(|f| f.name == "Flask"));
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    /// I-PROFILE-JAVA-MAIN-ENTRY-POINT (entry point detection family, Java slice).
    /// See docs/superpowers/specs/2026-04-14-java-entry-point-catalogue.md.
    #[test]
    fn i_profile_java_main_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = "content/Main.java";
        let full = tmp.path().join("content");
        std::fs::create_dir_all(&full).unwrap();
        std::fs::write(
            full.join("Main.java"),
            "public class Main { public static void main(String[] args) {} }",
        )
        .unwrap();

        let entry = {
            let mut e = make_entry("src/Main.java", "Java", 5, Some(content_path));
            e.language = "Java".to_string();
            e
        };
        let index = make_index(vec![entry]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert!(
            profile
                .entry_points
                .iter()
                .any(|ep| ep.path == "src/Main.java"),
            "Expected entry point for src/Main.java, got: {:?}",
            profile.entry_points
        );
    }

    #[test]
    fn detects_python_dunder_main_entry_point() {
        let index = make_index(vec![make_entry("pkg/__main__.py", "Python", 2, None)]);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        let paths = entry_point_paths(&profile);
        assert!(paths.contains(&"pkg/__main__.py"));
        assert_eq!(profile.project_type, ProjectType::Cli);
    }

    #[test]
    fn detects_python_if_name_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(
            &tmp,
            "src/runner.py",
            "def main():\n    return 0\n\nif __name__ == \"__main__\":\n    main()\n",
        );
        let index = make_index(vec![make_entry(
            "src/runner.py",
            "Python",
            5,
            Some(&content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        let paths = entry_point_paths(&profile);
        assert!(paths.contains(&"src/runner.py"));
        assert_eq!(profile.project_type, ProjectType::Cli);
    }

    #[test]
    fn detects_go_main_function_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(
            &tmp,
            "cmd/server/main.go",
            "package main\n\nfunc main() {}\n",
        );
        let index = make_index(vec![make_entry(
            "cmd/server/main.go",
            "Go",
            3,
            Some(&content_path),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        let paths = entry_point_paths(&profile);
        assert!(paths.contains(&"cmd/server/main.go"));
        assert_eq!(profile.project_type, ProjectType::Cli);
    }

    #[test]
    fn detects_js_family_entry_points_for_supported_filenames() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            make_entry("index.js", "JavaScript", 1, None),
            make_entry("src/index.ts", "TypeScript", 1, None),
            make_entry("index.jsx", "JSX", 1, None),
            make_entry("src/index.tsx", "TSX", 1, None),
            make_entry("main.js", "JavaScript", 1, None),
            make_entry("src/main.ts", "TypeScript", 1, None),
            make_entry("app.js", "JavaScript", 1, None),
            make_entry("src/app.ts", "TypeScript", 1, None),
        ];
        let profile = ProfileAnalyzer::analyze(&make_index(entries), tmp.path());
        let paths = entry_point_paths(&profile);
        for expected in [
            "index.js",
            "src/index.ts",
            "index.jsx",
            "src/index.tsx",
            "main.js",
            "src/main.ts",
            "app.js",
            "src/app.ts",
        ] {
            assert!(
                paths.contains(&expected),
                "missing JS/TS entry point {expected}: {paths:?}"
            );
        }
        assert_eq!(profile.project_type, ProjectType::Cli);
    }

    #[test]
    fn rejects_deep_or_unsupported_js_family_entry_point_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            make_entry("apps/web/index.js", "JavaScript", 1, None),
            make_entry("packages/ui/src/main.ts", "TypeScript", 1, None),
            make_entry("main.jsx", "JSX", 1, None),
            make_entry("app.tsx", "TSX", 1, None),
        ];
        let profile = ProfileAnalyzer::analyze(&make_index(entries), tmp.path());
        let paths = entry_point_paths(&profile);
        assert!(
            !paths.contains(&"apps/web/index.js"),
            "depth > 1 must not be treated as a JS/TS entry point: {paths:?}"
        );
        assert!(
            !paths.contains(&"packages/ui/src/main.ts"),
            "depth > 1 must not be treated as a JS/TS entry point: {paths:?}"
        );
        assert!(
            !paths.contains(&"main.jsx"),
            "main.jsx is not part of the shipped filename contract: {paths:?}"
        );
        assert!(
            !paths.contains(&"app.tsx"),
            "app.tsx is not part of the shipped filename contract: {paths:?}"
        );
    }

    #[test]
    fn detects_cargo_workspace_name_from_directory_when_no_root_package() {
        // A Cargo workspace root (no [package]) plus a binary member crate.
        // detect_name should derive "repolens-rs" from the workspace directory
        // name (the parent of `.repolens/`), not from the binary crate's
        // [package] name = "repolens-cli". Reproduces the F1 regression from
        // docs/philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md.
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repolens-rs");
        let pack_dir = repo_root.join(".repolens").join("pack-test");
        let content_dir = pack_dir.join("content");
        std::fs::create_dir_all(content_dir.join("crates/repolens-cli/src")).unwrap();

        std::fs::write(
            content_dir.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::write(
            content_dir.join("crates/repolens-cli/Cargo.toml"),
            "[package]\nname = \"repolens-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            content_dir.join("crates/repolens-cli/src/main.rs"),
            "fn main() {}",
        )
        .unwrap();

        let entries = vec![
            make_entry("Cargo.toml", "TOML", 3, Some("content/Cargo.toml")),
            make_entry(
                "crates/repolens-cli/Cargo.toml",
                "TOML",
                3,
                Some("content/crates/repolens-cli/Cargo.toml"),
            ),
            make_entry(
                "crates/repolens-cli/src/main.rs",
                "Rust",
                1,
                Some("content/crates/repolens-cli/src/main.rs"),
            ),
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, &pack_dir);
        assert_eq!(
            profile.name, "repolens-rs",
            "expected workspace directory name 'repolens-rs', got {:?}",
            profile.name
        );
    }

    #[test]
    fn detects_cargo_workspace_name_from_member_package() {
        // A Cargo workspace root (no [package]) plus a member crate with
        // [package] name = "my-crate". detect_name should return "my-crate",
        // not fall through to "unknown".
        let tmp = tempfile::tempdir().unwrap();
        let content_dir = tmp.path().join("content");
        std::fs::create_dir_all(content_dir.join("crates/my-crate")).unwrap();

        std::fs::write(
            content_dir.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            content_dir.join("crates/my-crate/Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let entries = vec![
            make_entry("Cargo.toml", "TOML", 5, Some("content/Cargo.toml")),
            make_entry(
                "crates/my-crate/Cargo.toml",
                "TOML",
                3,
                Some("content/crates/my-crate/Cargo.toml"),
            ),
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(
            profile.name, "my-crate",
            "expected name from member [package], got {:?}",
            profile.name
        );
    }

    #[test]
    fn detects_cargo_root_package_name() {
        // A root Cargo.toml with [package] name = "repo-lens". detect_name
        // should return "repo-lens" before considering workspace-member
        // manifests.
        let tmp = tempfile::tempdir().unwrap();
        let content_dir = tmp.path().join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        std::fs::write(
            content_dir.join("Cargo.toml"),
            "[package]\nname = \"repo-lens\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let index = make_index(vec![make_entry(
            "Cargo.toml",
            "TOML",
            5,
            Some("content/Cargo.toml"),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(
            profile.name, "repo-lens",
            "expected name from root [package], got {:?}",
            profile.name
        );
    }

    #[test]
    fn cargo_workspace_prefers_binary_crate_over_alphabetical_library() {
        // Two member crates: 'alpha' (library) and 'zulu' (binary with src/main.rs).
        // Alphabetical order would pick 'alpha'; the binary-crate preference
        // should pick 'zulu'. This reflects what the workspace actually produces.
        let tmp = tempfile::tempdir().unwrap();
        let content_dir = tmp.path().join("content");
        std::fs::create_dir_all(content_dir.join("crates/alpha")).unwrap();
        std::fs::create_dir_all(content_dir.join("crates/zulu/src")).unwrap();

        std::fs::write(
            content_dir.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::write(
            content_dir.join("crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            content_dir.join("crates/zulu/Cargo.toml"),
            "[package]\nname = \"zulu\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(content_dir.join("crates/zulu/src/main.rs"), "fn main() {}").unwrap();

        let entries = vec![
            make_entry("Cargo.toml", "TOML", 3, Some("content/Cargo.toml")),
            make_entry(
                "crates/alpha/Cargo.toml",
                "TOML",
                3,
                Some("content/crates/alpha/Cargo.toml"),
            ),
            make_entry(
                "crates/zulu/Cargo.toml",
                "TOML",
                3,
                Some("content/crates/zulu/Cargo.toml"),
            ),
            make_entry(
                "crates/zulu/src/main.rs",
                "Rust",
                1,
                Some("content/crates/zulu/src/main.rs"),
            ),
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(
            profile.name, "zulu",
            "expected binary crate 'zulu' to win over alphabetical 'alpha', got {:?}",
            profile.name
        );
    }

    #[test]
    fn cargo_workspace_without_member_falls_back_to_unknown() {
        // Workspace root only, no member crate inspected → "unknown".
        let tmp = tempfile::tempdir().unwrap();
        let content_dir = tmp.path().join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::write(
            content_dir.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        let index = make_index(vec![make_entry(
            "Cargo.toml",
            "TOML",
            3,
            Some("content/Cargo.toml"),
        )]);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(profile.name, "unknown");
    }

    #[test]
    fn non_cargo_build_systems_fall_back_to_workspace_directory_name() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("sample-repo");
        let pack_dir = repo_root.join(".repolens").join("pack-test");

        for (path, language) in [
            ("pom.xml", "XML"),
            ("build.gradle", "Gradle"),
            ("build.gradle.kts", "Gradle"),
            ("go.mod", "Go"),
            ("pyproject.toml", "TOML"),
            ("setup.py", "Python"),
        ] {
            let index = make_index(vec![make_entry(path, language, 1, None)]);
            let profile = ProfileAnalyzer::analyze(&index, &pack_dir);
            assert_eq!(
                profile.name, "sample-repo",
                "expected workspace directory fallback for {path}, got {:?}",
                profile.name
            );
        }
    }

    #[test]
    fn detects_unknown_name_when_no_package_json_or_cargo_package_exists() {
        // Generic fallback coverage: when neither package.json nor a usable
        // Cargo [package] source exists, detect_name must return the shipped
        // "unknown" fallback instead of inventing a name.
        let tmp = tempfile::tempdir().unwrap();
        let entries = [make_entry("README.md", "Markdown", 1, None)];
        let included: Vec<&FileEntry> = entries.iter().collect();

        assert_eq!(
            ProfileAnalyzer::detect_name(&included, tmp.path()),
            "unknown"
        );
    }

    #[test]
    fn detects_rust_main_rs_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            {
                let mut e = make_entry("crates/cli/src/main.rs", "Rust", 5, None);
                e.language = "Rust".to_string();
                e
            },
            {
                let mut e = make_entry("crates/cli/src/lib.rs", "Rust", 5, None);
                e.language = "Rust".to_string();
                e
            },
            {
                let mut e = make_entry("src/main.rs", "Rust", 5, None);
                e.language = "Rust".to_string();
                e
            },
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        let paths: Vec<&str> = profile
            .entry_points
            .iter()
            .map(|ep| ep.path.as_str())
            .collect();
        assert!(
            paths.contains(&"crates/cli/src/main.rs"),
            "expected workspace-member main.rs as entry point, got: {:?}",
            paths
        );
        assert!(
            paths.contains(&"src/main.rs"),
            "expected root src/main.rs as entry point, got: {:?}",
            paths
        );
        assert!(
            !paths.contains(&"crates/cli/src/lib.rs"),
            "lib.rs must not be reported as an entry point, got: {:?}",
            paths
        );
    }

    #[test]
    fn fixture_like_rust_main_rs_is_not_reported_as_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            make_entry("fixtures/inputs/demo/src/main.rs", "Rust", 5, None),
            make_entry("src/lib.rs", "Rust", 5, None),
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        let paths = entry_point_paths(&profile);
        assert!(
            !paths.contains(&"fixtures/inputs/demo/src/main.rs"),
            "fixture-like paths must not define runtime shape: {paths:?}"
        );
    }

    #[test]
    fn fixture_only_entry_points_do_not_force_cli_project_type() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![make_entry("fixtures/demo/src/main.rs", "Rust", 5, None)];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        assert_eq!(profile.entry_points.len(), 0);
        assert_eq!(profile.project_type, ProjectType::Library);
    }

    #[test]
    fn fixture_only_entry_points_do_not_change_framework_driven_project_type() {
        let tmp = tempfile::tempdir().unwrap();
        let content_path = write_content_file(
            &tmp,
            "package.json",
            r#"{"name":"web","dependencies":{"react":"^18"}}"#,
        );
        let entries = vec![
            make_entry("package.json", "JSON", 1, Some(&content_path)),
            make_entry("fixtures/demo/src/main.rs", "Rust", 5, None),
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        assert_eq!(profile.entry_points.len(), 0);
        assert_eq!(profile.project_type, ProjectType::WebApp);
    }

    #[test]
    fn detects_rust_src_bin_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            {
                let mut e = make_entry("src/bin/tool.rs", "Rust", 5, None);
                e.language = "Rust".to_string();
                e
            },
            {
                let mut e = make_entry("src/lib.rs", "Rust", 5, None);
                e.language = "Rust".to_string();
                e
            },
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        let paths: Vec<&str> = profile
            .entry_points
            .iter()
            .map(|ep| ep.path.as_str())
            .collect();
        assert!(
            paths.contains(&"src/bin/tool.rs"),
            "expected src/bin/tool.rs as entry point, got: {:?}",
            paths
        );
        assert!(
            !paths.contains(&"src/lib.rs"),
            "lib.rs must not be reported as an entry point, got: {:?}",
            paths
        );
    }

    #[test]
    fn real_rust_main_rs_still_reports_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![make_entry("src/main.rs", "Rust", 5, None)];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        let paths = entry_point_paths(&profile);
        assert!(paths.contains(&"src/main.rs"));
        assert_eq!(profile.project_type, ProjectType::Cli);
    }

    #[test]
    fn detects_rust_explicit_bin_path_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let content_dir = tmp.path().join("content/crates/cli");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::write(
            content_dir.join("Cargo.toml"),
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"worker\"\npath = \"bin/worker.rs\"\n",
        )
        .unwrap();

        let entries = vec![
            make_entry(
                "crates/cli/Cargo.toml",
                "TOML",
                8,
                Some("content/crates/cli/Cargo.toml"),
            ),
            {
                let mut e = make_entry("crates/cli/bin/worker.rs", "Rust", 5, None);
                e.language = "Rust".to_string();
                e
            },
        ];
        let index = make_index(entries);
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());

        let paths: Vec<&str> = profile
            .entry_points
            .iter()
            .map(|ep| ep.path.as_str())
            .collect();
        assert!(
            paths.contains(&"crates/cli/bin/worker.rs"),
            "expected explicit [[bin]] path as entry point, got: {:?}",
            paths
        );
    }

    #[test]
    fn computes_language_distribution() {
        let entries = vec![
            make_entry("src/A.java", "Java", 100, None),
            make_entry("src/B.java", "Java", 50, None),
            make_entry("README.md", "Markdown", 20, None),
        ];
        let index = make_index(entries);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(profile.language_distribution.get("Java"), Some(&150));
        assert_eq!(profile.language_distribution.get("Markdown"), Some(&20));
    }

    #[test]
    fn computes_modules() {
        let entries = vec![
            make_entry("src/A.java", "Java", 100, None),
            make_entry("src/B.java", "Java", 50, None),
            make_entry("test/TestA.java", "Java", 30, None),
        ];
        let index = make_index(entries);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(profile.modules.len(), 2);
        let src_mod = profile.modules.iter().find(|m| m.name == "src").unwrap();
        assert_eq!(src_mod.file_count, 2);
        assert_eq!(src_mod.loc, 150);
        let test_mod = profile.modules.iter().find(|m| m.name == "test").unwrap();
        assert_eq!(test_mod.file_count, 1);
        assert_eq!(test_mod.loc, 30);
    }

    #[test]
    fn skips_dotfolder_top_level_modules() {
        let entries = vec![
            make_entry("src/lib.rs", "Rust", 100, None),
            make_entry(".github/workflows/ci.yml", "YAML", 50, None),
            make_entry(".claude/settings.json", "JSON", 20, None),
            make_entry(".vscode/settings.json", "JSON", 10, None),
        ];
        let index = make_index(entries);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(profile.modules.len(), 1);
        assert_eq!(profile.modules[0].name, "src");
    }

    #[test]
    fn library_type_when_no_entry_points_or_frameworks() {
        let index = make_index(vec![make_entry("src/lib.rs", "Rust", 200, None)]);
        let tmp = tempfile::tempdir().unwrap();
        let profile = ProfileAnalyzer::analyze(&index, tmp.path());
        assert_eq!(profile.project_type, ProjectType::Library);
    }

    // -- #25: manifests decide library vs app, and the Python lock tool ----

    fn analyze_files(files: &[(&str, &str, &str)]) -> ProjectProfile {
        let tmp = tempfile::tempdir().unwrap();
        let entries = files
            .iter()
            .map(|(path, lang, body)| {
                let cp = write_content_file(&tmp, path, body);
                make_entry(path, lang, body.lines().count() as i32, Some(&cp))
            })
            .collect();
        ProfileAnalyzer::analyze(&make_index(entries), tmp.path())
    }

    #[test]
    fn python_project_scripts_make_an_app() {
        let p = analyze_files(&[
            (
                "pyproject.toml",
                "TOML",
                "[project]\nname = \"tool\"\n\n[project.scripts]\ntool = \"tool.cli:main\"\n",
            ),
            ("tool/cli.py", "Python", "def main():\n    return 0\n"),
        ]);
        assert_eq!(p.project_type, ProjectType::Cli);
    }

    #[test]
    fn python_console_scripts_in_setup_py_make_an_app() {
        let p = analyze_files(&[
            (
                "setup.py",
                "Python",
                "from setuptools import setup\nsetup(name=\"tool\", entry_points=\
                 {\"console_scripts\": [\"tool=tool.cli:main\"]})\n",
            ),
            ("tool/cli.py", "Python", "def main():\n    return 0\n"),
        ]);
        assert_eq!(p.project_type, ProjectType::Cli);
    }

    #[test]
    fn node_package_with_main_and_no_bin_is_a_library() {
        let p = analyze_files(&[
            (
                "package.json",
                "JSON",
                r#"{"name":"lib","main":"index.js","exports":"./index.js"}"#,
            ),
            ("index.js", "JavaScript", "module.exports = {};\n"),
        ]);
        assert_eq!(p.project_type, ProjectType::Library);
    }

    #[test]
    fn node_package_with_bin_is_a_cli() {
        let p = analyze_files(&[
            (
                "package.json",
                "JSON",
                r#"{"name":"tool","main":"lib.js","bin":{"tool":"cli.js"}}"#,
            ),
            ("cli.js", "JavaScript", "require('./lib');\n"),
            ("lib.js", "JavaScript", "module.exports = {};\n"),
        ]);
        assert_eq!(p.project_type, ProjectType::Cli);
    }

    #[test]
    fn plain_setuptools_python_names_no_lock_tool() {
        let p = analyze_files(&[(
            "pyproject.toml",
            "TOML",
            "[build-system]\nrequires = [\"setuptools\"]\n\n[project]\nname = \"x\"\n",
        )]);
        let bs = p.build_system.expect("Python build system");
        assert_eq!(bs.name, "Python");
        assert_eq!(bs.details, None);
    }

    #[test]
    fn python_lock_tool_is_read_from_pyproject_or_pipfile() {
        for (tool, table) in [("poetry", "poetry"), ("pdm", "pdm"), ("uv", "uv")] {
            let body = format!("[project]\nname = \"x\"\n\n[tool.{table}]\n");
            let p = analyze_files(&[("pyproject.toml", "TOML", &body)]);
            assert_eq!(
                p.build_system.and_then(|b| b.details).as_deref(),
                Some(tool)
            );
        }
        let p = analyze_files(&[("Pipfile", "TOML", "[packages]\nrequests = \"*\"\n")]);
        let bs = p.build_system.expect("Pipfile is a Python manifest");
        assert_eq!(bs.name, "Python");
        assert_eq!(bs.details.as_deref(), Some("pipenv"));
    }
}
