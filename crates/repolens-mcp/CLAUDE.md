# repolens-mcp guidance

Loaded when working under `crates/repolens-mcp/`.

## MCP Server

Uses `#[tool_router]` macro from `rmcp`. Each tool takes a `Parameters<T>` struct with `JsonSchema` for type-safe, self-documenting arguments. Tools return `CallToolResult` via `ok_json()`/`err_json()` helpers.

## Gotchas

- MCP param structs use per-field `#[serde(rename = "...")]` for camelCase fields, NOT `rename_all` on the struct — always add explicit rename for multi-word fields like `maxDepth`
- MCP integration tests that assert on envelope fields (`evidenceGrade`, `knowledgeLayer`, `confidence`, `provenance`) need `call_*_envelope` helpers — the default `call_*` helpers run through `extract_text` which auto-unwraps `.data`
