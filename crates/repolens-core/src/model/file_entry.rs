use serde::{Deserialize, Serialize};

/// A symbol extracted from a source file (class, method, function, etc.).
///
/// The `type` JSON key is mapped to `symbol_type` to avoid collision with the
/// Rust keyword.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    #[serde(rename = "type")]
    pub symbol_type: String,
    pub name: String,
    pub qualified_name: Option<String>,
    pub visibility: Option<String>,
    /// 1-based declaration line of the symbol in its source file.
    ///
    /// `None` for extractors that don't capture position yet (only the Rust
    /// tree-sitter extractor populates this) and for entries carried forward
    /// from packs written before this field existed — incremental scans
    /// re-extract changed files only, so mixed line coverage within one pack
    /// is expected until a full rescan.
    ///
    /// Serialized with `skip_serializing_if` so `None` emits no key, keeping
    /// pre-line pack bytes unchanged. The sibling `Option` fields above
    /// deliberately keep serializing `null` — changing their wire shape is
    /// out of scope for this additive field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Wire-model for a single entry in a pack chunk file (`files-*.json`).
///
/// Both included and excluded entries are present in chunk files.
/// Excluded entries have empty `sha256`/`language`, `loc = 0`, and a
/// `reason` of the form `"excluded:<rule>"`.
///
/// `symbols` defaults to an empty `Vec` when the JSON key is absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub language: String,
    pub loc: i32,
    pub included: bool,
    /// Always present in the wire format — never `Option<String>`.
    pub reason: String,
    pub content_path: Option<String>,
    #[serde(default)]
    pub symbols: Vec<Symbol>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol_with_line(line: Option<u32>) -> Symbol {
        Symbol {
            symbol_type: "function".to_string(),
            name: "serve".to_string(),
            qualified_name: Some("serve".to_string()),
            visibility: Some("public".to_string()),
            line,
        }
    }

    #[test]
    fn symbol_line_round_trips_when_some() {
        let json = serde_json::to_value(symbol_with_line(Some(7))).unwrap();
        assert_eq!(json["line"], 7);
        let back: Symbol = serde_json::from_value(json).unwrap();
        assert_eq!(back.line, Some(7));
    }

    #[test]
    fn symbol_line_none_emits_no_key() {
        let json = serde_json::to_value(symbol_with_line(None)).unwrap();
        assert!(
            json.get("line").is_none(),
            "None must omit the key entirely so pre-line pack bytes stay unchanged: {json}"
        );
    }

    #[test]
    fn symbol_without_line_key_deserializes_to_none() {
        // Legacy packs written before the field existed have no "line" key.
        let json = serde_json::json!({
            "type": "class",
            "name": "App",
            "qualifiedName": "com.example.App",
            "visibility": "public"
        });
        let sym: Symbol = serde_json::from_value(json).unwrap();
        assert_eq!(sym.line, None);
    }
}
