//! Non-import references that make a file "used" even though no source file
//! imports it (#12): JavaScript loaded by an HTML `<script src>` tag
//! (including Jekyll `_layouts/` and `_includes/`), scripts that a CI
//! workflow runs directly (`node run.cjs`, `python scripts/x.py`), Rust
//! modules loaded through `#[path = "..."]` (#19), and `build.rs` scripts
//! that sit next to a `Cargo.toml` (#26), and Next.js route and convention
//! files (`app/**/page.tsx`, `pages/**`, `middleware.ts`, ...) in a package
//! whose `package.json` depends on `next` (#46).
//!
//! This is a text scan, not a parser: it can miss references built at
//! runtime and may over-match a path mentioned in a comment. It only ever
//! removes files from the "may be unused" list, never adds any.

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;
use repolens_core::pack_reader::PackIndex;

/// Paths (as stored in the pack index) referenced by HTML script tags or by
/// CI workflow commands. Reads file bodies from `pack_dir`.
pub fn referenced_files(index: &PackIndex, pack_dir: &Path) -> HashSet<String> {
    let all: Vec<&str> = index.entries.iter().map(|e| e.path.as_str()).collect();
    let mut out = HashSet::new();
    let mut next_dirs = HashSet::new();
    for e in &index.entries {
        let lower = e.path.to_ascii_lowercase();
        let is_html = [".html", ".htm", ".liquid"]
            .iter()
            .any(|x| lower.ends_with(x));
        let is_ci = is_ci_workflow(&lower);
        let is_rust = lower.ends_with(".rs");
        let is_pkg = e.path == "package.json" || e.path.ends_with("/package.json");
        if !is_html && !is_ci && !is_rust && !is_pkg {
            continue;
        }
        let Some(cp) = e.content_path.as_deref() else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(pack_dir.join(cp)) else {
            continue;
        };
        if is_html {
            out.extend(script_src_refs(&e.path, &text, &all));
        }
        if is_ci {
            out.extend(ci_script_refs(&text, &all));
        }
        if is_rust {
            out.extend(rust_path_refs(&e.path, &text, &all));
        }
        if is_pkg && package_json_depends_on_next(&text) {
            let dir = e.path.strip_suffix("package.json").unwrap_or("");
            next_dirs.insert(dir.trim_end_matches('/').to_string());
        }
    }
    out.extend(cargo_build_scripts(&all));
    out.extend(nextjs_route_files(&all, &next_dirs));
    out
}

/// `true` when a `package.json` lists `next` under `dependencies` or
/// `devDependencies`. Unparseable JSON counts as no.
pub fn package_json_depends_on_next(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    ["dependencies", "devDependencies"]
        .iter()
        .any(|k| v.get(k).and_then(|d| d.get("next")).is_some())
}

/// Next.js loads these by name, so nothing imports them (#46). `next_dirs`
/// holds the directories (`""` for the repo root) whose `package.json`
/// depends on `next`; a file only counts when its nearest ancestor
/// `package.json` is one of them, so an `app/page.ts` in a package without
/// `next` stays in the "may be unused" list.
///
/// Relative to that package directory:
/// - `app/` or `src/app/`: files named `page`, `layout`, `route`, `loading`,
///   `error`, `global-error`, `not-found`, `template`, `default`, and the
///   metadata files (`sitemap`, `robots`, `manifest`, `icon`, `apple-icon`,
///   `opengraph-image`, `twitter-image`), at any depth, including route
///   groups `(x)`, dynamic segments `[slug]`, `[...x]`, `[[...x]]` and
///   parallel routes `@slot`. Private folders (`_name`) are not routes.
///   `global-not-found` counts only directly in `app/` (or `src/app/`).
/// - `pages/` or `src/pages/`: every page file, including `_app`,
///   `_document`, `_error` and `api/`.
/// - `middleware`, `proxy`, `instrumentation`, `instrumentation-client` and
///   `mdx-components` at the package root or in `src/`.
pub fn nextjs_route_files(all: &[&str], next_dirs: &HashSet<String>) -> Vec<String> {
    if next_dirs.is_empty() {
        return Vec::new();
    }
    let pkg_dirs: HashSet<&str> = all
        .iter()
        .filter_map(|p| {
            if *p == "package.json" {
                Some("")
            } else {
                p.strip_suffix("/package.json")
            }
        })
        .collect();
    all.iter()
        .filter(|p| {
            let Some(pkg) = nearest_package_dir(p, &pkg_dirs) else {
                return false;
            };
            if !next_dirs.contains(pkg) {
                return false;
            }
            let rel = if pkg.is_empty() {
                **p
            } else {
                &p[pkg.len() + 1..]
            };
            is_nextjs_convention_path(rel)
        })
        .map(|p| p.to_string())
        .collect()
}

/// Directory of the nearest `package.json` above `path` (`""` = root).
fn nearest_package_dir<'a>(path: &str, pkg_dirs: &HashSet<&'a str>) -> Option<&'a str> {
    let mut dir = path;
    while let Some((d, _)) = dir.rsplit_once('/') {
        if let Some(hit) = pkg_dirs.get(d) {
            return Some(hit);
        }
        dir = d;
    }
    pkg_dirs.get("").copied()
}

/// `rel` is relative to a Next.js package directory.
fn is_nextjs_convention_path(rel: &str) -> bool {
    const EXTS: &[&str] = &["tsx", "ts", "jsx", "js", "mjs", "mdx"];
    const APP_NAMES: &[&str] = &[
        "page",
        "layout",
        "route",
        "loading",
        "error",
        "global-error",
        "not-found",
        "forbidden",
        "unauthorized",
        "template",
        "default",
        "sitemap",
        "robots",
        "manifest",
        "icon",
        "apple-icon",
        "opengraph-image",
        "twitter-image",
    ];
    const ROOT_NAMES: &[&str] = &[
        "middleware",
        "proxy",
        "instrumentation",
        "instrumentation-client",
        "mdx-components",
    ];
    let (dirs, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let Some((stem, ext)) = file.rsplit_once('.') else {
        return false;
    };
    if !EXTS.contains(&ext) {
        return false;
    }
    // `.d.ts` declarations and test files are not routes.
    if stem.ends_with(".d") || stem.contains(".test") || stem.contains(".spec") {
        return false;
    }
    if dirs.is_empty() || dirs == "src" {
        return ROOT_NAMES.contains(&stem);
    }
    let under = |root: &str| -> Option<&str> {
        [root.to_string(), format!("src/{root}")]
            .into_iter()
            .find_map(|r| {
                if dirs == r {
                    Some("")
                } else {
                    dirs.strip_prefix(&format!("{r}/"))
                        .map(|rest| &dirs[dirs.len() - rest.len()..])
                }
            })
    };
    if let Some(sub) = under("app") {
        if stem == "global-not-found" {
            return sub.is_empty();
        }
        let private = sub.split('/').any(|seg| seg.starts_with('_'));
        return !private && APP_NAMES.contains(&stem);
    }
    under("pages").is_some()
}

/// Files named by Rust `#[path = "..."]` (or `#[cfg_attr(.., path = "..")]`)
/// attributes in `text` (file `from`). The path is relative to the
/// directory holding `from`, which is what rustc uses for a module declared
/// at file level. A `#[path]` inside an inline `mod x { }` block resolves
/// differently and is not handled.
pub fn rust_path_refs(from: &str, text: &str, all: &[&str]) -> Vec<String> {
    static ATTR: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r#"#\[\s*(?:cfg_attr\s*\([^\]]*?,\s*)?path\s*=\s*"([^"]+)""#).expect("regex")
    });
    let attr = &*ATTR;
    let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut out = Vec::new();
    for line in text.lines() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        for cap in attr.captures_iter(code) {
            let target = if dir.is_empty() {
                cap[1].to_string()
            } else {
                format!("{dir}/{}", &cap[1])
            };
            if let Some(p) = normalize(&target) {
                if all.contains(&p.as_str()) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// `build.rs` files in a directory that also holds a `Cargo.toml`: Cargo
/// runs them as the package's build script, wherever the package sits in
/// the repo. A `build.rs` with no manifest beside it is left alone.
pub fn cargo_build_scripts(all: &[&str]) -> Vec<String> {
    all.iter()
        .filter(|p| p.rsplit('/').next() == Some("build.rs"))
        .filter(|p| {
            let manifest = match p.rsplit_once('/') {
                Some((d, _)) => format!("{d}/Cargo.toml"),
                None => "Cargo.toml".to_string(),
            };
            all.contains(&manifest.as_str())
        })
        .map(|p| p.to_string())
        .collect()
}

fn is_ci_workflow(lower: &str) -> bool {
    (lower.starts_with(".github/workflows/")
        && (lower.ends_with(".yml") || lower.ends_with(".yaml")))
        || lower == ".gitlab-ci.yml"
        || lower == ".circleci/config.yml"
}

/// JS files named by `<script ... src=...>` tags in `html` (file `from`).
pub fn script_src_refs(from: &str, html: &str, all: &[&str]) -> Vec<String> {
    let tag = Regex::new(r"(?is)<script\b[^>]*\bsrc\s*=([^>]*)>").expect("regex");
    let token = Regex::new(r"[A-Za-z0-9_./@~-]+\.(?:m?js|cjs)\b").expect("regex");
    let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut out = Vec::new();
    for cap in tag.captures_iter(html) {
        for t in token.find_iter(&cap[1]) {
            let t = t.as_str();
            if t.contains("://") || t.starts_with("//") {
                continue;
            }
            // Root-relative first (Jekyll layouts are rendered at any URL),
            // then relative to the HTML file's directory.
            let mut candidates = vec![normalize(t.trim_start_matches('/'))];
            if !t.starts_with('/') {
                candidates.push(normalize(&format!("{dir}/{t}")));
            }
            if let Some(p) = candidates
                .into_iter()
                .flatten()
                .find(|c| all.contains(&c.as_str()))
            {
                out.push(p);
            }
        }
    }
    out
}

/// Script files a CI workflow names. A token with a directory must match a
/// path exactly; a bare file name (run with `working-directory:`) must
/// match exactly one indexed file.
pub fn ci_script_refs(yaml: &str, all: &[&str]) -> Vec<String> {
    let token = Regex::new(r"[A-Za-z0-9_./-]+\.(?:py|rb|sh|bash|js|cjs|mjs|ts|pl|php|go|rs)\b")
        .expect("regex");
    let mut out = Vec::new();
    for t in token.find_iter(yaml) {
        let Some(t) = normalize(t.as_str().trim_start_matches("./")) else {
            continue;
        };
        if all.contains(&t.as_str()) {
            out.push(t);
            continue;
        }
        let suffix = format!("/{t}");
        let hits: Vec<&&str> = all.iter().filter(|p| p.ends_with(&suffix)).collect();
        if hits.len() == 1 {
            out.push((*hits[0]).to_string());
        }
    }
    out
}

/// Resolve `.` and `..` segments; `None` if the path escapes the root.
fn normalize(p: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&str] = &[
        "_layouts/default.html",
        "assets/js/theme.js",
        "assets/js/search.js",
        "www/app.js",
        "www/index.html",
        "pw/run.cjs",
        "pw/live-check.cjs",
        "scripts/build.rb",
        "a/util.js",
        "b/util.js",
    ];

    #[test]
    fn jekyll_layout_script_tags_resolve_from_root() {
        let html = r#"<script src="{{ '/assets/js/theme.js' | relative_url }}"></script>
<script defer src="/assets/js/search.js?v=2"></script>
<script src="https://cdn.example.com/x.js"></script>"#;
        let mut got = script_src_refs("_layouts/default.html", html, ALL);
        got.sort();
        assert_eq!(got, vec!["assets/js/search.js", "assets/js/theme.js"]);
    }

    #[test]
    fn relative_script_tags_resolve_from_html_dir() {
        let got = script_src_refs("www/index.html", r#"<script src="./app.js">"#, ALL);
        assert_eq!(got, vec!["www/app.js"]);
    }

    #[test]
    fn rust_path_attributes_are_referenced() {
        // #19: ripgrep `crates/core/index/mod.rs`, gm-crypto-rs build.rs.
        let all = &[
            "crates/core/index/mod.rs",
            "crates/core/index/disabled.rs",
            "crates/core/index/enabled.rs",
            "crates/simd/build.rs",
            "crates/simd/build/rustc_version.rs",
            "src/sys/unix.rs",
        ][..];
        let src = "#[cfg(not(feature = \"x\"))]\n#[path = \"disabled.rs\"]\nmod imp;\n\
                   #[cfg_attr(feature = \"x\", path = \"enabled.rs\")]\nmod imp2;\n\
                   // #[path = \"gone.rs\"]\n";
        let mut got = rust_path_refs("crates/core/index/mod.rs", src, all);
        got.sort();
        assert_eq!(
            got,
            vec![
                "crates/core/index/disabled.rs",
                "crates/core/index/enabled.rs"
            ]
        );
        let got = rust_path_refs(
            "crates/simd/build.rs",
            "#[path = \"build/rustc_version.rs\"]\nmod rustc_version;\n",
            all,
        );
        assert_eq!(got, vec!["crates/simd/build/rustc_version.rs"]);
        let got = rust_path_refs(
            "src/lib.rs",
            "#[path = \"sys/../sys/unix.rs\"] mod sys;",
            all,
        );
        assert_eq!(got, vec!["src/sys/unix.rs"]);
    }

    #[test]
    fn build_scripts_next_to_a_cargo_manifest_are_referenced() {
        // #26: Cargo runs `build.rs` in any package root, not just the
        // repo root; a `build.rs` with no `Cargo.toml` beside it stays
        // visible.
        let all = &[
            "Cargo.toml",
            "build.rs",
            "crates/c/Cargo.toml",
            "crates/c/build.rs",
            "tools/build.rs",
            "crates/c/src/build.rs",
        ][..];
        let mut got = cargo_build_scripts(all);
        got.sort();
        assert_eq!(got, vec!["build.rs", "crates/c/build.rs"]);
    }

    #[test]
    fn ci_scripts_run_directly_are_referenced() {
        let yaml = "steps:\n  - run: node run.cjs\n    working-directory: pw\n  \
                    - run: ruby scripts/build.rb\n  - run: node util.js\n";
        let mut got = ci_script_refs(yaml, ALL);
        got.sort();
        // `util.js` is ambiguous (two matches), so it is not claimed.
        assert_eq!(got, vec!["pw/run.cjs", "scripts/build.rb"]);
    }

    #[test]
    fn nextjs_route_files_need_a_next_dependency() {
        // #46: route and convention files in a package that depends on
        // `next` are loaded by name; the same layout without `next` is not.
        let all = &[
            "package.json",
            "app/layout.tsx",
            "app/page.tsx",
            "app/loading.tsx",
            "app/error.tsx",
            "app/global-error.tsx",
            "app/global-not-found.tsx",
            "app/blog/global-not-found.tsx",
            "app/not-found.tsx",
            "app/template.tsx",
            "app/sitemap.ts",
            "app/robots.ts",
            "app/api/x/route.ts",
            "app/(marketing)/about/page.tsx",
            "app/blog/[slug]/page.tsx",
            "app/blog/[slug]/opengraph-image.tsx",
            "app/docs/[...parts]/page.tsx",
            "app/shop/[[...cat]]/page.tsx",
            "app/dashboard/default.tsx",
            "app/dashboard/@analytics/page.tsx",
            "app/_components/page.tsx",
            "app/components/Button.tsx",
            "pages/_app.tsx",
            "pages/_document.tsx",
            "pages/posts/[id].tsx",
            "pages/api/hello.ts",
            "pages/types.d.ts",
            "middleware.ts",
            "src/middleware.ts",
            "mdx-components.tsx",
            "src/mdx-components.ts",
            "lib/page.tsx",
            "lib/middleware.ts",
            "lib/mdx-components.tsx",
            "packages/docs/package.json",
            "packages/docs/app/page.tsx",
            "packages/docs/app/[lang]/layout.tsx",
            "apps/site/package.json",
            "apps/site/src/app/page.tsx",
            "apps/site/src/pages/about.tsx",
            "apps/site/src/middleware.ts",
            "apps/site/src/app/global-not-found.jsx",
            "apps/site/mdx-components.jsx",
            "packages/plain/package.json",
            "packages/plain/app/page.ts",
            "packages/plain/pages/index.ts",
            "packages/plain/middleware.ts",
            "packages/plain/mdx-components.tsx",
            "packages/plain/app/global-not-found.tsx",
        ][..];
        let next_dirs: HashSet<String> = ["", "packages/docs", "apps/site"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let got: HashSet<String> = nextjs_route_files(all, &next_dirs).into_iter().collect();
        let not_routes = [
            "package.json",
            "app/_components/page.tsx",
            "app/components/Button.tsx",
            "pages/types.d.ts",
            "lib/page.tsx",
            "lib/middleware.ts",
            "lib/mdx-components.tsx",
            "app/blog/global-not-found.tsx",
            "packages/docs/package.json",
            "apps/site/package.json",
            "packages/plain/package.json",
            "packages/plain/app/page.ts",
            "packages/plain/pages/index.ts",
            "packages/plain/middleware.ts",
            "packages/plain/mdx-components.tsx",
            "packages/plain/app/global-not-found.tsx",
        ];
        for p in all {
            assert_eq!(got.contains(*p), !not_routes.contains(p), "{p}: {got:?}");
        }
        // No package depends on `next`: nothing is claimed.
        assert!(nextjs_route_files(all, &HashSet::new()).is_empty());
    }

    #[test]
    fn next_dependency_is_read_from_dependencies_or_dev_dependencies() {
        assert!(package_json_depends_on_next(
            r#"{"dependencies":{"next":"14.2.0"}}"#
        ));
        assert!(package_json_depends_on_next(
            r#"{"devDependencies":{"next":"^15"}}"#
        ));
        assert!(!package_json_depends_on_next(
            r#"{"dependencies":{"next-auth":"4"},"peerDependencies":{"next":"14"}}"#
        ));
        assert!(!package_json_depends_on_next(r#"{"name":"plain"}"#));
        assert!(!package_json_depends_on_next("not json"));
    }
}
