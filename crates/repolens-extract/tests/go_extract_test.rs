use repolens_extract::go_rel::GoRelationshipExtractor;
use repolens_extract::go_symbol::GoSymbolExtractor;
use repolens_extract::{RelationshipExtractor, SymbolExtractor};
use std::io::Write;
use std::path::Path;

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs/mixed-small")
        .join(name)
}

fn go_small_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs/go-small")
        .join(name)
}

#[test]
fn extracts_go_functions() {
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(&fixture_path("go/cmd/tool/main.go"));
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");

    assert_eq!(symbols[0].symbol_type, "function");
    assert_eq!(symbols[0].name, "main");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("main"));
    assert_eq!(symbols[0].visibility.as_deref(), Some("private"));

    assert_eq!(symbols[1].symbol_type, "function");
    assert_eq!(symbols[1].name, "greet");
    assert_eq!(symbols[1].qualified_name.as_deref(), Some("greet"));
    assert_eq!(symbols[1].visibility.as_deref(), Some("private"));
}

#[test]
fn extracts_struct_and_interface() {
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(
        f,
        "package main\n\ntype Server struct {{\n\tAddr string\n}}\n\ntype Handler interface {{\n\tHandle()\n}}"
    )
    .unwrap();
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "class");
    assert_eq!(symbols[0].name, "Server");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].symbol_type, "interface");
    assert_eq!(symbols[1].name, "Handler");
    assert_eq!(symbols[1].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_method_with_receiver() {
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(
        f,
        "package main\n\nfunc (s *Server) Start() {{}}\nfunc (s server) stop() {{}}"
    )
    .unwrap();
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "function");
    assert_eq!(symbols[0].name, "Start");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("Server.Start"));
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].symbol_type, "function");
    assert_eq!(symbols[1].name, "stop");
    assert_eq!(symbols[1].qualified_name.as_deref(), Some("server.stop"));
    assert_eq!(symbols[1].visibility.as_deref(), Some("private"));
}

#[test]
fn empty_on_invalid_file() {
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(Path::new("/nonexistent/foo.go"));
    assert!(symbols.is_empty());
}

// --- Relationship extractor tests ---

#[test]
fn main_go_has_no_intra_repo_imports() {
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(&fixture_path("go/cmd/tool/main.go"));
    assert!(
        rels.is_empty(),
        "stdlib imports should be filtered out, rels: {rels:?}"
    );
}

#[test]
fn extracts_grouped_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(
        f,
        "package main\n\nimport (\n\t\"fmt\"\n\t\"myapp/config\"\n)"
    )
    .unwrap();
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert_eq!(
        rels.len(),
        1,
        "should keep intra-repo, filter stdlib, rels: {rels:?}"
    );
    assert_eq!(rels[0].detail, "myapp/config");
}

#[test]
fn filters_known_stdlib() {
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(
        f,
        "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n\t\"io\"\n\t\"net\"\n\t\"strings\"\n)"
    )
    .unwrap();
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.is_empty(),
        "all stdlib should be filtered, rels: {rels:?}"
    );
}

#[test]
fn keeps_dotted_import_paths_for_the_graph_to_check() {
    // `github.com/...` may be the repo's own module (#24); the graph builder
    // reads go.mod and drops third-party ones.
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(f, "package main\n\nimport \"github.com/pkg/errors\"").unwrap();
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert_eq!(rels.len(), 1, "rels: {rels:?}");
    assert_eq!(rels[0].rel_type, "imports");
    assert_eq!(rels[0].target_name, "github.com/pkg/errors");
}

#[test]
fn extracts_struct_embedding() {
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(
        f,
        "package main\n\ntype Server struct {{\n\tBaseServer\n\tAddr string\n}}"
    )
    .unwrap();
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let extends: Vec<_> = rels.iter().filter(|r| r.rel_type == "extends").collect();
    assert_eq!(
        extends.len(),
        1,
        "should find BaseServer embedding, extends: {extends:?}"
    );
    assert_eq!(extends[0].target_name, "BaseServer");
}

// --- Tree-sitter-specific Go symbol tests (go-small fixture) ---

#[test]
fn go_small_extracts_server_symbols() {
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(&go_small_path("internal/model/server.go"));
    assert!(!symbols.is_empty(), "should extract symbols from server.go");

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"BaseServer"),
        "should find BaseServer struct: {names:?}"
    );
    assert!(
        names.contains(&"Server"),
        "should find Server struct: {names:?}"
    );
    assert!(
        names.contains(&"Handler"),
        "should find Handler interface: {names:?}"
    );
    assert!(
        names.contains(&"NewServer"),
        "should find NewServer function: {names:?}"
    );

    let base_server = symbols.iter().find(|s| s.name == "BaseServer").unwrap();
    assert_eq!(base_server.symbol_type, "class");
    assert_eq!(base_server.visibility.as_deref(), Some("public"));

    let handler_iface = symbols.iter().find(|s| s.name == "Handler").unwrap();
    assert_eq!(handler_iface.symbol_type, "interface");
    assert_eq!(handler_iface.visibility.as_deref(), Some("public"));
}

#[test]
fn go_small_extracts_methods_with_receivers() {
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(&go_small_path("internal/model/server.go"));

    let start = symbols
        .iter()
        .find(|s| s.name == "Start")
        .expect("should find Start method");
    assert_eq!(start.symbol_type, "function");
    assert_eq!(start.qualified_name.as_deref(), Some("Server.Start"));
    assert_eq!(start.visibility.as_deref(), Some("public"));
}

#[test]
fn go_small_extracts_unexported_struct() {
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(&go_small_path("internal/model/server.go"));

    let helper = symbols
        .iter()
        .find(|s| s.name == "unexportedHelper")
        .expect("should find unexportedHelper");
    assert_eq!(helper.symbol_type, "function");
    assert_eq!(helper.visibility.as_deref(), Some("private"));
}

#[test]
fn go_small_extracts_main_functions() {
    let ext = GoSymbolExtractor::new();
    let symbols = ext.extract(&go_small_path("cmd/server/main.go"));
    assert_eq!(
        symbols.len(),
        1,
        "main.go should have exactly one symbol: {symbols:?}"
    );
    assert_eq!(symbols[0].name, "main");
    assert_eq!(symbols[0].symbol_type, "function");
    assert_eq!(symbols[0].visibility.as_deref(), Some("private"));
}

// --- Tree-sitter-specific Go relationship tests (go-small fixture) ---

#[test]
fn go_small_extracts_intra_module_import() {
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(&go_small_path("internal/handler/handler.go"));
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(
        imports.len(),
        1,
        "should find one intra-module import: {imports:?}"
    );
    assert_eq!(imports[0].detail, "go-small/internal/model");
}

#[test]
fn go_small_extracts_struct_embedding() {
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(&go_small_path("internal/model/server.go"));
    let extends: Vec<_> = rels.iter().filter(|r| r.rel_type == "extends").collect();
    assert_eq!(extends.len(), 1, "Server embeds BaseServer: {extends:?}");
    assert_eq!(extends[0].target_name, "BaseServer");
}

#[test]
fn go_small_extracts_pointer_embedding() {
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(&go_small_path("internal/handler/handler.go"));
    let extends: Vec<_> = rels.iter().filter(|r| r.rel_type == "extends").collect();
    assert_eq!(
        extends.len(),
        1,
        "Handler pointer-embeds model.Server: {extends:?}"
    );
    assert_eq!(extends[0].target_name, "model.Server");
}

#[test]
fn go_deduplicates_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".go").unwrap();
    writeln!(
        f,
        "package main\n\nimport \"myapp/config\"\nimport \"myapp/config\""
    )
    .unwrap();
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(
        imports.len(),
        1,
        "duplicate imports should be deduped: {imports:?}"
    );
    assert_eq!(imports[0].detail, "myapp/config");
}

#[test]
fn go_rel_empty_on_invalid_file() {
    let ext = GoRelationshipExtractor::new();
    let rels = ext.extract(Path::new("/nonexistent/foo.go"));
    assert!(rels.is_empty());
}

#[test]
fn go_small_server_has_no_embeddings() {
    let ext = GoRelationshipExtractor::new();
    // cmd/server/main.go has no structs, so no extends relationships
    let rels = ext.extract(&go_small_path("cmd/server/main.go"));
    let extends: Vec<_> = rels.iter().filter(|r| r.rel_type == "extends").collect();
    assert!(
        extends.is_empty(),
        "main.go has no struct embeddings: {extends:?}"
    );
}
