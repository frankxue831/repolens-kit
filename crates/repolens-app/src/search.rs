use anyhow::Result;
use tracing::info;

use crate::discover::PackResolver;
use crate::scan_inputs::DEFAULT_MAX_FILE_BYTES;
use repolens_core::path_safety::read_packed_file_capped;

/// A path-mode search result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PathResult {
    pub path: String,
    pub language: String,
}

/// A content-mode search result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentResult {
    pub path: String,
    pub line_number: usize,
    pub context: String,
}

/// A symbol-mode search result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolResult {
    pub path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified: Option<String>,
    /// 1-based declaration line, passed through from the pack symbol when
    /// the extractor captured it (Rust only today).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Searches pack entries by path, content, or symbol.
pub struct SearchService {
    resolver: PackResolver,
}

impl SearchService {
    pub fn new(resolver: PackResolver) -> Self {
        Self { resolver }
    }

    pub fn search_path(
        &self,
        pack: Option<&str>,
        root: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PathResult>> {
        let (_, index) = self.resolver.resolve(pack, root)?;
        let query_lower = query.to_lowercase();
        info!(query = query, mode = "path", "Searching");

        let results: Vec<PathResult> = index
            .entries
            .iter()
            .filter(|e| e.included && e.path.to_lowercase().contains(&query_lower))
            .take(limit)
            .map(|e| PathResult {
                path: e.path.clone(),
                language: e.language.clone(),
            })
            .collect();

        info!(results = results.len(), "Path search complete");
        Ok(results)
    }

    pub fn search_content(
        &self,
        pack: Option<&str>,
        root: Option<&str>,
        query: &str,
        limit: usize,
        regex: bool,
    ) -> Result<Vec<ContentResult>> {
        let (pack_dir, index) = self.resolver.resolve(pack, root)?;
        info!(query = query, mode = "content", regex = regex, "Searching");

        let matcher: Box<dyn Fn(&str) -> bool> = if regex {
            let re =
                regex::Regex::new(query).map_err(|e| anyhow::anyhow!("Invalid regex: {}", e))?;
            Box::new(move |line: &str| re.is_match(line))
        } else {
            let query_lower = query.to_lowercase();
            Box::new(move |line: &str| line.to_lowercase().contains(&query_lower))
        };

        let mut results = Vec::new();

        for entry in index.entries.iter().filter(|e| e.included) {
            if results.len() >= limit {
                break;
            }
            let Some(cp) = entry.content_path.as_deref() else {
                continue;
            };
            let content =
                match read_packed_file_capped(&pack_dir, cp, DEFAULT_MAX_FILE_BYTES as u64) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

            for (i, line) in content.lines().enumerate() {
                if results.len() >= limit {
                    break;
                }
                if matcher(line) {
                    results.push(ContentResult {
                        path: entry.path.clone(),
                        line_number: i + 1,
                        context: line.trim().to_string(),
                    });
                }
            }
        }

        info!(results = results.len(), "Content search complete");
        Ok(results)
    }

    pub fn search_symbol(
        &self,
        pack: Option<&str>,
        root: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SymbolResult>> {
        let (_, index) = self.resolver.resolve(pack, root)?;
        let query_lower = query.to_lowercase();
        info!(query = query, mode = "symbol", "Searching");

        let mut results = Vec::new();

        for entry in index.entries.iter().filter(|e| e.included) {
            if results.len() >= limit {
                break;
            }
            for sym in &entry.symbols {
                if results.len() >= limit {
                    break;
                }
                if sym.name.to_lowercase().contains(&query_lower) {
                    results.push(SymbolResult {
                        path: entry.path.clone(),
                        symbol_name: sym.name.clone(),
                        symbol_type: sym.symbol_type.clone(),
                        qualified: sym.qualified_name.clone(),
                        line: sym.line,
                    });
                }
            }
        }

        info!(results = results.len(), "Symbol search complete");
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::{FileEntry, Symbol};
    use std::fs;
    use std::path::PathBuf;

    fn make_pack_on_disk(dir: &std::path::Path) -> (PathBuf, PackResolver) {
        let pack_dir = dir.join("pack");
        fs::create_dir_all(pack_dir.join("files/src")).unwrap();

        let java_content = "package com.example;\n\npublic class App {\n    public void run() {\n        System.out.println(\"hello\");\n    }\n}\n";
        fs::write(pack_dir.join("files/src/App.java"), java_content).unwrap();
        fs::write(pack_dir.join("files/README.md"), "# Hello World\n").unwrap();

        let entries = vec![
            FileEntry {
                path: "src/App.java".to_string(),
                sha256: "abc123".to_string(),
                language: "Java".to_string(),
                loc: 7,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/App.java".to_string()),
                symbols: vec![
                    Symbol {
                        symbol_type: "class".to_string(),
                        name: "App".to_string(),
                        qualified_name: Some("com.example.App".to_string()),
                        visibility: Some("public".to_string()),
                        line: Some(3),
                    },
                    Symbol {
                        symbol_type: "method".to_string(),
                        name: "run".to_string(),
                        qualified_name: Some("com.example.App.run".to_string()),
                        visibility: Some("public".to_string()),
                        line: None,
                    },
                ],
            },
            FileEntry {
                path: "README.md".to_string(),
                sha256: "def456".to_string(),
                language: "Markdown".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/README.md".to_string()),
                symbols: vec![],
            },
            FileEntry {
                path: "build.log".to_string(),
                sha256: String::new(),
                language: String::new(),
                loc: 0,
                included: false,
                reason: "excluded:gitignore".to_string(),
                content_path: None,
                symbols: vec![],
            },
        ];

        // Use a numeric timestamp that JavaEpochSeconds can deserialize
        let manifest = serde_json::json!({
            "schemaVersion": "1.0",
            "schemaUrl": "",
            "timestamp": 1774753241.321919,
            "chunks": ["files-0001.json"]
        });
        fs::write(
            pack_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            pack_dir.join("files-0001.json"),
            serde_json::to_string_pretty(&entries).unwrap(),
        )
        .unwrap();

        let resolver = PackResolver::new();
        (pack_dir, resolver)
    }

    #[test]
    fn search_path_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_path(Some(pack_dir.to_str().unwrap()), None, "app", 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/App.java");
        assert_eq!(results[0].language, "Java");
    }

    #[test]
    fn search_path_excludes_non_included() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_path(Some(pack_dir.to_str().unwrap()), None, "build", 10)
            .unwrap();
        assert!(
            results.is_empty(),
            "excluded files should not appear in path search"
        );
    }

    #[test]
    fn search_path_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_path(Some(pack_dir.to_str().unwrap()), None, "", 1)
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_content_substring() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_content(Some(pack_dir.to_str().unwrap()), None, "hello", 10, false)
            .unwrap();
        assert_eq!(results.len(), 2); // App.java ("hello") + README.md ("Hello World")
        assert!(results.iter().any(|r| r.path == "src/App.java"));
    }

    #[test]
    fn search_content_regex() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_content(
                Some(pack_dir.to_str().unwrap()),
                None,
                r"public\s+class",
                10,
                true,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/App.java");
        assert_eq!(results[0].line_number, 3);
    }

    #[test]
    fn search_content_invalid_regex_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let err = service
            .search_content(Some(pack_dir.to_str().unwrap()), None, "[invalid", 10, true)
            .unwrap_err();
        assert!(err.to_string().contains("Invalid regex"));
    }

    #[test]
    fn search_symbol_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_symbol(Some(pack_dir.to_str().unwrap()), None, "App", 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "App");
        assert_eq!(results[0].symbol_type, "class");
        assert_eq!(results[0].qualified, Some("com.example.App".to_string()));
        // Pass-through from the pack symbol (the fixture sets line: Some(3)).
        assert_eq!(results[0].line, Some(3));
    }

    #[test]
    fn search_symbol_line_none_serializes_without_key() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        // "run" has no line in the fixture — the wire shape must omit the
        // key entirely (absent, not null), matching the pack convention.
        let results = service
            .search_symbol(Some(pack_dir.to_str().unwrap()), None, "run", 10)
            .unwrap();
        assert_eq!(results[0].line, None);
        let json = serde_json::to_value(&results[0]).unwrap();
        assert!(
            json.get("line").is_none(),
            "None line must omit the key: {json}"
        );
    }

    #[test]
    fn search_symbol_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_symbol(Some(pack_dir.to_str().unwrap()), None, "run", 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "run");
    }

    #[test]
    fn search_symbol_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (pack_dir, resolver) = make_pack_on_disk(dir.path());
        let service = SearchService::new(resolver);
        let results = service
            .search_symbol(Some(pack_dir.to_str().unwrap()), None, "", 1)
            .unwrap();
        assert_eq!(results.len(), 1);
    }
}
