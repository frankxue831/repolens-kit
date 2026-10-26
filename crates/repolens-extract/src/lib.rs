use repolens_core::model::file_entry::Symbol;
use std::path::Path;

pub mod fidelity;
pub mod lang;
pub mod registry;
pub mod relationship;

pub(crate) mod ts_util;

pub mod go_rel;
pub mod go_symbol;
pub mod java_rel;
pub mod java_symbol;
pub mod js_rel;
pub mod js_symbol;
pub mod py_rel;
pub mod py_symbol;
pub mod rust_rel;
pub mod rust_symbol;

pub use fidelity::{extraction_fidelity, ExtractionFidelity};
pub use relationship::{Relationship, TYPE_IMPORT};

/// Extracts symbols (classes, functions, methods, etc.) from a source file.
pub trait SymbolExtractor: Send + Sync {
    /// Returns symbols found in the file. Empty vec on parse failure.
    fn extract(&self, path: &Path) -> Vec<Symbol>;

    /// Extract from an already-parsed tree + its source. Tree-sitter extractors
    /// override this; non-tree-sitter (Python) keep the default and are only
    /// reached via the path-based `extract`.
    fn extract_from_tree(&self, _tree: &tree_sitter::Tree, _source: &str) -> Vec<Symbol> {
        Vec::new()
    }
}

/// Extracts relationships (imports, extends, calls, etc.) from a source file.
pub trait RelationshipExtractor: Send + Sync {
    /// Returns relationships found in the file. Empty vec on parse failure.
    fn extract(&self, path: &Path) -> Vec<Relationship>;

    /// Extract from an already-parsed tree + its source. Tree-sitter extractors
    /// override this; non-tree-sitter (Python) keep the default and are only
    /// reached via the path-based `extract`.
    fn extract_from_tree(&self, _tree: &tree_sitter::Tree, _source: &str) -> Vec<Relationship> {
        Vec::new()
    }
}
