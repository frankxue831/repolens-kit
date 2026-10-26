//! Repo-context detection. Produces `Vec<RepoContextTag>` plus indicator-match
//! notes from a `ProjectProfile`. Project-shape tags (e.g. `RustLibrary`) are
//! proven from manifests; `*Indicators` tags are suggestive and every emitted
//! indicator carries a `note` describing the literal evidence that matched.
//!
//! See `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` §F.

use repolens_core::model::evidence::RepoContextTag;
use repolens_core::model::profile::{ProjectProfile, ProjectType};

/// Output of repo-context detection. The `notes` vec is a parallel record of
/// why each indicator tag fired, intended for `EvidenceFinding::false_positive_notes`.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoContext {
    pub tags: Vec<RepoContextTag>,
    pub notes: Vec<IndicatorMatch>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorMatch {
    pub tag: RepoContextTag,
    pub note: String,
}

impl RepoContext {
    pub fn empty() -> Self {
        Self {
            tags: vec![],
            notes: vec![],
        }
    }

    pub fn has(&self, tag: &RepoContextTag) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    pub fn any_indicator(&self) -> bool {
        self.tags.iter().any(|t| t.is_indicator())
    }

    pub fn any_rust(&self) -> bool {
        self.tags.iter().any(|t| t.is_rust())
    }
}

// ---------------------------------------------------------------------------
// Indicator name patterns. Single source of truth so amendments can be
// reviewed in one place. Every match emits a `note` quoting the literal
// matcher and the matched value.
// ---------------------------------------------------------------------------

// Strong cryptographic-intent indicators. Bare hash crates (sha2, sha3,
// blake2, blake3) are deliberately excluded: they are widely used for
// non-cryptographic purposes (content addressing, cache keys, build
// artifact integrity) and false-positive on dev tools. A repo doing
// genuine cryptography will also pull in a primitive/protocol crate from
// this list, so the signal is preserved without the false positives.
const CRYPTO_DEP_PATTERNS: &[&str] = &[
    "ring",
    "rustls",
    "ed25519",
    "ed25519-dalek",
    "aes",
    "aes-gcm",
    "chacha20",
    "chacha20poly1305",
    "rsa",
    "x25519",
    "curve25519",
    "p256",
    "k256",
    "secp256k1",
    "openssl",
    "openssl-sys",
    "boring",
    "nacl",
    "sodiumoxide",
    "libsodium",
    "bcrypt",
    "scrypt",
    "argon2",
    "hmac",
    "hkdf",
    "pbkdf2",
];

const SECURITY_SENSITIVE_DEP_PATTERNS: &[&str] = &[
    "jsonwebtoken",
    "jwt",
    "oauth2",
    "openidconnect",
    "saml",
    "paseto",
    "biscuit",
    "casbin",
    "totp",
    "webauthn",
    "ldap3",
];

const DATABASE_DEP_PATTERNS: &[&str] = &[
    "sqlx",
    "diesel",
    "sea-orm",
    "sea-query",
    "rusqlite",
    "tokio-postgres",
    "postgres",
    "mongodb",
    "redis",
    "rocksdb",
    "sled",
    "tikv-client",
    "scylla",
];

// Strong compiler-specific indicators. Generic parser / proc-macro
// substrate crates (syn, quote, proc-macro2 — used by nearly every
// derive macro in the ecosystem; nom, logos, chumsky, pest, tree-sitter
// — general-purpose parsing libraries used by dev tools, linters,
// formatters, doc generators, repo-analysis tools) are deliberately
// excluded: they false-positive on anything that parses code without
// being a compiler. The remaining patterns are parser-generators and
// codegen backends that have no reasonable non-compiler use.
const COMPILER_DEP_PATTERNS: &[&str] = &[
    "lalrpop",
    "lalrpop-util",
    "pest_derive",
    "cranelift",
    "inkwell",
    "llvm-sys",
];

const INFRASTRUCTURE_DEP_PATTERNS: &[&str] = &[
    "kube",
    "k8s-openapi",
    "aws-sdk",
    "rusoto",
    "azure-sdk",
    "google-cloud",
    "terraform",
    "pulumi",
];

const NO_STD_DEP_PATTERNS: &[&str] = &[
    "heapless",
    "embedded-hal",
    "embedded-io",
    "cortex-m",
    "riscv",
    "panic-halt",
    "panic-abort",
    "rtic",
    "embassy",
    "stm32",
    "nrf",
    "esp-hal",
    "rp2040",
];

const CRYPTO_PATH_HINTS: &[&str] = &[
    "/crypto",
    "/cipher",
    "/keystore",
    "/signing",
    "/signer",
    "/encrypt",
    "/decrypt",
];

const SECURITY_PATH_HINTS: &[&str] = &["/auth", "/oauth", "/saml", "/mtls", "/jwt", "/openid"];

const DATABASE_PATH_HINTS: &[&str] = &["/migrations/", "/repository/", "/repositories/"];

const COMPILER_PATH_HINTS: &[&str] = &["/parser/", "/lexer/", "/codegen/", "/ast/"];

const INFRASTRUCTURE_PATH_HINTS: &[&str] = &[
    "/infra/",
    "/terraform/",
    "/k8s/",
    "/kubernetes/",
    "/helm/",
    "/ansible/",
];

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

pub struct RepoContextDetector;

impl RepoContextDetector {
    /// Detect repo-context tags from a `ProjectProfile` and a list of file
    /// paths (relative to the repo root). Tags are deterministically ordered:
    /// project-shape first, then indicator tags, then `Custom` last.
    ///
    /// This shim does not consult manifest dependencies. Callers that have
    /// access to the repo root should prefer
    /// [`RepoContextDetector::detect_with_manifest_deps`] so that crate-name
    /// indicators (e.g. `ring`, `sqlx`, `heapless`) actually fire — the
    /// `ProjectProfile::frameworks` field detects framework names like
    /// "Spring" or "Express", not raw Cargo / npm dependency names.
    pub fn detect(profile: &ProjectProfile, file_paths: &[String]) -> RepoContext {
        Self::detect_with_manifest_deps(profile, file_paths, &[])
    }

    /// Detect repo-context tags from a `ProjectProfile`, file paths, and an
    /// optional list of manifest dependency names (typically extracted from
    /// `Cargo.toml`'s `[dependencies]` / `[dev-dependencies]` /
    /// `[workspace.dependencies]` sections). Indicator tag matching considers
    /// framework detections, manifest dep names, and path hints.
    pub fn detect_with_manifest_deps(
        profile: &ProjectProfile,
        file_paths: &[String],
        manifest_dep_names: &[String],
    ) -> RepoContext {
        Self::detect_full(profile, file_paths, manifest_dep_names, false)
    }

    /// Detect repo-context tags with full information available, including
    /// whether the root manifest declares a Cargo `[workspace]`. Callers
    /// that have access to the repo root should prefer this method:
    /// `is_cargo_workspace` is the only way to surface the
    /// `RustWorkspace` tag without false positives from nested fixture
    /// `Cargo.toml` files.
    pub fn detect_full(
        profile: &ProjectProfile,
        file_paths: &[String],
        manifest_dep_names: &[String],
        is_cargo_workspace: bool,
    ) -> RepoContext {
        let mut tags: Vec<RepoContextTag> = Vec::new();
        let mut notes: Vec<IndicatorMatch> = Vec::new();

        // ---- Project-shape detection (proven from manifests) -------------
        let build_name = profile
            .build_system
            .as_ref()
            .map(|b| b.name.to_ascii_lowercase());

        match build_name.as_deref() {
            Some("cargo") => {
                if is_cargo_workspace {
                    tags.push(RepoContextTag::RustWorkspace);
                }
                match profile.project_type {
                    ProjectType::Library => tags.push(RepoContextTag::RustLibrary),
                    ProjectType::Cli | ProjectType::WebApp => {
                        tags.push(RepoContextTag::RustBinary);
                    }
                    ProjectType::Unknown => {
                        // Fall back to entry-point-presence as a tiebreaker.
                        if profile.entry_points.is_empty() {
                            tags.push(RepoContextTag::RustLibrary);
                        } else {
                            tags.push(RepoContextTag::RustBinary);
                        }
                    }
                }
            }
            Some("maven") | Some("gradle") => match profile.project_type {
                ProjectType::Library => tags.push(RepoContextTag::JavaLibrary),
                ProjectType::WebApp => tags.push(RepoContextTag::JavaService),
                ProjectType::Cli | ProjectType::Unknown => {
                    tags.push(RepoContextTag::JavaApplication);
                }
            },
            Some("npm") | Some("yarn") | Some("pnpm") | Some("node") => {
                if matches!(profile.project_type, ProjectType::Library) {
                    tags.push(RepoContextTag::NodeLibrary);
                } else {
                    tags.push(RepoContextTag::NodeApplication);
                }
            }
            Some("go") | Some("gomod") => {
                tags.push(RepoContextTag::GoModule);
                if !profile.entry_points.is_empty() {
                    tags.push(RepoContextTag::GoBinary);
                }
            }
            // ProfileAnalyzer emits "Python" for both pyproject.toml and
            // setup.py manifests; lowercase "python" is the canonical match.
            // The other names are kept for forward compatibility.
            Some("python") | Some("pip") | Some("poetry") | Some("setuptools")
            | Some("pyproject") => {
                if matches!(profile.project_type, ProjectType::Library) {
                    tags.push(RepoContextTag::PythonPackage);
                } else {
                    tags.push(RepoContextTag::PythonScript);
                }
            }
            _ => {}
        }

        // ---- Indicator tags ---------------------------------------------
        // Track framework detections (high-level: "Spring", "React") and
        // raw manifest dep names (Cargo crate names like "ring", "sqlx")
        // separately so that match notes can describe the actual evidence
        // source rather than always crediting "project frameworks".
        let framework_names: Vec<String> = profile
            .frameworks
            .iter()
            .map(|f| f.name.to_ascii_lowercase())
            .collect();
        let manifest_lower: Vec<String> = manifest_dep_names
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();

        // Helper closure: describe the source of a matched dep name. If
        // the name appears in framework_names it came from the profile's
        // framework detection; otherwise it came from manifest deps.
        let describe_dep_source = |matched: &str| -> &'static str {
            if framework_names.iter().any(|n| n == matched) {
                "via project frameworks"
            } else {
                "via manifest dependency"
            }
        };
        let mut dep_names: Vec<String> = framework_names.clone();
        for name in &manifest_lower {
            if !dep_names.contains(name) {
                dep_names.push(name.clone());
            }
        }

        if let Some(matched) = match_dep_pattern(&dep_names, CRYPTO_DEP_PATTERNS) {
            tags.push(RepoContextTag::CryptoIndicators);
            let source = describe_dep_source(&matched);
            notes.push(IndicatorMatch {
                tag: RepoContextTag::CryptoIndicators,
                note: format!(
                    "cryptoIndicators matched on dependency '{matched}' ({source}). Suggestive, not proof of cryptographic intent."
                ),
            });
        }
        if let Some(matched) = match_path_hint(file_paths, CRYPTO_PATH_HINTS) {
            if !tags.contains(&RepoContextTag::CryptoIndicators) {
                tags.push(RepoContextTag::CryptoIndicators);
            }
            notes.push(IndicatorMatch {
                tag: RepoContextTag::CryptoIndicators,
                note: format!(
                    "cryptoIndicators matched on path hint '{matched}'. Suggestive, not proof of cryptographic intent."
                ),
            });
        }

        if let Some(matched) = match_dep_pattern(&dep_names, SECURITY_SENSITIVE_DEP_PATTERNS) {
            tags.push(RepoContextTag::SecuritySensitiveIndicators);
            notes.push(IndicatorMatch {
                tag: RepoContextTag::SecuritySensitiveIndicators,
                note: format!(
                    "securitySensitiveIndicators matched on dependency '{matched}'. Suggestive, not proof of security-sensitive responsibilities."
                ),
            });
        }
        if let Some(matched) = match_path_hint(file_paths, SECURITY_PATH_HINTS) {
            if !tags.contains(&RepoContextTag::SecuritySensitiveIndicators) {
                tags.push(RepoContextTag::SecuritySensitiveIndicators);
            }
            notes.push(IndicatorMatch {
                tag: RepoContextTag::SecuritySensitiveIndicators,
                note: format!("securitySensitiveIndicators matched on path hint '{matched}'."),
            });
        }

        if let Some(matched) = match_dep_pattern(&dep_names, NO_STD_DEP_PATTERNS) {
            tags.push(RepoContextTag::NoStdIndicators);
            notes.push(IndicatorMatch {
                tag: RepoContextTag::NoStdIndicators,
                note: format!(
                    "noStdIndicators matched on dependency '{matched}'. Suggestive, not proof of no_std compatibility."
                ),
            });
        }

        if let Some(matched) = match_dep_pattern(&dep_names, DATABASE_DEP_PATTERNS) {
            tags.push(RepoContextTag::DatabaseIndicators);
            notes.push(IndicatorMatch {
                tag: RepoContextTag::DatabaseIndicators,
                note: format!("databaseIndicators matched on dependency '{matched}'."),
            });
        }
        if let Some(matched) = match_path_hint(file_paths, DATABASE_PATH_HINTS) {
            if !tags.contains(&RepoContextTag::DatabaseIndicators) {
                tags.push(RepoContextTag::DatabaseIndicators);
            }
            notes.push(IndicatorMatch {
                tag: RepoContextTag::DatabaseIndicators,
                note: format!("databaseIndicators matched on path hint '{matched}'."),
            });
        }

        if let Some(matched) = match_dep_pattern(&dep_names, COMPILER_DEP_PATTERNS) {
            tags.push(RepoContextTag::CompilerIndicators);
            notes.push(IndicatorMatch {
                tag: RepoContextTag::CompilerIndicators,
                note: format!("compilerIndicators matched on dependency '{matched}'."),
            });
        }
        if let Some(matched) = match_path_hint(file_paths, COMPILER_PATH_HINTS) {
            if !tags.contains(&RepoContextTag::CompilerIndicators) {
                tags.push(RepoContextTag::CompilerIndicators);
            }
            notes.push(IndicatorMatch {
                tag: RepoContextTag::CompilerIndicators,
                note: format!("compilerIndicators matched on path hint '{matched}'."),
            });
        }

        if let Some(matched) = match_dep_pattern(&dep_names, INFRASTRUCTURE_DEP_PATTERNS) {
            tags.push(RepoContextTag::InfrastructureIndicators);
            notes.push(IndicatorMatch {
                tag: RepoContextTag::InfrastructureIndicators,
                note: format!("infrastructureIndicators matched on dependency '{matched}'."),
            });
        }
        if let Some(matched) = match_path_hint(file_paths, INFRASTRUCTURE_PATH_HINTS) {
            if !tags.contains(&RepoContextTag::InfrastructureIndicators) {
                tags.push(RepoContextTag::InfrastructureIndicators);
            }
            notes.push(IndicatorMatch {
                tag: RepoContextTag::InfrastructureIndicators,
                note: format!("infrastructureIndicators matched on path hint '{matched}'."),
            });
        }

        // Deduplicate while preserving first-seen order.
        let mut seen = std::collections::HashSet::new();
        tags.retain(|t| seen.insert(t.clone()));

        RepoContext { tags, notes }
    }
}

/// True iff the root `Cargo.toml` declares a `[workspace]` (or
/// `[workspace.dependencies]`/`[workspace.package]`) section. This is the
/// only proof of a Cargo workspace; nested `Cargo.toml` files do not
/// imply that the root is a workspace.
pub fn cargo_root_is_workspace(repo_root: &std::path::Path) -> bool {
    let cargo_toml = repo_root.join("Cargo.toml");
    let Ok(content) = std::fs::read_to_string(&cargo_toml) else {
        return false;
    };
    for raw in content.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = parse_toml_section_header(line) {
            if header == "workspace"
                || header.starts_with("workspace.")
                || header == "workspace.dependencies"
            {
                return true;
            }
        }
    }
    false
}

/// Whether the root Cargo project (the root package and every member of a
/// root `[workspace]`) has a binary target: a `[[bin]]` table, a
/// `src/main.rs`, or a `src/bin/` directory. Nested crates outside the
/// root workspace (e.g. a cargo-fuzz `fuzz/` crate with its own
/// `[workspace]`, listed in `exclude`) have their own lockfile and do not
/// count. `None` when there is no root `Cargo.toml`.
///
/// Member globs are expanded only for a trailing `/*` (`crates/*`), the
/// common form; other patterns are skipped.
pub fn cargo_root_has_binary_target(repo_root: &std::path::Path) -> Option<bool> {
    let content = std::fs::read_to_string(repo_root.join("Cargo.toml")).ok()?;
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut section = String::new();
    let mut has_root_package = false;
    let mut in_members = false;
    let mut members: Vec<String> = Vec::new();
    for raw in content.lines() {
        let line = strip_toml_comment(raw).trim();
        if in_members {
            members.extend(quoted_strings(line));
            if line.contains(']') {
                in_members = false;
            }
            continue;
        }
        if let Some(h) = parse_toml_section_header(line) {
            section = h.to_string();
            if section == "package" {
                has_root_package = true;
            }
            continue;
        }
        if line.starts_with("[[bin]]") {
            return Some(true);
        }
        if section == "workspace" {
            if let Some(rest) = line.strip_prefix("members") {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    members.extend(quoted_strings(v));
                    in_members = !v.contains(']');
                }
            }
        }
    }
    if has_root_package || members.is_empty() {
        dirs.push(repo_root.to_path_buf());
    }
    for m in members {
        if let Some(parent) = m.strip_suffix("/*") {
            if let Ok(rd) = std::fs::read_dir(repo_root.join(parent)) {
                for e in rd.flatten() {
                    if e.path().join("Cargo.toml").is_file() {
                        dirs.push(e.path());
                    }
                }
            }
        } else if !m.contains('*') {
            dirs.push(repo_root.join(m));
        }
    }
    Some(dirs.iter().any(|d| {
        d.join("src/main.rs").is_file()
            || d.join("src/bin").is_dir()
            || std::fs::read_to_string(d.join("Cargo.toml"))
                .is_ok_and(|c| c.lines().any(|l| l.trim_start().starts_with("[[bin]]")))
    }))
}

fn quoted_strings(s: &str) -> Vec<String> {
    s.split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, v)| v.to_string())
        .collect()
}

/// Strip an unquoted `#` line-comment from a TOML line. Naive (does not
/// understand string literals), but Cargo's `[section]` headers and bare
/// dep keys never contain quoted `#`, so trailing-comment handling is
/// the only thing that matters for this parser.
fn strip_toml_comment(raw: &str) -> &str {
    match raw.find('#') {
        Some(idx) => &raw[..idx],
        None => raw,
    }
}

/// Parse a `[section.subsection]` header from a TOML line that has had
/// any inline `#`-comment already stripped. Returns the bracketed body
/// trimmed, or `None` when the line is not a valid section header.
fn parse_toml_section_header(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let body = rest.strip_suffix(']')?;
    Some(body.trim())
}

/// Extract dependency names from the root `Cargo.toml` plus every
/// member-manifest reachable through the pack-relative `manifest_paths`.
/// Returns the deduplicated union of dependency names.
///
/// Two safety rules:
///   1. Member manifests are only consulted when their containing
///      directory matches a `members = [...]` pattern declared in the
///      root `Cargo.toml`. Fixture/example crates that happen to be in
///      the pack but are not declared workspace members are NOT
///      surveyed for indicator detection.
///   2. Each candidate path is validated as a relative, non-escaping
///      subpath before being joined to `repo_root`, so a malformed pack
///      entry cannot read a manifest outside the scanned root.
pub fn extract_cargo_dependency_names_recursive(
    repo_root: &std::path::Path,
    manifest_paths: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = extract_cargo_dependency_names(repo_root);

    // Parse declared workspace members and excludes from the root
    // Cargo.toml. If the root has no `[workspace] members = [...]`, no
    // nested manifests are surveyed — the recursive variant collapses to
    // the root-only path.
    let root_content = std::fs::read_to_string(repo_root.join("Cargo.toml")).unwrap_or_default();
    let members = parse_workspace_members(&root_content);
    if members.is_empty() {
        return out;
    }
    let excludes = parse_workspace_excludes(&root_content);

    for rel in manifest_paths {
        if rel == "Cargo.toml" {
            continue; // root manifest already covered
        }
        // Filter to Cargo.toml basenames only.
        let basename = rel.rsplit('/').next().unwrap_or(rel);
        if !basename.eq_ignore_ascii_case("Cargo.toml") {
            continue;
        }
        // Validate path safety: reject absolute paths, `..` components,
        // and Windows-style root prefixes.
        if !is_relative_safe_subpath(rel) {
            continue;
        }
        // Restrict to declared workspace members and respect excludes.
        let parent = match rel.rsplit_once('/').map(|(p, _)| p) {
            Some(p) => p,
            None => continue,
        };
        if !members.iter().any(|m| member_path_matches(m, parent)) {
            continue;
        }
        if excludes.iter().any(|e| member_path_matches(e, parent)) {
            continue;
        }
        let abs = repo_root.join(rel);
        for name in parse_cargo_dep_names(&abs) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Parse the `members = [...]` array from a Cargo.toml `[workspace]`
/// section. Handles both single-line and multi-line array literals.
/// Returns an empty vec when the manifest has no `[workspace]` section,
/// or when `members` appears under a different section such as
/// `[package.metadata]` (those are not workspace members and must not
/// drive recursive Cargo.toml reads).
fn parse_workspace_members(content: &str) -> Vec<String> {
    parse_workspace_array(content, "members")
}

/// True iff `rel` is a relative path that does not contain `..`, does not
/// start with an absolute root or Windows-style prefix, and would not
/// escape its base when joined. Mirrors `path_safety::validate_relative_subpath`
/// without taking a dependency on repolens-core's I/O surface.
fn is_relative_safe_subpath(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    let bytes = rel.as_bytes();
    if (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || rel.starts_with('/')
        || rel.starts_with('\\')
    {
        return false;
    }
    for component in std::path::Path::new(rel).components() {
        match component {
            std::path::Component::Normal(_) => {}
            _ => return false,
        }
    }
    true
}

/// Parse the `exclude = [...]` array from a Cargo.toml `[workspace]`
/// section. Same semantics and gating as `parse_workspace_members`:
/// the array is only recognised when nested directly under
/// `[workspace]`.
fn parse_workspace_excludes(content: &str) -> Vec<String> {
    parse_workspace_array(content, "exclude")
}

/// Generic helper for parsing a single named array under `[workspace]`.
fn parse_workspace_array(content: &str, key: &str) -> Vec<String> {
    let mut in_workspace_section = false;
    let mut in_array = false;
    let mut buf = String::new();
    for raw in content.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = parse_toml_section_header(line) {
            in_workspace_section = header == "workspace";
            if in_array {
                break;
            }
            continue;
        }
        if !in_workspace_section {
            continue;
        }
        if !in_array {
            if line.trim_start().starts_with(key) {
                if let Some(idx) = line.find('[') {
                    in_array = true;
                    buf.push_str(&line[idx + 1..]);
                    if line.contains(']') {
                        break;
                    }
                }
            }
        } else {
            buf.push(' ');
            buf.push_str(line);
            if line.contains(']') {
                break;
            }
        }
    }
    if let Some(idx) = buf.find(']') {
        buf.truncate(idx);
    }
    buf.split(',')
        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Match a workspace `members` pattern against a candidate manifest's
/// parent directory. Supports literal directory names and a single
/// trailing `*` wildcard (e.g. `crates/*`).
fn member_path_matches(pattern: &str, candidate_dir: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        if let Some(rest) = candidate_dir.strip_prefix(prefix) {
            let trimmed = rest.trim_start_matches('/');
            // Single component depth — `crates/foo` matches `crates/*`
            // but `crates/foo/bar` does not.
            !trimmed.is_empty() && !trimmed.contains('/')
        } else {
            false
        }
    } else {
        candidate_dir == pattern
    }
}

/// Internal: parse a single Cargo.toml at `path`. Returns an empty vec on
/// I/O or parse failure.
fn parse_cargo_dep_names(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_cargo_dep_names_from_text(&content)
}

/// Dependency-section header recognised by the parser. Includes both the
/// flat-table form (`[dependencies]`) and Cargo's table-form
/// (`[dependencies.ring]`, where `ring` is the dep name).
fn classify_cargo_section(header: &str) -> CargoSection {
    const PLAIN: &[&str] = &[
        "dependencies",
        "dev-dependencies",
        "build-dependencies",
        "workspace.dependencies",
    ];
    const TABLE_PREFIXES: &[&str] = &[
        "dependencies.",
        "dev-dependencies.",
        "build-dependencies.",
        "workspace.dependencies.",
    ];
    if PLAIN.contains(&header) {
        return CargoSection::DepFlat;
    }
    for prefix in TABLE_PREFIXES {
        if let Some(rest) = header.strip_prefix(prefix) {
            // The dep name is the segment between the prefix and the next
            // dot (Cargo allows nested keys like `dependencies.foo.path`).
            let dep = rest.split('.').next().unwrap_or("");
            if !dep.is_empty() {
                return CargoSection::DepTable(dep.to_string());
            }
        }
    }
    CargoSection::Other
}

enum CargoSection {
    DepFlat,
    DepTable(String),
    Other,
}

fn parse_cargo_dep_names_from_text(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_deps = false;
    for raw in content.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = parse_toml_section_header(line) {
            match classify_cargo_section(header) {
                CargoSection::DepFlat => in_deps = true,
                CargoSection::DepTable(name) => {
                    in_deps = false;
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
                CargoSection::Other => in_deps = false,
            }
            continue;
        }
        if !in_deps {
            continue;
        }
        let name: String = line
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() {
            let after = &line[name.len()..];
            if after.trim_start().starts_with('=') && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Extract top-level dependency names from a `Cargo.toml` at `repo_root`.
/// Reads `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`,
/// and `[workspace.dependencies]` sections via a small line-based parser.
/// Returns an empty list on any I/O or parsing failure — the indicator
/// detector treats missing dep names as a no-op and falls back to path
/// hints.
///
/// The parser is intentionally narrow: we only need the leading identifier
/// before `=` on each in-section line to drive crate-name pattern matching.
/// Aliased deps (`alias = { package = "real-name", ... }`) are extracted
/// using the alias, which is acceptable because the alias is what code
/// actually references.
pub fn extract_cargo_dependency_names(repo_root: &std::path::Path) -> Vec<String> {
    let cargo_toml = repo_root.join("Cargo.toml");
    let Ok(content) = std::fs::read_to_string(&cargo_toml) else {
        return Vec::new();
    };
    parse_cargo_dep_names_from_text(&content)
}

fn match_dep_pattern(deps: &[String], patterns: &[&str]) -> Option<String> {
    for d in deps {
        for p in patterns {
            // Exact match only — substring matching produces too many false
            // positives (e.g. "rsa" matching "tarsa-fs").
            if d == p {
                return Some(d.clone());
            }
        }
    }
    None
}

fn match_path_hint(paths: &[String], hints: &[&str]) -> Option<String> {
    // Each hint is encoded as either `/segment` (a single segment) or
    // `/segment/` (a multi-component prefix that must end at a `/`).
    // We require true segment boundaries on both ends so that:
    //   - `/auth` matches `src/auth/x.rs` and `auth/x.rs`,
    //   - `/auth` does NOT match `src/authentication.rs` or `authors.rs`,
    //   - `/migrations/` matches `db/migrations/0001.sql`.
    for p in paths {
        let lower = p.to_ascii_lowercase();
        for h in hints {
            if hint_matches_path(&lower, h) {
                return Some(p.clone());
            }
        }
    }
    None
}

fn hint_matches_path(lower_path: &str, hint: &str) -> bool {
    // Hints starting with `/` describe a segment boundary on the left.
    // The boundary on the right is either the end of the hint string
    // already (`/migrations/`) or another `/` immediately after the hint
    // body (`/auth`).
    let Some(hint_body) = hint.strip_prefix('/') else {
        // Defensive: hint without leading `/` is not segment-anchored.
        return lower_path.contains(hint);
    };
    if hint_body.ends_with('/') {
        // Already a multi-component prefix: require the segment immediately
        // before it (or start) and exact appearance of the prefix.
        let needle = format!("/{hint_body}");
        if lower_path.contains(&needle) {
            return true;
        }
        return lower_path.starts_with(hint_body);
    }
    // Single-segment hint: require `/<body>/` (nested) OR `<body>/` at the
    // start of the path. End-of-string is intentionally NOT a match: the
    // hint describes a directory segment, and source paths should always
    // continue past the directory name.
    let nested_with_slash = format!("/{hint_body}/");
    if lower_path.contains(&nested_with_slash) {
        return true;
    }
    let top_with_slash = format!("{hint_body}/");
    lower_path.starts_with(&top_with_slash)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_only_workspace_has_no_binary_target() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        std::fs::write(
            r.join("Cargo.toml"),
            "[workspace]\n# c\nmembers = [\n  \"crates/a\", # x\n  \"crates/b\",\n]\nexclude = [\"fuzz\"]\n",
        )
        .unwrap();
        for m in ["crates/a", "crates/b"] {
            std::fs::create_dir_all(r.join(m).join("src")).unwrap();
            std::fs::write(
                r.join(m).join("Cargo.toml"),
                "[package]\nname=\"x\"\n[lib]\n",
            )
            .unwrap();
            std::fs::write(r.join(m).join("src/lib.rs"), "").unwrap();
        }
        // A separate fuzz workspace with binaries does not count.
        std::fs::create_dir_all(r.join("fuzz")).unwrap();
        std::fs::write(
            r.join("fuzz/Cargo.toml"),
            "[workspace]\n[[bin]]\nname=\"f\"\n",
        )
        .unwrap();
        assert_eq!(cargo_root_has_binary_target(r), Some(false));
        // A member with src/main.rs does.
        std::fs::write(r.join("crates/b/src/main.rs"), "fn main(){}").unwrap();
        assert_eq!(cargo_root_has_binary_target(r), Some(true));
    }

    #[test]
    fn glob_members_and_bin_tables_are_seen() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        std::fs::write(
            r.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(r.join("crates/tool")).unwrap();
        std::fs::write(
            r.join("crates/tool/Cargo.toml"),
            "[package]\n[[bin]]\nname=\"t\"\n",
        )
        .unwrap();
        assert_eq!(cargo_root_has_binary_target(r), Some(true));
        assert_eq!(cargo_root_has_binary_target(&r.join("nope")), None);
    }
    use repolens_core::model::profile::{BuildSystem, EntryPoint, Framework, ModuleInfo};
    use std::collections::BTreeMap;

    fn make_profile(
        build: &str,
        ptype: ProjectType,
        deps: &[&str],
        entry_points: Vec<EntryPoint>,
    ) -> ProjectProfile {
        ProjectProfile {
            name: "demo".into(),
            build_system: Some(BuildSystem {
                name: build.into(),
                details: None,
                config_file: format!("{build}-conf"),
            }),
            frameworks: deps
                .iter()
                .map(|d| Framework {
                    name: (*d).to_string(),
                    version: "1.0.0".into(),
                    category: "library".into(),
                })
                .collect(),
            project_type: ptype,
            entry_points,
            description: None,
            language_distribution: BTreeMap::new(),
            modules: vec![ModuleInfo {
                name: "demo".into(),
                path: "src".into(),
                file_count: 1,
                loc: 100,
                primary_language: "rust".into(),
            }],
        }
    }

    #[test]
    fn rust_library_tags_no_indicators() {
        let p = make_profile("cargo", ProjectType::Library, &[], vec![]);
        let ctx = RepoContextDetector::detect(&p, &["src/lib.rs".into(), "Cargo.toml".into()]);
        assert!(ctx.has(&RepoContextTag::RustLibrary));
        assert!(!ctx.any_indicator());
        assert!(ctx.any_rust());
        assert!(ctx.notes.is_empty());
    }

    #[test]
    fn rust_binary_with_entry_point_tagged() {
        let p = make_profile(
            "cargo",
            ProjectType::Cli,
            &[],
            vec![EntryPoint {
                entry_type: "binary".into(),
                path: "src/main.rs".into(),
                description: "main".into(),
            }],
        );
        let ctx = RepoContextDetector::detect(&p, &["src/main.rs".into()]);
        assert!(ctx.has(&RepoContextTag::RustBinary));
        assert!(!ctx.has(&RepoContextTag::RustLibrary));
    }

    #[test]
    fn crypto_indicators_emit_dep_match_note() {
        let p = make_profile("cargo", ProjectType::Library, &["ring", "serde"], vec![]);
        let ctx = RepoContextDetector::detect(&p, &["src/lib.rs".into()]);
        assert!(ctx.has(&RepoContextTag::CryptoIndicators));
        assert!(ctx.any_indicator());
        assert_eq!(ctx.notes.len(), 1);
        assert!(ctx.notes[0].note.contains("ring"));
        assert!(ctx.notes[0].note.contains("Suggestive"));
    }

    #[test]
    fn crypto_indicators_path_match_when_no_dep() {
        let p = make_profile("cargo", ProjectType::Library, &[], vec![]);
        let ctx = RepoContextDetector::detect(&p, &["src/crypto/mod.rs".into()]);
        assert!(ctx.has(&RepoContextTag::CryptoIndicators));
        assert_eq!(ctx.notes.len(), 1);
        assert!(ctx.notes[0].note.contains("/crypto"));
    }

    #[test]
    fn no_indicators_for_csv_parser_repo() {
        let p = make_profile(
            "cargo",
            ProjectType::Library,
            &["csv", "serde", "anyhow"],
            vec![],
        );
        let ctx = RepoContextDetector::detect(&p, &["src/lib.rs".into(), "src/parser.rs".into()]);
        // "parser" path matches CompilerIndicators path hint — that's expected and
        // documents why path hints are conservative. Dep names should NOT match.
        assert!(!ctx.has(&RepoContextTag::CryptoIndicators));
        assert!(!ctx.has(&RepoContextTag::SecuritySensitiveIndicators));
        assert!(!ctx.has(&RepoContextTag::DatabaseIndicators));
    }

    /// Regression for the 2026-05-11 dogfood pass: a dev tool that uses sha2
    /// for content addressing (build artifact integrity, package manifests,
    /// cache keys) is not doing cryptography. Bare hash crates are too
    /// dual-use to count as a CryptoIndicators signal on their own.
    #[test]
    fn bare_hash_deps_do_not_imply_crypto() {
        for hash_dep in &["sha2", "sha3", "blake2", "blake3"] {
            let p = make_profile("cargo", ProjectType::Library, &[hash_dep], vec![]);
            let ctx = RepoContextDetector::detect(&p, &["src/lib.rs".into()]);
            assert!(
                !ctx.has(&RepoContextTag::CryptoIndicators),
                "bare '{hash_dep}' dep must not trigger CryptoIndicators on its own"
            );
        }
    }

    /// Regression for the 2026-05-11 dogfood pass: dev tools that use
    /// tree-sitter / syn / quote / proc-macro2 / nom for parsing or proc-macro
    /// work are not compilers. Every derive macro in the ecosystem pulls in
    /// syn+quote+proc-macro2 — that's not a useful compiler signal.
    #[test]
    fn generic_parsing_deps_do_not_imply_compiler() {
        for parsing_dep in &[
            "syn",
            "quote",
            "proc-macro2",
            "tree-sitter",
            "nom",
            "logos",
            "chumsky",
            "pest",
        ] {
            let p = make_profile("cargo", ProjectType::Library, &[parsing_dep], vec![]);
            let ctx = RepoContextDetector::detect(&p, &["src/lib.rs".into()]);
            assert!(
                !ctx.has(&RepoContextTag::CompilerIndicators),
                "bare '{parsing_dep}' dep must not trigger CompilerIndicators on its own"
            );
        }
    }

    /// Genuine compiler signals — parser generators and codegen backends with
    /// no reasonable non-compiler use — still trigger CompilerIndicators.
    #[test]
    fn strong_compiler_deps_still_trigger() {
        for compiler_dep in &["lalrpop", "lalrpop-util", "pest_derive", "cranelift"] {
            let p = make_profile("cargo", ProjectType::Library, &[compiler_dep], vec![]);
            let ctx = RepoContextDetector::detect(&p, &["src/lib.rs".into()]);
            assert!(
                ctx.has(&RepoContextTag::CompilerIndicators),
                "'{compiler_dep}' should still trigger CompilerIndicators"
            );
        }
    }

    #[test]
    fn database_indicators_via_dep_or_path() {
        let p1 = make_profile("cargo", ProjectType::Library, &["sqlx"], vec![]);
        assert!(RepoContextDetector::detect(&p1, &[]).has(&RepoContextTag::DatabaseIndicators));

        let p2 = make_profile("cargo", ProjectType::Library, &[], vec![]);
        let ctx = RepoContextDetector::detect(&p2, &["db/migrations/0001.sql".into()]);
        assert!(ctx.has(&RepoContextTag::DatabaseIndicators));
    }

    #[test]
    fn no_std_indicators_via_dep() {
        let p = make_profile("cargo", ProjectType::Library, &["heapless"], vec![]);
        let ctx = RepoContextDetector::detect(&p, &[]);
        assert!(ctx.has(&RepoContextTag::NoStdIndicators));
    }

    #[test]
    fn java_service_from_webapp() {
        let p = make_profile("maven", ProjectType::WebApp, &[], vec![]);
        let ctx = RepoContextDetector::detect(&p, &[]);
        assert!(ctx.has(&RepoContextTag::JavaService));
    }

    #[test]
    fn node_library_vs_application() {
        let lib = make_profile("npm", ProjectType::Library, &[], vec![]);
        assert!(RepoContextDetector::detect(&lib, &[]).has(&RepoContextTag::NodeLibrary));

        let app = make_profile("pnpm", ProjectType::WebApp, &[], vec![]);
        assert!(RepoContextDetector::detect(&app, &[]).has(&RepoContextTag::NodeApplication));
    }

    #[test]
    fn indicator_tags_dedup_when_dep_and_path_both_match() {
        let p = make_profile("cargo", ProjectType::Library, &["ring"], vec![]);
        let ctx = RepoContextDetector::detect(&p, &["src/crypto/mod.rs".into()]);
        // Tag should appear exactly once, but two notes (dep + path) recorded.
        let crypto_tag_count = ctx
            .tags
            .iter()
            .filter(|t| **t == RepoContextTag::CryptoIndicators)
            .count();
        assert_eq!(crypto_tag_count, 1);
        assert!(ctx.notes.len() >= 2);
    }

    #[test]
    fn no_build_system_detected_yields_only_indicators() {
        let mut p = make_profile("cargo", ProjectType::Unknown, &[], vec![]);
        p.build_system = None;
        let ctx = RepoContextDetector::detect(&p, &["src/auth/oauth.rs".into()]);
        // No project-shape tag, but auth path hint fires.
        assert!(!ctx.has(&RepoContextTag::RustLibrary));
        assert!(!ctx.has(&RepoContextTag::RustBinary));
        assert!(ctx.has(&RepoContextTag::SecuritySensitiveIndicators));
    }

    // -- Regression tests for the manifest-deps fix (Codex P2) ----------

    #[test]
    fn cargo_root_is_workspace_detects_section() {
        use std::fs;
        let workspace_dir = tempfile::tempdir().unwrap();
        fs::write(
            workspace_dir.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        assert!(cargo_root_is_workspace(workspace_dir.path()));

        let single_dir = tempfile::tempdir().unwrap();
        fs::write(
            single_dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert!(!cargo_root_is_workspace(single_dir.path()));

        // Workspace-only attributes (no top-level `[workspace]`) should
        // also count.
        let ws_attr_dir = tempfile::tempdir().unwrap();
        fs::write(
            ws_attr_dir.path().join("Cargo.toml"),
            "[workspace.dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        assert!(cargo_root_is_workspace(ws_attr_dir.path()));

        // Missing Cargo.toml → false (graceful).
        let missing = tempfile::tempdir().unwrap();
        assert!(!cargo_root_is_workspace(missing.path()));
    }

    #[test]
    fn cargo_root_is_workspace_handles_inline_comments() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace] # the root workspace\nresolver = \"2\"\n",
        )
        .unwrap();
        assert!(
            cargo_root_is_workspace(dir.path()),
            "[workspace] with trailing comment must still detect as workspace"
        );
    }

    #[test]
    fn extract_cargo_dependency_names_recursive_walks_member_manifests() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        // Root Cargo.toml has only `[workspace]`, no deps of its own.
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/api\", \"crates/db\"]\n",
        )
        .unwrap();
        // Member `api` declares `ring`.
        fs::create_dir_all(dir.path().join("crates/api")).unwrap();
        fs::write(
            dir.path().join("crates/api/Cargo.toml"),
            "[package]\nname = \"api\"\n[dependencies]\nring = \"0.17\"\n",
        )
        .unwrap();
        // Member `db` declares `sqlx`.
        fs::create_dir_all(dir.path().join("crates/db")).unwrap();
        fs::write(
            dir.path().join("crates/db/Cargo.toml"),
            "[package]\nname = \"db\"\n[dependencies]\nsqlx = \"0.7\"\n",
        )
        .unwrap();

        let manifest_paths = vec![
            "Cargo.toml".to_string(),
            "crates/api/Cargo.toml".to_string(),
            "crates/db/Cargo.toml".to_string(),
            "crates/api/src/lib.rs".to_string(), // non-manifest, must be skipped
        ];
        let names = extract_cargo_dependency_names_recursive(dir.path(), &manifest_paths);
        assert!(
            names.contains(&"ring".to_string()),
            "must pick up ring from crates/api, got {names:?}"
        );
        assert!(
            names.contains(&"sqlx".to_string()),
            "must pick up sqlx from crates/db, got {names:?}"
        );
    }

    #[test]
    fn extract_cargo_dependency_names_recursive_skips_non_member_manifests() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        // Root declares `members = ["crates/*"]`. fixtures/foo is NOT
        // a declared member — its deps must NOT propagate.
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        // Member crates/api with `ring` — should be picked up.
        fs::create_dir_all(dir.path().join("crates/api")).unwrap();
        fs::write(
            dir.path().join("crates/api/Cargo.toml"),
            "[package]\nname = \"api\"\n[dependencies]\nring = \"0.17\"\n",
        )
        .unwrap();
        // Non-member fixtures/foo with `sqlx` — should be IGNORED.
        fs::create_dir_all(dir.path().join("fixtures/foo")).unwrap();
        fs::write(
            dir.path().join("fixtures/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\n[dependencies]\nsqlx = \"0.7\"\n",
        )
        .unwrap();

        let manifest_paths = vec![
            "Cargo.toml".into(),
            "crates/api/Cargo.toml".into(),
            "fixtures/foo/Cargo.toml".into(),
        ];
        let names = extract_cargo_dependency_names_recursive(dir.path(), &manifest_paths);
        assert!(
            names.contains(&"ring".to_string()),
            "must include member dep ring, got {names:?}"
        );
        assert!(
            !names.contains(&"sqlx".to_string()),
            "must NOT include non-member fixture dep sqlx, got {names:?}"
        );
    }

    #[test]
    fn extract_cargo_dependency_names_recursive_rejects_unsafe_paths() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        // Absolute path, parent escape, and Windows-style prefix all rejected.
        let manifest_paths = vec![
            "Cargo.toml".into(),
            "/etc/Cargo.toml".into(),
            "../escape/Cargo.toml".into(),
            "C:/evil/Cargo.toml".into(),
        ];
        // No member matches and no path passes safety, so we should return
        // just the root deps (none here).
        let names = extract_cargo_dependency_names_recursive(dir.path(), &manifest_paths);
        assert!(names.is_empty(), "got unexpected names {names:?}");
    }

    #[test]
    fn member_path_matches_handles_glob_and_literal() {
        assert!(super::member_path_matches("crates/*", "crates/foo"));
        assert!(!super::member_path_matches("crates/*", "crates/foo/bar")); // depth>1
        assert!(!super::member_path_matches("crates/*", "fixtures/foo"));
        assert!(super::member_path_matches("crates/api", "crates/api"));
        assert!(!super::member_path_matches("crates/api", "crates/db"));
    }

    #[test]
    fn parse_workspace_members_ignores_members_outside_workspace_section() {
        // A `members = [...]` array under [package.metadata] (or any
        // non-workspace section) must not be treated as workspace members.
        let content = "[package]\nname = \"demo\"\n\n\
                       [package.metadata.bundle]\nmembers = [\"fixtures/foo\"]\n";
        let members = super::parse_workspace_members(content);
        assert!(
            members.is_empty(),
            "members under [package.metadata.*] must not count as workspace members; got {members:?}"
        );
    }

    #[test]
    fn parse_cargo_dep_names_handles_table_form_headers() {
        let content = "[package]\nname = \"x\"\n\n\
                       [dependencies.ring]\nversion = \"0.17\"\n\n\
                       [dev-dependencies.foo]\nversion = \"1\"\n\n\
                       [build-dependencies]\ncc = \"1\"\n\n\
                       [workspace.dependencies.serde]\nversion = \"1\"\n";
        let names = super::parse_cargo_dep_names_from_text(content);
        assert!(names.contains(&"ring".to_string()), "got {names:?}");
        assert!(names.contains(&"foo".to_string()), "got {names:?}");
        assert!(names.contains(&"cc".to_string()), "got {names:?}");
        assert!(names.contains(&"serde".to_string()), "got {names:?}");
    }

    #[test]
    fn extract_cargo_dependency_names_recursive_respects_workspace_excludes() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/fixture\"]\n",
        )
        .unwrap();
        // Both candidates match members = ["crates/*"]. The fixture is
        // explicitly excluded.
        fs::create_dir_all(dir.path().join("crates/api")).unwrap();
        fs::write(
            dir.path().join("crates/api/Cargo.toml"),
            "[package]\nname = \"api\"\n[dependencies]\nring = \"0.17\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("crates/fixture")).unwrap();
        fs::write(
            dir.path().join("crates/fixture/Cargo.toml"),
            "[package]\nname = \"fixture\"\n[dependencies]\nsqlx = \"0.7\"\n",
        )
        .unwrap();

        let manifest_paths = vec![
            "Cargo.toml".into(),
            "crates/api/Cargo.toml".into(),
            "crates/fixture/Cargo.toml".into(),
        ];
        let names = extract_cargo_dependency_names_recursive(dir.path(), &manifest_paths);
        assert!(names.contains(&"ring".to_string()), "got {names:?}");
        assert!(
            !names.contains(&"sqlx".to_string()),
            "must respect workspace.exclude; got {names:?}"
        );
    }

    #[test]
    fn parse_workspace_members_handles_multi_line_array() {
        let content = "[workspace]\nmembers = [\n    \"crates/api\",\n    \"crates/*\",\n]\n";
        let members = super::parse_workspace_members(content);
        assert_eq!(
            members,
            vec!["crates/api".to_string(), "crates/*".to_string()]
        );
    }

    #[test]
    fn extract_cargo_dependency_names_handles_inline_comments() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[dependencies] # core deps\nring = \"0.17\"\nserde = \"1\" # serialization\n",
        )
        .unwrap();
        let names = extract_cargo_dependency_names(dir.path());
        assert!(
            names.contains(&"ring".to_string()),
            "expected ring even with header comment, got {names:?}"
        );
        assert!(names.contains(&"serde".to_string()));
    }

    #[test]
    fn detect_full_only_tags_workspace_when_explicitly_signalled() {
        let p = make_profile("cargo", ProjectType::Library, &[], vec![]);
        // A nested fixture Cargo.toml is present in file_paths but the
        // root manifest does NOT declare a workspace. detect_full should
        // not tag RustWorkspace.
        let file_paths = vec![
            "Cargo.toml".to_string(),
            "fixtures/inputs/rust-small/Cargo.toml".to_string(),
            "src/lib.rs".to_string(),
        ];
        let ctx_no_ws = RepoContextDetector::detect_full(&p, &file_paths, &[], false);
        assert!(!ctx_no_ws.has(&RepoContextTag::RustWorkspace));

        // When the caller explicitly proves a workspace, the tag fires.
        let ctx_ws = RepoContextDetector::detect_full(&p, &file_paths, &[], true);
        assert!(ctx_ws.has(&RepoContextTag::RustWorkspace));
    }

    #[test]
    fn path_hint_matcher_enforces_segment_boundaries() {
        // `/auth` must match path segments `auth`, not unrelated tokens.
        const HINTS: &[&str] = &["/auth"];
        // Positives: nested segment, top-level segment.
        assert!(super::match_path_hint(&["src/auth/foo.rs".into()], HINTS).is_some());
        assert!(super::match_path_hint(&["auth/oauth.rs".into()], HINTS).is_some());
        // Negatives: substring of a different identifier.
        assert!(super::match_path_hint(&["src/authors.rs".into()], HINTS).is_none());
        assert!(super::match_path_hint(&["src/authentication.rs".into()], HINTS).is_none());
        assert!(super::match_path_hint(&["authors/list.rs".into()], HINTS).is_none());

        // Multi-component prefix `/migrations/` matches both nested and top-level.
        const MIG: &[&str] = &["/migrations/"];
        assert!(super::match_path_hint(&["db/migrations/0001.sql".into()], MIG).is_some());
        assert!(super::match_path_hint(&["migrations/0001.sql".into()], MIG).is_some());
        // Negative: a path ending in `/migrations` (no trailing slash) — not a directory segment.
        assert!(super::match_path_hint(&["src/migrations".into()], MIG).is_none());
    }

    #[test]
    fn extract_cargo_dependency_names_parses_dependencies_section() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nring = \"0.17\"\nserde = { version = \"1\", features = [\"derive\"] }\n\
             # commented = \"0.1\"\n\n\
             [dev-dependencies]\ntempfile = \"3\"\n\n\
             [build-dependencies]\ncc = \"1\"\n",
        )
        .unwrap();
        let names = extract_cargo_dependency_names(dir.path());
        assert!(names.contains(&"ring".to_string()), "got {names:?}");
        assert!(names.contains(&"serde".to_string()), "got {names:?}");
        assert!(names.contains(&"tempfile".to_string()), "got {names:?}");
        assert!(names.contains(&"cc".to_string()), "got {names:?}");
        assert!(!names.contains(&"commented".to_string()));
    }

    #[test]
    fn extract_cargo_dependency_names_returns_empty_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let names = extract_cargo_dependency_names(dir.path());
        assert!(names.is_empty());
    }

    #[test]
    fn python_build_system_name_maps_to_python_tags() {
        // ProfileAnalyzer emits "Python" for pyproject.toml/setup.py.
        let lib = make_profile("Python", ProjectType::Library, &[], vec![]);
        assert!(RepoContextDetector::detect(&lib, &[]).has(&RepoContextTag::PythonPackage));

        let script = make_profile("Python", ProjectType::Cli, &[], vec![]);
        assert!(RepoContextDetector::detect(&script, &[]).has(&RepoContextTag::PythonScript));
    }

    #[test]
    fn manifest_deps_drive_indicator_tags_when_frameworks_silent() {
        // Profile has zero frameworks (the case Codex flagged: real Cargo
        // crates rarely register as "frameworks" — that field is for
        // Spring/React/Django).
        let p = make_profile("cargo", ProjectType::Library, &[], vec![]);
        let manifest_deps = vec![
            "ring".to_string(),
            "sqlx".to_string(),
            "heapless".to_string(),
        ];
        let ctx = RepoContextDetector::detect_with_manifest_deps(
            &p,
            &["src/lib.rs".into()],
            &manifest_deps,
        );
        assert!(
            ctx.has(&RepoContextTag::CryptoIndicators),
            "expected CryptoIndicators from manifest dep 'ring'"
        );
        assert!(
            ctx.has(&RepoContextTag::DatabaseIndicators),
            "expected DatabaseIndicators from manifest dep 'sqlx'"
        );
        assert!(
            ctx.has(&RepoContextTag::NoStdIndicators),
            "expected NoStdIndicators from manifest dep 'heapless'"
        );
        // Each indicator must record the literal match in notes.
        assert!(ctx.notes.iter().any(|m| m.note.contains("ring")));
        assert!(ctx.notes.iter().any(|m| m.note.contains("sqlx")));
        assert!(ctx.notes.iter().any(|m| m.note.contains("heapless")));
    }
}
