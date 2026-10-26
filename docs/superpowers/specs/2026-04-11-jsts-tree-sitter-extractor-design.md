# JS/TS Tree-Sitter Extractor Design

**Date:** 2026-04-11
**Status:** Approved
**Scope:** Replace both regex-based JS/TS extractors (symbol + relationship) with tree-sitter AST parsing

## Goal

Migrate the JavaScript/TypeScript symbol and relationship extractors from regex to tree-sitter, adding `implements` and `type_ref` relationship types. This completes the tree-sitter migration for the second-most important language pair (after Java) and brings JS/TS extraction quality in line with Java and Rust.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scope | Both symbol + relationship extractors | One grammar dependency, avoid inconsistency |
| Grammar handling | One struct, two `Language` fields, pick at parse time | Matches current registry pattern (shared instance), DRY |
| Relationship types | `imports`, `extends`, `implements`, `type_ref` | `calls` deferred — noisier, harder to get right |
| Import filtering | Relative paths only (`./`, `../`) | Package imports don't resolve to repo files |
| Test fixtures | New `js-small` fixture directory | Realistic coverage for tree-sitter-specific behavior |

## Architecture

### Grammar Selection

Each extractor stores two `tree_sitter::Language` values:

```rust
pub struct JsSymbolExtractor {
    js_language: tree_sitter::Language,   // tree_sitter_javascript::LANGUAGE
    ts_language: tree_sitter::Language,   // tree_sitter_typescript::LANGUAGE_TSX
}

pub struct JsRelationshipExtractor {
    js_language: tree_sitter::Language,
    ts_language: tree_sitter::Language,
}
```

At parse time, the extractor checks the file extension:
- `.js`, `.jsx`, `.mjs`, `.cjs` → use `js_language` (tree-sitter-javascript handles JSX natively)
- `.ts`, `.tsx`, `.mts`, `.cts` → use `ts_language` (TSX grammar is a superset of TS, safe for all TypeScript files)

### Files Touched

- **Modify:** `js_symbol.rs` — rewrite with tree-sitter
- **Modify:** `js_rel.rs` — rewrite with tree-sitter
- **Modify:** `registry.rs` — update constructors (`JsSymbolExtractor::new()`, `JsRelationshipExtractor::new()`)
- **Modify:** `Cargo.toml` (workspace root) — add `tree-sitter-javascript`, `tree-sitter-typescript`
- **Modify:** `crates/repolens-extract/Cargo.toml` — add grammar dependencies
- **Modify:** `crates/repolens-app/src/graph.rs` — add `"type"` to symbol index filter, add `.cjs`/`.mts`/`.cts` to `JS_EXTENSIONS`
- **Create:** `fixtures/inputs/js-small/` — 5 files + package.json
- **Modify:** `crates/repolens-extract/tests/js_extract_test.rs` — expanded tests

### Graph Builder Changes

Two small changes to `crates/repolens-app/src/graph.rs` to support the new extraction:

1. **Add `"type"` to qualified-name index filter** (line ~79): The current `matches!` only indexes `class | interface | enum | record | annotation`. Add `"type"` so that `type_ref` relationships targeting TypeScript type aliases can resolve. Without this, `type_ref` to `type Config = ...` would stay permanently unresolved.

2. **Add missing extensions to `JS_EXTENSIONS`** (line ~29): Current list is `[".js", ".jsx", ".ts", ".tsx", ".mjs"]`. Add `".cjs"`, `".mts"`, `".cts"` so extensionless imports into those module variants resolve correctly.

### What Stays the Same

- `SymbolExtractor` and `RelationshipExtractor` traits — no changes
- Registry pattern — one shared `Arc` per extractor for JS + TS keys
- `ts_util::node_text()` — reused as-is
- Language detection in `lang.rs` — unchanged
- `Relationship` struct — unchanged

## Symbol Extractor

### Symbol Types

| AST Node | Symbol Type | Notes |
|----------|-------------|-------|
| `function_declaration` | `function` | |
| `class_declaration` | `class` | |
| `abstract_class_declaration` (TS) | `class` | Treated identically to `class_declaration` |
| `interface_declaration` (TS) | `interface` | |
| `type_alias_declaration` (TS) | `type` | |
| `enum_declaration` (TS) | `enum` | |
| `lexical_declaration` → `variable_declarator` where `value` is `arrow_function` or `function_expression` | `function` | `const foo = () => ...` or `const foo = function() ...` |
| `method_definition` inside class body | `method` | Applies to both `class_declaration` and `abstract_class_declaration` bodies |

### Visibility

Two patterns signal `public` visibility:

1. **ES6 export:** Declaration wrapped in `export_statement` → `public`
2. **CommonJS export:** Symbol assigned to `module.exports` or `exports.<name>` → `public`

All other declarations → `module-private`.

Detection for CommonJS: check if the file contains `module.exports` or `exports.` assignment statements. If a function/class name matches the RHS of such an assignment, mark it `public`. This is a best-effort heuristic — CommonJS exports are dynamic and can't always be resolved statically. For `module.exports = { foo, bar }` shorthand, extract the property names.

### Qualified Names

- Top-level symbols: qualified name equals simple name (e.g., `UserService`)
- Methods: `ClassName.methodName` (e.g., `UserService.getUser`) — same pattern as Rust's `ImplType.method`

### Traversal Boundary

The symbol extractor walks **top-level `program` children and `class_body` children only**. It does not recurse into function bodies, arrow function bodies, or other nested scopes. This prevents over-emitting nested helpers, callbacks, and locally-scoped functions.

Concretely:
- Walk `program.named_children()` → extract declarations
- If a child is a `class_declaration`, walk its `class_body.named_children()` → extract methods
- If a child is an `export_statement`, unwrap and check its `declaration` child
- Do **not** recurse into `function_declaration` bodies, `arrow_function` bodies, `if_statement`, `for_statement`, etc.

### Improvements Over Regex

- Methods extracted (regex missed them — line-by-line scanning couldn't see class bodies)
- Nested declarations handled correctly (explicit traversal boundary prevents over-emission)
- No false positives from comments or strings

## Relationship Extractor

### Relationship Types

| AST Node | Rel Type | Target | Detail |
|----------|----------|--------|--------|
| `import_statement` with relative source | `imports` | `./util` | `./util` |
| `call_expression` where callee is `require` with relative string arg | `imports` | `../config` | `../config` |
| `export_statement` with `source` field containing relative path | `imports` | `./types` | `./types` |
| Side-effect import (`import './polyfill'`) | `imports` | `./polyfill` | `./polyfill` |
| `class_declaration` or `abstract_class_declaration` with `extends` clause | `extends` | `BaseClass` | `BaseClass` |
| `class_declaration` or `abstract_class_declaration` with `implements` clause (TS) | `implements` | `Serializable` | `Serializable` |
| `interface_declaration` with `extends_type_clause` (TS) | `extends` | `BaseInterface` | `BaseInterface` |
| Type annotations: function params, return types, variable declarations, class fields | `type_ref` | `UserService` | `UserService` |

### Import Filtering

Relative paths only — `./` or `../` prefix required. Package imports (`react`, `lodash`, `@angular/core`) are ignored because they don't resolve to files in the scanned repository.

### Type Reference Filtering

Skip common built-in types to reduce graph noise. Filtered set:

**JS built-ins:** `String`, `Number`, `Boolean`, `Object`, `Array`, `Function`, `Symbol`, `RegExp`, `Error`, `Date`, `Map`, `Set`, `WeakMap`, `WeakSet`, `Promise`

**TS built-ins:** `Partial`, `Required`, `Readonly`, `Record`, `Pick`, `Omit`, `Exclude`, `Extract`, `ReturnType`, `Parameters`, `InstanceType`

**Primitives:** `string`, `number`, `boolean`, `void`, `never`, `any`, `unknown`, `null`, `undefined`, `bigint`

**Single-letter generics:** `T`, `K`, `V`, `U`, `E`, `R`

### Deduplication

Same pattern as Rust extractor — `HashSet<String>` keyed by:
- `{rel_type}:{detail}` for `imports`
- `{rel_type}:{target_name}` for `extends`, `implements`, `type_ref`

## Test Fixtures

### New `fixtures/inputs/js-small/`

```
fixtures/inputs/js-small/
├── package.json
├── src/
│   ├── app.ts          # Main entry: imports, type annotations, arrow functions
│   ├── service.ts      # extends BaseService, implements Disposable, typed fields
│   ├── types.ts        # Interface, type alias, enum declarations; re-export
│   ├── base-service.js # Plain JS class with methods (tests JS grammar path)
│   └── util.js         # Exported functions, CommonJS require
```

**Coverage per file:**

- `app.ts` — ES6 relative imports, type annotations on params/returns, exported arrow functions
- `service.ts` — `extends BaseService`, `implements Disposable`, typed class fields, methods with type_ref
- `types.ts` — `interface Disposable`, `type Config = ...`, `enum Status`, re-export from `./base-service`
- `base-service.js` — Plain JS class with methods (validates JS grammar path)
- `util.js` — `module.exports`, CommonJS `require('./types')` (validates require extraction)

## Testing Strategy

### Three Layers

1. **Backward-compatible integration tests** — same assertions as current `js_extract_test.rs` against the `mixed-small` fixture. Proves migration doesn't break existing behavior.

2. **Tree-sitter-specific tests** against `js-small` — validate new behavior:
   - `implements` extraction
   - `type_ref` extraction with filtering
   - Method extraction with qualified names
   - JS vs TS grammar selection
   - Deduplication
   - Built-in type filtering

3. **Manual validation** — `cargo run -- scan --root fixtures/inputs/js-small` then inspect pack graph.

### Relationship Test Cases

- ES6 relative import extracted, package import skipped
- CommonJS `require` with relative path extracted
- Re-export from relative path extracted as `imports`
- `extends` captured from class declaration
- `extends` captured from abstract class declaration
- `extends` captured from interface with `extends_type_clause`
- `implements` captured from TypeScript class or abstract class
- `type_ref` from function parameter type annotation captured
- `type_ref` from return type annotation captured
- `type_ref` for built-in type (`Promise`, `string`, etc.) filtered out
- Single-letter generic (`T`) filtered out
- Dedup: same import path appears once even if imported multiple times
- JS file uses JS grammar, TS file uses TS grammar
- Parse failure returns empty vec (no panic)

### Symbol Test Cases

- Function, class, interface, type, enum extracted
- Methods extracted with `ClassName.methodName` qualified name
- Exported symbol → `public` visibility
- Non-exported symbol → `module-private` visibility
- Arrow function assigned to const detected as `function`
- Abstract class extracted as `class` with methods
- Nested function inside function body not extracted as top-level
- Class constructor extracted as method

## Dependencies

Add to workspace `Cargo.toml` (pinned to match `tree-sitter = "0.25"`, same pattern as `tree-sitter-java = "0.23"` and `tree-sitter-rust = "0.24"`):
```toml
tree-sitter-javascript = "0.23"
tree-sitter-typescript = "0.23"
```

Add to `crates/repolens-extract/Cargo.toml`:
```toml
tree-sitter-javascript = { workspace = true }
tree-sitter-typescript = { workspace = true }
```

## Out of Scope

| Item | Reason |
|------|--------|
| `calls` relationship type | Noisy, hard to filter; defer to future work |
| Package import extraction | Can't resolve to repo files |
| Dynamic `import()` expressions | Rare in practice, hard to resolve statically |
| JSX component references | Would require understanding JSX element → import mapping |
| Decorator extraction | TypeScript decorators are experimental, low priority |
