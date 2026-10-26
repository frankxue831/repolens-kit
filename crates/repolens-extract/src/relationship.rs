/// A relationship extracted from a source file (import, extends, calls, etc.).
///
/// Internal to `repolens-extract` — consumed by the graph builder in Phase 5,
/// not serialized directly into pack output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Relationship type: "imports", "type_import" (Python import inside
    /// `if TYPE_CHECKING:`), "extends", "implements", "calls", "type_ref",
    /// or "declares" (Rust module declarations)
    pub rel_type: String,
    /// Target of the relationship (unresolved simple name or import path).
    pub target_name: String,
    /// Additional context: full import path, method signature, etc.
    pub detail: String,
}

/// Relationship type for a type-only import: a Python import inside
/// `if TYPE_CHECKING:` or a TypeScript `import type` / `export type ... from`.
/// It still shows the target file is used, but it never runs, so cycle
/// detection leaves it out.
pub const TYPE_IMPORT: &str = "type_import";
