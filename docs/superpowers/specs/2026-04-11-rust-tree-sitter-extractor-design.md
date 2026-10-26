# Rust Tree-Sitter Extractor — Design Spec

**Date**: 2026-04-11
**Status**: Revised
**Scope**: Replace regex-based Rust symbol and relationship extractors with tree-sitter AST parsing
**Primary goal**: Improve Rust extraction accuracy and graph quality without changing the current Rust pack contract more than necessary

## Overview

Replace the regex-based Rust extractors in `repolens-extract` with `tree-sitter-rust` parsers. Keep the existing trait interfaces, registry wiring, file locations, and wire models stable. The first iteration focuses on parser correctness and better relationships, while preserving current symbol shapes where the rest of the repo already has expectations.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Migration style | In-place rewrite of `rust_symbol.rs` and `rust_rel.rs` | Lowest integration cost; no new public API |
| Parser ownership | Store `tree_sitter::Language`; create a fresh `Parser` per `extract()` call | Matches the Java extractor pattern and keeps the extractor structs `Send + Sync` |
| Symbol contract | Preserve current `name` and `qualified_name` shapes for existing cases (`serve`, `Server.new`) | Avoids breaking current tests and downstream assumptions |
| Relationship expansion | Add AST-backed `type_ref` plus better `imports`, `implements`, and `declares` extraction | Improves graph quality where regex is weak today |
| Type-ref noise filter | Filter primitives plus a curated set of ubiquitous std/prelude container types | Prevents the graph from filling with low-value unresolved edges like `String`, `Vec`, and `Option` |
| Dedup granularity | Dedup `imports` by normalized `detail`; dedup other relationship kinds by `(rel_type, target_name)` | Keeps graph metrics stable while preserving distinct import paths |
| Scope limit | No `calls`, macro tracking, enum-variant symbols, or signature-bearing symbol names in v1 | Keeps the parser migration separate from a contract redesign |

## In Scope

- Tree-sitter-backed extraction for the current Rust symbol surface:
  - root `function_item`, `struct_item`, `enum_item`, `trait_item`, `type_item`, `const_item`, `static_item`, `mod_item`
  - methods declared inside `impl_item`
- Relationship extraction for `imports`, `implements`, `type_ref`, `declares`
- Recursive `use` tree flattening including aliases, wildcards, `self`, and nested groups
- Visibility parsing for `pub`, `pub(crate)`, `pub(super)`, `pub(in ...)`, and private items
- Type-reference extraction from function params and returns, named fields, tuple fields, type aliases, const/static annotations, impl headers, and trait method signatures
- Tests that lock in the extractor contract and the new graph-relevant behavior

## Out of Scope

- `calls` relationship extraction
- Changes to the `Symbol` or `Relationship` data models
- Signature-based `qualified_name` values such as `serve(u16) -> Result`
- Standalone trait-method symbols from `function_signature_item`
- Module-qualified symbol names for inline `mod { ... }` bodies
- Enum variant symbols, struct field symbols, macro/derive tracking, or `extern crate` modeling

## Dependencies

Add to workspace `Cargo.toml`:

```toml
tree-sitter-rust = "0.24"
```

Add to `crates/repolens-extract/Cargo.toml`:

```toml
tree-sitter-rust = { workspace = true }
```

This stays aligned with the workspace's existing `tree-sitter = "0.24"` dependency line.

## Files Modified

| File | Change |
|------|--------|
| `Cargo.toml` | Add `tree-sitter-rust` to workspace dependencies |
| `crates/repolens-extract/Cargo.toml` | Add `tree-sitter-rust = { workspace = true }` |
| `crates/repolens-extract/src/rust_symbol.rs` | Replace regex scanner with AST-backed symbol extraction |
| `crates/repolens-extract/src/rust_rel.rs` | Replace regex scanner with AST-backed relationship extraction |
| `crates/repolens-extract/src/registry.rs` | Instantiate Rust extractors with `::new()` and `Default`, matching Java |
| `crates/repolens-extract/tests/rust_extract_test.rs` | Preserve current contract tests and add AST-specific coverage |
| `crates/repolens-app/src/graph.rs` tests | Add one Rust type-ref integration test if needed to lock in graph resolution |

**No production changes required in** `repolens-app`, `repolens-core`, `lang.rs`, `ts_util.rs`, or the pack schema. Existing Rust graph-resolution strategies are reused as-is.

## Architecture

Use the same lifecycle as the Java extractors:

```rust
pub struct RustSymbolExtractor {
    language: tree_sitter::Language,
}

impl RustSymbolExtractor {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
        }
    }
}
```

Apply the same pattern to `RustRelationshipExtractor`, with `Default` implemented via `new()`.

Execution flow for each `extract()`:

1. Read the file to string; on I/O error log `warn!` and return `Vec::new()`.
2. Create a fresh `tree_sitter::Parser`.
3. `set_language(&self.language)` and parse the source.
4. On parse failure log `warn!` and return `Vec::new()`.
5. Walk the AST in source order and emit deterministic results.

Use `crate::ts_util::node_text()` for UTF-8 extraction. Keep Rust-specific helper logic private to `rust_symbol.rs` and `rust_rel.rs`.

## Symbol Extraction

### Traversal Boundary

To preserve the current contract, symbol extraction remains root-item oriented:

- inspect the source file's root declarations in source order
- emit symbols for matching root items
- recurse into `impl_item.body` only to collect nested method `function_item`s
- do not emit extra standalone symbols from inline module bodies in v1

This keeps the current test shape intact while still replacing the brittle regex parser.

### Node Mapping

| Node kind | Emitted `symbol_type` | Name source | `qualified_name` |
|-----------|-----------------------|-------------|------------------|
| root `function_item` | `function` | `name` field | bare function name |
| `struct_item` | `class` | `name` field | type name |
| `enum_item` | `enum` | `name` field | type name |
| `trait_item` | `interface` | `name` field | type name |
| `type_item` | `type` | `name` field | type name |
| `const_item` | `field` | `name` field | item name |
| `static_item` | `field` | `name` field | item name |
| `mod_item` | `module` | `name` field | module name |
| `function_item` inside `impl_item` | `function` | `name` field | `TypeName.method` |

### Impl Handling

For each `impl_item`:

1. Read the `type` field.
2. Normalize it to the receiver type's right-most identifier for naming purposes.
   - `Server` -> `Server`
   - `crate::net::Server` -> `Server`
   - `Server<T>` -> `Server`
3. Walk `body` and emit nested `function_item` nodes as methods with `qualified_name = Some(format!("{impl_type}.{name}"))`.

Trait impls (`impl Trait for Type`) still qualify methods by the concrete type, not the trait name.

### Visibility

Resolve visibility from the optional `visibility_modifier` child:

- `pub` -> `public`
- `pub(crate)` -> `crate`
- `pub(super)` -> `restricted`
- `pub(in ...)` -> `restricted`
- absent -> `private`

This expands coverage for Rust-only visibility forms without changing existing public/private cases.

## Relationship Extraction

Relationship extraction walks the full AST because imports and type usage may appear in nested declarations even when symbol emission stays root-scoped.

### Relationship Types

| Source | `rel_type` | `target_name` | `detail` |
|--------|------------|---------------|----------|
| `use_declaration` leaf | `imports` | imported name or alias | normalized full use path |
| `impl_item` with `trait` field | `implements` | trait simple name | textual trait path |
| local type usage | `type_ref` | referenced type simple name | textual referenced type |
| `mod_item` with no `body` | `declares` | module name | module name |

### Import Flattening

Handle the real Rust `use` grammar rather than line-based patterns:

- `use_declaration` field `argument`
- `scoped_use_list { path, list }`
- `use_list`
- `use_as_clause { path, alias }`
- `use_wildcard`
- plain `identifier`, `scoped_identifier`, `crate`, `self`, and `super`

Normalization rules:

- `use crate::config::Settings;` -> target `Settings`, detail `crate::config::Settings`
- `use foo::Bar as Baz;` -> target `Baz`, detail `foo::Bar`
- `use foo::*;` -> target `*`, detail `foo`
- `use crate::model::{self, Symbol};` expands to:
  - target `model`, detail `crate::model`
  - target `Symbol`, detail `crate::model::Symbol`

Filtering rules:

- skip imports whose first path segment is `std`, `core`, or `alloc`
- keep workspace-crate and third-party imports
- preserve `crate::`, `super::`, and `self::` paths verbatim in `detail`, because the graph builder already resolves those forms

### Type-Reference Collection

Collect `type_ref` relationships from these contexts:

- `function_item` and `function_signature_item`: parameter types and return types
- `struct_item` and `enum_variant`: named fields and tuple fields
- `type_item`, `const_item`, `static_item`: annotated `type` field
- `impl_item`: concrete `type` and optional `trait` field

Traverse recursively through Rust type nodes such as:

- `type_identifier`
- `scoped_type_identifier`
- `generic_type`
- `reference_type`
- `tuple_type`
- `array_type`

Extraction rules:

- `target_name` is the right-most referenced type identifier
  - `crate::foo::Bar` -> `Bar`
  - `Vec<Foo>` -> `Vec` and `Foo`
- `detail` is the textual referenced type for that specific emitted relationship
- skip `Self` and `self`
- skip primitive types
- skip a curated `COMMON_RUST_TYPES` set, initially including:
  - `String`, `str`, `Vec`, `VecDeque`, `Option`, `Result`, `Box`
  - `HashMap`, `HashSet`, `BTreeMap`, `BTreeSet`
  - `Path`, `PathBuf`, `Arc`, `Rc`, `Mutex`, `RwLock`

The goal is to keep project-meaningful type edges while suppressing standard-library noise.

### Deduplication

Deduplicate before returning from the extractor so graph metrics do not inflate:

- `imports`: key on `(rel_type, detail)`
- `implements`, `type_ref`, `declares`: key on `(rel_type, target_name)`

This is intentionally stricter than the Java helper because Rust imports rely on `detail` for resolution.

## Graph Builder Compatibility

This design is constrained by the graph builder's current Rust resolution logic:

- `declares.target_name` must remain the bare module name because `GraphBuilder` converts it to `{mod}.rs` and `{mod}/mod.rs`
- `imports.detail` must remain the full Rust module path because `GraphBuilder` already resolves:
  - `crate::...`
  - `super::...`
  - `self::...`
  - workspace-crate imports like `repolens_core::model::FileEntry`
- `type_ref.target_name` should remain a simple type name because Rust currently has no import-map resolution path equivalent to Java's

Because of those constraints, this spec intentionally defers signature-based qualified names and fully module-qualified symbol names to a later contract change.

## Testing Strategy

### Existing Tests

All current tests in `crates/repolens-extract/tests/rust_extract_test.rs` should continue to pass unchanged. That is a hard requirement for this phase.

### New Extractor Tests

Add targeted tests for AST-only behavior the regex extractor could not handle reliably:

| Test | What it validates |
|------|-------------------|
| `pub_crate_visibility` | `pub(crate)` maps to `crate` |
| `pub_super_visibility` | `pub(super)` maps to `restricted` |
| `generic_impl_method_keeps_type_name` | `impl Foo<T>` still yields `Foo.method` |
| `scoped_impl_method_keeps_type_name` | `impl crate::net::Foo` still yields `Foo.method` |
| `type_ref_from_function_params` | local parameter types emit `type_ref` |
| `type_ref_from_return_type` | local return types emit `type_ref` |
| `type_ref_from_named_struct_fields` | named field types emit `type_ref` |
| `type_ref_from_tuple_struct_fields` | tuple struct fields from `ordered_field_declaration_list` emit `type_ref` |
| `type_ref_from_trait_method_signature` | trait method signatures contribute type refs without emitting extra symbols |
| `type_ref_filters_common_std_types` | `String`, `Vec`, `Option`, etc. do not create noisy edges |
| `use_as_alias` | alias import preserves alias in `target_name` and original path in `detail` |
| `nested_use_groups` | recursively flattened grouped imports produce one relationship per leaf |
| `grouped_use_self_leaf` | `use crate::model::{self, Symbol}` emits `crate::model` and `crate::model::Symbol` |
| `use_wildcard` | wildcard import emits `target_name = "*"` and detail path prefix |

### Graph Test

If the new `type_ref` output is not already covered indirectly, add one Rust graph test in `crates/repolens-app/src/graph.rs`:

- source file references a local Rust type in a field or function signature
- target file defines that type
- resulting graph contains a resolved `type_ref` edge

That test locks in the fact that the new extractor output is useful to the existing graph builder without requiring graph-builder production changes.

### Validation Commands

Run:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-extract rust_extract
cargo test -p repolens-app rust_small_fixture_produces_correct_graph
cargo test -p repolens-app rust_
cargo fmt --all -- --check
```

## Manual Validation

After implementation:

1. Run `cargo run -- scan --root .`
2. Inspect the newest `.repolens/pack-*/files-*.json` chunk for Rust symbols
3. Inspect the same pack's `graph.json` for Rust `type_ref`, `imports`, and `declares` edges
4. Confirm that:
   - current Rust symbol shapes are unchanged for existing cases
   - grouped imports and workspace-crate imports still resolve
   - new `type_ref` edges appear for project-local Rust types
   - standard-library noise is not dominating unresolved edges

`project-profile.json` is not the right validation artifact for this work; the pack chunks and graph output are.
