use crate::go_rel::GoRelationshipExtractor;
use crate::go_symbol::GoSymbolExtractor;
use crate::java_rel::JavaRelationshipExtractor;
use crate::java_symbol::JavaSymbolExtractor;
use crate::js_rel::JsRelationshipExtractor;
use crate::js_symbol::JsSymbolExtractor;
use crate::py_rel::PyRelationshipExtractor;
use crate::py_symbol::PySymbolExtractor;
use crate::rust_rel::RustRelationshipExtractor;
use crate::rust_symbol::RustSymbolExtractor;
use crate::{Relationship, RelationshipExtractor, SymbolExtractor};
use repolens_core::model::file_entry::Symbol;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Registry that maps language names to symbol extractors.
pub struct SymbolExtractorRegistry {
    extractors: HashMap<&'static str, Arc<dyn SymbolExtractor>>,
}

impl SymbolExtractorRegistry {
    pub fn new() -> Self {
        let mut extractors: HashMap<&'static str, Arc<dyn SymbolExtractor>> = HashMap::new();
        extractors.insert("Java", Arc::new(JavaSymbolExtractor::new()));
        let js: Arc<dyn SymbolExtractor> = Arc::new(JsSymbolExtractor::new());
        extractors.insert("JavaScript", Arc::clone(&js));
        extractors.insert("TypeScript", js);
        extractors.insert("Python", Arc::new(PySymbolExtractor));
        extractors.insert("Go", Arc::new(GoSymbolExtractor::new()));
        extractors.insert("Rust", Arc::new(RustSymbolExtractor::new()));
        Self { extractors }
    }

    pub fn for_language(&self, language: &str) -> Option<&dyn SymbolExtractor> {
        self.extractors.get(language).map(|e| e.as_ref())
    }
}

impl Default for SymbolExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry that maps language names to relationship extractors.
pub struct RelationshipExtractorRegistry {
    extractors: HashMap<&'static str, Arc<dyn RelationshipExtractor>>,
}

impl RelationshipExtractorRegistry {
    pub fn new() -> Self {
        let mut extractors: HashMap<&'static str, Arc<dyn RelationshipExtractor>> = HashMap::new();
        extractors.insert("Java", Arc::new(JavaRelationshipExtractor::new()));
        let js: Arc<dyn RelationshipExtractor> = Arc::new(JsRelationshipExtractor::new());
        extractors.insert("JavaScript", Arc::clone(&js));
        extractors.insert("TypeScript", js);
        extractors.insert("Python", Arc::new(PyRelationshipExtractor));
        extractors.insert("Go", Arc::new(GoRelationshipExtractor::new()));
        extractors.insert("Rust", Arc::new(RustRelationshipExtractor::new()));
        Self { extractors }
    }

    pub fn for_language(&self, language: &str) -> Option<&dyn RelationshipExtractor> {
        self.extractors.get(language).map(|e| e.as_ref())
    }
}

impl Default for RelationshipExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined symbol + relationship extractors that can parse a file **once** and
/// run both walks on the shared tree-sitter tree.
pub struct Extractors {
    symbols: SymbolExtractorRegistry,
    relationships: RelationshipExtractorRegistry,
}

impl Extractors {
    pub fn new() -> Self {
        Self {
            symbols: SymbolExtractorRegistry::new(),
            relationships: RelationshipExtractorRegistry::new(),
        }
    }

    /// The underlying symbol registry (for callers that only need symbols).
    pub fn symbols(&self) -> &SymbolExtractorRegistry {
        &self.symbols
    }

    /// Parse-once extraction. For a tree-sitter language, reads + parses the file
    /// a **single time** and runs both the symbol and relationship walks on the
    /// shared tree; returns `(symbols, Some(relationships))` — the `Some` (even
    /// when empty) tells the caller the relationships are authoritative and graph
    /// build need not re-extract them.
    ///
    /// For a non-tree-sitter language (e.g. Python) or on read/parse failure,
    /// symbols come from the path-based extractor and relationships are `None`,
    /// signalling graph build to extract them from the packed copy itself.
    pub fn extract_symbols_and_relationships(
        &self,
        path: &Path,
        language: &str,
    ) -> (Vec<Symbol>, Option<Vec<Relationship>>) {
        if let Ok(source) = std::fs::read_to_string(path) {
            if let Some(tree) = crate::ts_util::parse_for(language, &source) {
                let symbols = self
                    .symbols
                    .for_language(language)
                    .map(|e| e.extract_from_tree(&tree, &source))
                    .unwrap_or_default();
                let relationships = self
                    .relationships
                    .for_language(language)
                    .map(|e| e.extract_from_tree(&tree, &source))
                    .unwrap_or_default();
                return (symbols, Some(relationships));
            }
        }
        let symbols = self
            .symbols
            .for_language(language)
            .map(|e| e.extract(path))
            .unwrap_or_default();
        (symbols, None)
    }
}

impl Default for Extractors {
    fn default() -> Self {
        Self::new()
    }
}
