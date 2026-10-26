# Contract Sources

Java source files whose output format the Rust port must reproduce exactly.
Grouped by output category.

## Pack and Content Output

These files define the pack directory structure, manifest format, file entry schema,
symbol metadata, and the writing/reading logic.

- `src/main/java/com/repolens/output/Manifest.java` — manifest.json record shape
- `src/main/java/com/repolens/output/FileEntry.java` — file entry record in files-NNNN.json chunks
- `src/main/java/com/repolens/metadata/Symbol.java` — symbol metadata attached to file entries
- `src/main/java/com/repolens/output/PackWriter.java` — writes manifest, chunk files, content copies
- `src/main/java/com/repolens/report/PackReader.java` — reads manifest + chunk files back

## Graph Output

These files define the dependency graph format written to `graph.json` and `graph-metrics.json`.

- `src/main/java/com/repolens/graph/FileGraph.java` — top-level graph record (nodes + edges)
- `src/main/java/com/repolens/graph/GraphNode.java` — graph node (file path, language, symbols)
- `src/main/java/com/repolens/graph/GraphEdge.java` — graph edge (source, target, relationship type)
- `src/main/java/com/repolens/graph/GraphResolutionMetrics.java` — resolution stats in graph-metrics.json

## Session and Memory Output

These files define the persistent state written to `.repolens/session.json` and
`.repolens/memory/memory.json`.

- `src/main/java/com/repolens/core/RepoSession.java` — session record shape
- `src/main/java/com/repolens/core/RepoSessionStore.java` — session persistence logic
- `src/main/java/com/repolens/memory/MemoryGraph.java` — top-level memory graph (nodes + edges)
- `src/main/java/com/repolens/memory/MemoryNode.java` — memory node record
- `src/main/java/com/repolens/memory/MemoryEdge.java` — memory edge record
- `src/main/java/com/repolens/memory/MemoryType.java` — enum of memory node types
- `src/main/java/com/repolens/memory/MemorySource.java` — enum of memory provenance sources
- `src/main/java/com/repolens/memory/MemoryRelation.java` — enum of memory edge relations
- `src/main/java/com/repolens/memory/MemoryStore.java` — memory persistence logic

## Profile Output

- `src/main/java/com/repolens/profile/ProjectProfile.java` — project-profile.json record shape

## MCP Response Builders

These files define the MCP tool schemas and the response JSON shapes returned by
the MCP server over STDIO.

- `src/main/java/com/repolens/mcp/McpToolRegistry.java` — tool name/schema definitions
- `src/main/java/com/repolens/mcp/McpSearchHandler.java` — search_code response format
- `src/main/java/com/repolens/mcp/McpSummaryHandler.java` — get_summary response format

## Settings and Config

These files define settings resolution behavior that affects output (e.g., chunk size,
include/exclude rules, cap limits).

- `src/main/java/com/repolens/config/Settings.java` — settings record shape
- `src/main/java/com/repolens/config/SettingsResolver.java` — layered settings resolution
