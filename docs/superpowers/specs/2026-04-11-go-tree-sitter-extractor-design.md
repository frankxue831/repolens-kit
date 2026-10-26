# Go Tree-Sitter Extractor Design

**Date:** 2026-04-11
**Status:** Approved
**Scope:** Replace both regex-based Go extractors (symbol + relationship) with tree-sitter AST parsing

## Goal

Migrate the Go symbol and relationship extractors from regex to tree-sitter, matching current extraction behavior with improved accuracy. This is the third tree-sitter migration (after Java/Rust and JS/TS) and completes coverage for 5 of 6 supported languages.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scope | Both symbol + relationship extractors | Same rationale as JS/TS — one grammar dependency, avoid inconsistency |
| Grammar handling | One struct, one `Language` field | Go has a single grammar, no variants |
| Relationship types | `imports` and `extends` only | Match current behavior; `type_ref` deferred — Go's simpler type system doesn't need it yet |
| Import filtering | Same as current: stdlib + external | Well-tested existing logic, no changes needed |
| Test fixtures | New `go-small` fixture directory | Current fixture too thin for struct/interface/method/embedding coverage |

## Architecture

### Struct Design

Each extractor stores a single `tree_sitter::Language`:

```rust
pub struct GoSymbolExtractor {
    language: tree_sitter::Language,  // tree_sitter_go::LANGUAGE
}

pub struct GoRelationshipExtractor {
    language: tree_sitter::Language,
}
```

Same pattern as the Rust extractor. Registry shares one `Arc` instance for the "Go" key.

### Files Touched

- **Modify:** `go_symbol.rs` — rewrite with tree-sitter
- **Modify:** `go_rel.rs` — rewrite with tree-sitter
- **Modify:** `registry.rs` — update constructors (`GoSymbolExtractor::new()`, `GoRelationshipExtractor::new()`)
- **Modify:** `Cargo.toml` (workspace root) — add `tree-sitter-go`
- **Modify:** `crates/repolens-extract/Cargo.toml` — add grammar dependency
- **Create:** `fixtures/inputs/go-small/` — 4 files (`go.mod`, `main.go`, `server/server.go`, `server/model.go`)
- **Modify:** `crates/repolens-extract/tests/go_extract_test.rs` — update constructors + new tests

### What Stays the Same

- `SymbolExtractor` and `RelationshipExtractor` traits — no changes
- Registry pattern — one `Arc` for "Go" key
- `ts_util::node_text()` — reused as-is
- Language detection in `lang.rs` — unchanged
- `Relationship` struct — unchanged
- `crates/repolens-app/src/graph.rs` — no changes needed (Go import resolution already works)

## Symbol Extractor

### Symbol Types

| AST Node | Symbol Type | Notes |
|----------|-------------|-------|
| `function_declaration` | `function` | Top-level functions; name from `name` field |
| `method_declaration` | `function` | Methods; receiver type from `receiver` field's `parameter_list` |
| `type_declaration` → `type_spec` where type is `struct_type` | `class` | Maps struct to class for Java contract parity |
| `type_declaration` → `type_spec` where type is `interface_type` | `interface` | |

### Visibility

Go's visibility rule: name starts with uppercase character → `public`, otherwise `private`.

```rust
fn go_visibility(name: &str) -> String {
    if name.starts_with(|c: char| c.is_uppercase()) {
        "public".to_string()
    } else {
        "private".to_string()
    }
}
```

Same helper as current implementation — no change in behavior.

### Qualified Names

- Top-level functions: qualified name equals simple name (e.g., `main`, `NewServer`)
- Methods: `ReceiverType.MethodName` (e.g., `Server.Start`, `server.stop`) — same as current regex extractor. Receiver type extracted from the first parameter in the `receiver` field, stripping `*` for pointer receivers.

### Traversal

Walk `source_file` top-level children only. Go doesn't have nested function declarations (closures are expressions, not declarations), so no traversal boundary concern.

### Method Receiver Extraction

The `method_declaration` node has a `receiver` field containing a `parameter_list`. The first parameter's type is the receiver type. For pointer receivers (`*Server`), strip the `*` prefix to get the base type name.

AST structure:
```
method_declaration
  receiver: parameter_list
    parameter_declaration
      name: identifier (receiver variable name, e.g., "s")
      type: pointer_type or type_identifier
  name: field_identifier (method name)
```

Extract the rightmost `type_identifier` from the receiver's type, handling `pointer_type` wrapping.

## Relationship Extractor

### Relationship Types

| AST Node | Rel Type | Target | Detail |
|----------|----------|--------|--------|
| `import_declaration` → `import_spec` with non-stdlib, non-external path | `imports` | `myapp/config` | `myapp/config` |
| Struct body field with no name (embedding) | `extends` | `BaseServer` | `BaseServer` |

### Import Filtering

Same logic as current regex extractor:

1. **External packages:** First path segment contains `.` (e.g., `github.com/pkg/errors`) → filtered
2. **Standard library:** First path segment is in `GO_STDLIB` set → filtered
3. Everything else → kept as intra-module import

The `GO_STDLIB` set is the same ~45 package set from the current implementation.

### Struct Embedding Detection

In Go's tree-sitter grammar, struct fields are represented as `field_declaration` nodes. An embedded field has no explicit field name — it only has a type. Concretely:

```go
type Server struct {
    BaseServer       // embedded — field_declaration with type but no name
    *Logger          // pointer-embedded — field_declaration with pointer_type, no name
    Addr string      // regular field — field_declaration with name + type
}
```

Detection: iterate `field_declaration_list` children. If a `field_declaration` has a type child but no `name` field, it's an embedding. Strip `*` for pointer embeddings.

### Deduplication

Same pattern as Rust/JS/TS — `HashSet<String>` keyed by:
- `{rel_type}:{detail}` for `imports`
- `{rel_type}:{target_name}` for `extends`

## Test Fixtures

### New `fixtures/inputs/go-small/`

```
fixtures/inputs/go-small/
├── go.mod              # module declaration
├── main.go             # main package, imports from internal package
└── server/
    ├── server.go       # package server: Server struct, Handler interface, pointer + value receivers
    └── model.go        # package server: struct embedding, unexported types, nested struct
```

**Coverage per file:**

- `go.mod` — `module gosmall`
- `main.go` — `package main`, imports `gosmall/server`, top-level `main()` and `setup()` functions
- `server/server.go` — `package server`, `Server` struct with `Addr string` field, `Handler` interface, methods `(s *Server) Start()` and `(s Server) Stop()`
- `server/model.go` — `package server`, `BaseModel` struct, `ExtendedModel` struct embedding `BaseModel` and `*Logger`, unexported `config` struct

## Testing Strategy

### Three Layers

1. **Backward-compatible tests** — same assertions as current `go_extract_test.rs` against `mixed-small` fixture. Proves migration doesn't break existing behavior.

2. **Tree-sitter-specific tests** against `go-small`:
   - Methods with pointer and value receivers
   - Struct embedding (including pointer embedding)
   - Interface extraction
   - Unexported types
   - Import filtering (stdlib, external, intra-module)

3. **Manual validation** — `cargo run -- scan --root fixtures/inputs/go-small` then inspect pack.

### Symbol Test Cases

- Function extracted with correct visibility
- Struct extracted as `class`
- Interface extracted as `interface`
- Method with pointer receiver has `Server.Start` qualified name
- Method with value receiver in `go-small` has `Server.Stop` qualified name
- Backward-compatible test still preserves the existing `server.stop` case from the temp-file fixture
- Unexported function/struct/interface → `private` visibility
- Exported function/struct/interface → `public` visibility
- Parse failure returns empty vec

### Relationship Test Cases

- Intra-module import extracted
- Stdlib import filtered
- External package filtered (dot in first segment)
- Struct embedding extracted as `extends`
- Pointer embedding (`*Logger`) extracted as `extends` with `Logger` target
- Regular field NOT extracted as embedding
- Import deduplication works
- Parse failure returns empty vec

## Dependencies

Add to workspace `Cargo.toml`:
```toml
tree-sitter-go = "0.23"
```

Add to `crates/repolens-extract/Cargo.toml`:
```toml
tree-sitter-go = { workspace = true }
```

## Out of Scope

| Item | Reason |
|------|--------|
| `type_ref` relationship type | Go's type system is simpler; deferred to future work |
| Type alias extraction (`type Foo = Bar`) | Low value, rare in practice |
| Const/var declarations | Not extracted by current regex either |
| Generic type parameters (Go 1.18+) | Adds complexity, defer until needed |
