use crate::SymbolExtractor;
use once_cell::sync::Lazy;
use regex::Regex;
use repolens_core::model::file_entry::Symbol;
use std::path::Path;
use tracing::warn;

/// Regex-based symbol extractor for Python files.
///
/// Only extracts top-level definitions (no leading whitespace).
pub struct PySymbolExtractor;

// Top-level class: class Foo or class Foo(Bar):
static CLASS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^class\s+(\w+)").unwrap());

// Top-level function: def foo(
static FUNC_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^def\s+(\w+)\(").unwrap());

// ALL_CAPS constant: MAX_SIZE = ...
static CONST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^([A-Z][A-Z_0-9]+)\s*=").unwrap());

// Dunder variable: __version__ = ...
static DUNDER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(__\w+__)\s*=").unwrap());

// Underscore-prefixed private variable: _cache = ...
static PRIVATE_VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(_\w+)\s*=").unwrap());

// Type-annotated variable: config: Config = ...
static TYPED_VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\w+)\s*:\s*\w+\s*=").unwrap());

impl SymbolExtractor for PySymbolExtractor {
    fn extract(&self, path: &Path) -> Vec<Symbol> {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to read {}: {e}", path.display());
                return Vec::new();
            }
        };

        let mut symbols = Vec::new();

        for line in source.lines() {
            // Only match top-level definitions (no leading whitespace)
            if line.starts_with(' ') || line.starts_with('\t') || line.is_empty() {
                continue;
            }

            if let Some(caps) = CLASS_RE.captures(line) {
                let name = caps[1].to_string();
                symbols.push(Symbol {
                    symbol_type: "class".to_string(),
                    name: name.clone(),
                    qualified_name: Some(name),
                    visibility: Some("public".to_string()),
                    line: None,
                });
            } else if let Some(caps) = FUNC_RE.captures(line) {
                let name = caps[1].to_string();
                let visibility = py_visibility(&name);
                symbols.push(Symbol {
                    symbol_type: "function".to_string(),
                    name: name.clone(),
                    qualified_name: Some(name),
                    visibility: Some(visibility),
                    line: None,
                });
            } else if let Some(caps) = DUNDER_RE.captures(line) {
                // Check dunder before private_var since __x__ starts with _
                let name = caps[1].to_string();
                symbols.push(Symbol {
                    symbol_type: "variable".to_string(),
                    name: name.clone(),
                    qualified_name: Some(name),
                    visibility: Some("public".to_string()),
                    line: None,
                });
            } else if let Some(caps) = CONST_RE.captures(line) {
                let name = caps[1].to_string();
                symbols.push(Symbol {
                    symbol_type: "variable".to_string(),
                    name: name.clone(),
                    qualified_name: Some(name),
                    visibility: Some("public".to_string()),
                    line: None,
                });
            } else if let Some(caps) = PRIVATE_VAR_RE.captures(line) {
                let name = caps[1].to_string();
                symbols.push(Symbol {
                    symbol_type: "variable".to_string(),
                    name: name.clone(),
                    qualified_name: Some(name),
                    visibility: Some("private".to_string()),
                    line: None,
                });
            } else if let Some(caps) = TYPED_VAR_RE.captures(line) {
                let name = caps[1].to_string();
                symbols.push(Symbol {
                    symbol_type: "variable".to_string(),
                    name: name.clone(),
                    qualified_name: Some(name),
                    visibility: Some("public".to_string()),
                    line: None,
                });
            }
        }

        symbols
    }
}

fn py_visibility(name: &str) -> String {
    if name.starts_with("__") && name.ends_with("__") {
        "public".to_string() // dunder methods
    } else if name.starts_with('_') {
        "private".to_string()
    } else {
        "public".to_string()
    }
}
