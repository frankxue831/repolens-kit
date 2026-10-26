//! Cargo crate boundaries for Rust import resolution.
//!
//! The import graph must resolve `crate::` / `super::` / `self::` within the
//! crate that owns the file, and must never join two files in different
//! crates unless the source crate's `Cargo.toml` declares a dependency on the
//! target crate. This module reads every included `Cargo.toml` (from the
//! pack copy) to learn which directory belongs to which crate, where each
//! crate's module root is, and which workspace crates each one depends on.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use repolens_core::model::file_entry::FileEntry;
use repolens_core::path_safety::resolve_existing_subpath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCrate {
    /// Directory holding `Cargo.toml`, repo-relative ("" for the repo root).
    pub dir: String,
    /// `[package] name`, with `-` turned into `_`.
    pub package: String,
    /// Library name as used in `use` paths (`[lib] name`, else `package`).
    pub lib_name: String,
    /// Directories of target root files (`src/lib.rs`, `[[bin]] path`, ...).
    pub roots: Vec<String>,
    /// Packages this crate depends on (any dependency table), `_`-normalized.
    pub deps: BTreeSet<String>,
}

#[derive(Debug, Default, Clone)]
pub struct RustCrates {
    /// Sorted by directory depth, deepest first, so the first prefix match
    /// is the owning crate.
    crates: Vec<RustCrate>,
}

impl RustCrates {
    /// Read crate layout from the `Cargo.toml` entries of a scan.
    pub fn from_entries(entries: &[&FileEntry], pack_dir: &Path) -> Self {
        let paths: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        let mut crates = Vec::new();
        for e in entries {
            let is_manifest = e.path == "Cargo.toml" || e.path.ends_with("/Cargo.toml");
            if !is_manifest {
                continue;
            }
            let Some(text) = e
                .content_path
                .as_ref()
                .and_then(|cp| resolve_existing_subpath(pack_dir, cp).ok())
                .and_then(|p| std::fs::read_to_string(p).ok())
            else {
                continue;
            };
            let dir = e.path.strip_suffix("Cargo.toml").unwrap_or("");
            let dir = dir.strip_suffix('/').unwrap_or(dir).to_string();
            if let Some(c) = parse_manifest(&dir, &text, &paths) {
                crates.push(c);
            }
        }
        Self::from_crates(crates)
    }

    pub fn from_crates(mut crates: Vec<RustCrate>) -> Self {
        crates.sort_by(|a, b| {
            depth(&b.dir)
                .cmp(&depth(&a.dir))
                .then_with(|| a.dir.cmp(&b.dir))
        });
        Self { crates }
    }

    pub fn is_empty(&self) -> bool {
        self.crates.is_empty()
    }

    /// The crate owning `path`: the deepest crate directory containing it.
    pub fn crate_of(&self, path: &str) -> Option<&RustCrate> {
        self.crates.iter().find(|c| under(path, &c.dir))
    }

    /// Module root for `crate::` paths in `path`: the deepest target-root
    /// directory of the owning crate that contains the file.
    pub fn module_root(&self, path: &str) -> Option<String> {
        let c = self.crate_of(path)?;
        c.roots
            .iter()
            .filter(|r| under(path, r))
            .max_by_key(|r| r.len())
            .cloned()
    }

    /// True if an edge from `source` to `target` stays inside one crate or
    /// follows a dependency declared in the source crate's `Cargo.toml`.
    /// Files outside any known crate are not restricted.
    pub fn allows_edge(&self, source: &str, target: &str) -> bool {
        let (Some(s), Some(t)) = (self.crate_of(source), self.crate_of(target)) else {
            return true;
        };
        s.dir == t.dir || s.deps.contains(&t.package)
    }

    /// `use` name (lib name and package name) → module root of its library.
    pub fn lib_roots(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        for c in &self.crates {
            let lib_root = c
                .roots
                .iter()
                .find(|r| **r == join(&c.dir, "src"))
                .or_else(|| c.roots.first())
                .cloned()
                .unwrap_or_else(|| join(&c.dir, "src"));
            m.entry(c.lib_name.clone()).or_insert(lib_root.clone());
            m.entry(c.package.clone()).or_insert(lib_root);
        }
        m
    }
}

fn depth(dir: &str) -> usize {
    if dir.is_empty() {
        0
    } else {
        dir.matches('/').count() + 1
    }
}

fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

fn parent(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(d, _)| d.to_string())
        .unwrap_or_default()
}

fn normalize(name: &str) -> String {
    name.trim().replace('-', "_")
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v.split(" #").next().unwrap_or(v).trim();
    v.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Value of `key = "..."` inside an inline table like `{ path = "x", package = "y" }`.
fn inline_value(table: &str, key: &str) -> Option<String> {
    let inner = table.trim().strip_prefix('{')?.strip_suffix('}')?;
    inner.split(',').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.trim() == key).then(|| unquote(v))
    })
}

/// Line-based reader for the parts of `Cargo.toml` that matter here.
fn parse_manifest(dir: &str, text: &str, paths: &HashSet<&str>) -> Option<RustCrate> {
    let mut section = String::new();
    let mut package: Option<String> = None;
    let mut lib_name: Option<String> = None;
    let mut lib_path: Option<String> = None;
    let mut target_paths: Vec<String> = Vec::new();
    let mut deps = BTreeSet::new();

    let is_dep_table = |s: &str| {
        let last = s.rsplit('.').next().unwrap_or(s);
        matches!(
            last,
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(h) = line.strip_prefix('[') {
            let h = h.trim_start_matches('[');
            let h = h.split(']').next().unwrap_or("").trim().to_string();
            // `[dependencies.foo]` / `[target.'cfg(..)'.dependencies.foo]`
            if let Some((table, dep)) = h.rsplit_once('.') {
                if is_dep_table(table) {
                    deps.insert(normalize(dep.trim_matches(|c| c == '"' || c == '\'')));
                    section = format!("{table}.{dep}");
                    continue;
                }
            }
            section = h;
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim().trim_matches('"');
        match section.as_str() {
            "package" if k == "name" => package = Some(unquote(v)),
            "lib" if k == "name" => lib_name = Some(unquote(v)),
            "lib" if k == "path" => lib_path = Some(unquote(v)),
            "bin" | "test" | "bench" | "example" if k == "path" => target_paths.push(unquote(v)),
            s if is_dep_table(s) => {
                let real = inline_value(v, "package").unwrap_or_else(|| k.to_string());
                deps.insert(normalize(&real));
            }
            s if s.rsplit_once('.').is_some_and(|(t, _)| is_dep_table(t)) && k == "package" => {
                // `[dependencies.alias] package = "real"`
                deps.insert(normalize(&unquote(v)));
            }
            _ => {}
        }
    }

    let package = normalize(&package?);
    let mut roots: BTreeSet<String> = BTreeSet::new();
    let src = join(dir, "src");
    if paths.iter().any(|p| under(p, &src)) {
        roots.insert(src);
    }
    if let Some(lp) = lib_path {
        roots.insert(parent(&join(dir, &lp)));
    }
    for tp in target_paths {
        roots.insert(parent(&join(dir, &tp)));
    }
    Some(RustCrate {
        dir: dir.to_string(),
        lib_name: lib_name
            .map(|n| normalize(&n))
            .unwrap_or_else(|| package.clone()),
        package,
        roots: roots.into_iter().collect(),
        deps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(v: &[&'static str]) -> HashSet<&'static str> {
        v.iter().copied().collect()
    }

    #[test]
    fn parses_ripgrep_style_root_manifest() {
        let text = r#"
[package]
name = "ripgrep"
build = "build.rs"

[[bin]]
bench = false
path = "crates/core/main.rs"
name = "rg"

[[test]]
name = "integration"
path = "tests/tests.rs"

[dependencies]
grep = { version = "0.4.1", path = "crates/grep" }
grep-index = { version = "0.0.1", path = "crates/index", optional = true }
ignore = { version = "0.4.29", path = "crates/ignore" }
renamed = { package = "real-name", version = "1" }

[target.'cfg(unix)'.dependencies.tikv-jemallocator]
version = "0.7.0"

[dev-dependencies]
serde = "1.0.77"
"#;
        let c = parse_manifest("", text, &paths(&["crates/core/main.rs"])).unwrap();
        assert_eq!(c.package, "ripgrep");
        assert_eq!(
            c.roots,
            vec!["crates/core".to_string(), "tests".to_string()]
        );
        for d in [
            "grep",
            "grep_index",
            "ignore",
            "real_name",
            "tikv_jemallocator",
            "serde",
        ] {
            assert!(c.deps.contains(d), "{d}: {:?}", c.deps);
        }
        assert!(!c.deps.contains("renamed"));
    }

    #[test]
    fn owning_crate_and_edges() {
        let crates = RustCrates::from_crates(vec![
            RustCrate {
                dir: String::new(),
                package: "app".into(),
                lib_name: "app".into(),
                roots: vec!["src".into()],
                deps: ["core_lib".to_string()].into(),
            },
            RustCrate {
                dir: "crates/core".into(),
                package: "core_lib".into(),
                lib_name: "core_lib".into(),
                roots: vec!["crates/core/src".into()],
                deps: BTreeSet::new(),
            },
        ]);
        assert_eq!(
            crates.crate_of("crates/core/src/a.rs").unwrap().package,
            "core_lib"
        );
        assert_eq!(crates.crate_of("src/main.rs").unwrap().package, "app");
        assert_eq!(
            crates.module_root("crates/core/src/x/y.rs").as_deref(),
            Some("crates/core/src")
        );
        assert!(crates.allows_edge("src/main.rs", "crates/core/src/a.rs"));
        assert!(!crates.allows_edge("crates/core/src/a.rs", "src/main.rs"));
        assert!(crates.allows_edge("crates/core/src/a.rs", "crates/core/src/b.rs"));
        // "crates/core2" is not inside "crates/core".
        assert_eq!(
            crates.crate_of("crates/core2/src/a.rs").unwrap().package,
            "app"
        );
    }
}
