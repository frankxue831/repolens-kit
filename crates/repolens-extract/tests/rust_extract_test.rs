use repolens_extract::rust_rel::RustRelationshipExtractor;
use repolens_extract::rust_symbol::RustSymbolExtractor;
use repolens_extract::{RelationshipExtractor, SymbolExtractor};
use std::io::Write;
use std::path::Path;

// --- Symbol extractor tests ---

#[test]
fn extracts_pub_function() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "pub fn serve(port: u16) -> Result<()> {{\n    Ok(())\n}}"
    )
    .unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "function");
    assert_eq!(symbols[0].name, "serve");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("serve"));
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[0].line, Some(1), "line is 1-based");
}

#[test]
fn extracts_private_function() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "fn helper() {{}}\nasync fn run() {{}}").unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "helper");
    assert_eq!(symbols[0].visibility.as_deref(), Some("private"));
    assert_eq!(symbols[0].line, Some(1));
    assert_eq!(symbols[1].name, "run");
    assert_eq!(symbols[1].visibility.as_deref(), Some("private"));
    assert_eq!(symbols[1].line, Some(2));
}

#[test]
fn extracts_struct_and_enum() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "pub struct Server {{\n    port: u16,\n}}\n\npub enum Status {{\n    Running,\n    Stopped,\n}}"
    ).unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "class");
    assert_eq!(symbols[0].name, "Server");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[0].line, Some(1));
    assert_eq!(symbols[1].symbol_type, "enum");
    assert_eq!(symbols[1].name, "Status");
    assert_eq!(symbols[1].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].line, Some(5));
}

#[test]
fn extracts_trait() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "pub trait Handler {{\n    fn handle(&self);\n}}").unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "interface");
    assert_eq!(symbols[0].name, "Handler");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_type_alias_and_const_and_static() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "pub type Result<T> = std::result::Result<T, Error>;\npub const MAX: usize = 100;\nstatic COUNTER: AtomicU64 = AtomicU64::new(0);"
    ).unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 3, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "type");
    assert_eq!(symbols[0].name, "Result");
    assert_eq!(symbols[1].symbol_type, "field");
    assert_eq!(symbols[1].name, "MAX");
    assert_eq!(symbols[2].symbol_type, "field");
    assert_eq!(symbols[2].name, "COUNTER");
    assert_eq!(symbols[2].visibility.as_deref(), Some("private"));
}

#[test]
fn extracts_impl_methods() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "impl Server {{\n    pub fn new(port: u16) -> Self {{\n        Self {{ port }}\n    }}\n\n    fn stop(&self) {{}}\n}}"
    ).unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    // Resolver map-split spec (2026-06-12): impl-block fns are "method" so
    // the free-function resolution map stops treating new/default/fmt-class
    // names as call candidates.
    assert_eq!(symbols[0].symbol_type, "method");
    assert_eq!(symbols[0].name, "new");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("Server.new"));
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(
        symbols[0].line,
        Some(2),
        "impl methods get per-method lines"
    );
    assert_eq!(symbols[1].symbol_type, "method");
    assert_eq!(symbols[1].name, "stop");
    assert_eq!(symbols[1].qualified_name.as_deref(), Some("Server.stop"));
    assert_eq!(symbols[1].visibility.as_deref(), Some("private"));
    assert_eq!(symbols[1].line, Some(6));
}

#[test]
fn free_fn_stays_function_while_impl_fn_is_method() {
    // The same name in both positions must classify differently — this is
    // the exact distinction the resolver's function/method map split needs.
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "pub fn run() {{}}\n\nstruct Job;\n\nimpl Job {{\n    pub fn run(&self) {{}}\n}}"
    )
    .unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    let free = symbols
        .iter()
        .find(|s| s.qualified_name.as_deref() == Some("run"))
        .expect("free fn extracted");
    let method = symbols
        .iter()
        .find(|s| s.qualified_name.as_deref() == Some("Job.run"))
        .expect("impl fn extracted");
    assert_eq!(free.symbol_type, "function");
    assert_eq!(method.symbol_type, "method");
}

#[test]
fn line_points_at_declaration_not_attributes_or_docs() {
    // tree-sitter-rust parses doc comments and attribute_items as SIBLINGS of
    // the item node, so `line` lands on the declaration itself, not the first
    // attached attribute or doc line. This pins that semantics.
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "/// Doc comment\n#[derive(Debug)]\npub struct Config {{\n    pub x: u8,\n}}"
    )
    .unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "Config");
    assert_eq!(
        symbols[0].line,
        Some(3),
        "declaration line, not doc/attribute line"
    );
}

#[test]
fn extracts_mod_declaration() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "pub mod handlers;\nmod internal;").unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "module");
    assert_eq!(symbols[0].name, "handlers");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].symbol_type, "module");
    assert_eq!(symbols[1].name, "internal");
    assert_eq!(symbols[1].visibility.as_deref(), Some("private"));
}

#[test]
fn empty_on_invalid_file() {
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(Path::new("/nonexistent/lib.rs"));
    assert!(symbols.is_empty());
}

#[test]
fn pub_crate_visibility() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "pub(crate) fn internal_helper() {{}}\npub(crate) struct Config {{}}"
    )
    .unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "internal_helper");
    assert_eq!(symbols[0].visibility.as_deref(), Some("crate"));
    assert_eq!(symbols[1].name, "Config");
    assert_eq!(symbols[1].visibility.as_deref(), Some("crate"));
}

#[test]
fn pub_super_visibility() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "pub(super) fn parent_visible() {{}}").unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "parent_visible");
    assert_eq!(symbols[0].visibility.as_deref(), Some("restricted"));
}

#[test]
fn generic_impl_method_keeps_type_name() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "struct Container<T> {{\n    val: T,\n}}\n\nimpl<T> Container<T> {{\n    pub fn get(&self) -> &T {{\n        &self.val\n    }}\n}}"
    ).unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    let methods: Vec<_> = symbols
        .iter()
        .filter(|s| s.symbol_type == "method")
        .collect();
    assert_eq!(methods.len(), 1, "methods: {methods:?}");
    assert_eq!(methods[0].name, "get");
    assert_eq!(methods[0].qualified_name.as_deref(), Some("Container.get"));
}

#[test]
fn scoped_impl_method_keeps_type_name() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "impl crate::net::Server {{\n    pub fn start(&self) {{}}\n}}"
    )
    .unwrap();
    let ext = RustSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "start");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("Server.start"));
}

// --- Relationship extractor tests ---

#[test]
fn extracts_use_statements() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "use crate::config::Settings;\nuse super::utils;\nuse crate::model::file_entry::Symbol;"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert_eq!(rels.len(), 3, "rels: {rels:?}");
    assert_eq!(rels[0].rel_type, "imports");
    assert_eq!(rels[0].target_name, "Settings");
    assert_eq!(rels[0].detail, "crate::config::Settings");
    assert_eq!(rels[1].target_name, "utils");
    assert_eq!(rels[1].detail, "super::utils");
    assert_eq!(rels[2].target_name, "Symbol");
    assert_eq!(rels[2].detail, "crate::model::file_entry::Symbol");
}

#[test]
fn extracts_grouped_use() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "use crate::model::{{FileEntry, Symbol}};").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert_eq!(rels.len(), 2, "rels: {rels:?}");
    assert_eq!(rels[0].target_name, "FileEntry");
    assert_eq!(rels[0].detail, "crate::model::FileEntry");
    assert_eq!(rels[1].target_name, "Symbol");
    assert_eq!(rels[1].detail, "crate::model::Symbol");
}

#[test]
fn filters_std_but_keeps_third_party_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "use std::collections::HashMap;\nuse serde::Serialize;\nuse crate::core::Engine;"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(
        imports.len(),
        2,
        "should keep serde + crate-local, filter std: {imports:?}"
    );
    assert_eq!(imports[0].target_name, "Serialize");
    assert_eq!(imports[0].detail, "serde::Serialize");
    assert_eq!(imports[1].target_name, "Engine");
    assert_eq!(imports[1].detail, "crate::core::Engine");
}

#[test]
fn extracts_trait_impl() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "impl SymbolExtractor for RustSymbolExtractor {{\n    fn extract(&self) {{}}\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let impls: Vec<_> = rels.iter().filter(|r| r.rel_type == "implements").collect();
    assert_eq!(impls.len(), 1, "impls: {impls:?}");
    assert_eq!(impls[0].target_name, "SymbolExtractor");
    assert_eq!(impls[0].detail, "SymbolExtractor");
}

#[test]
fn rel_empty_on_invalid_file() {
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(Path::new("/nonexistent/lib.rs"));
    assert!(rels.is_empty());
}

/// I-RUST-MOD-DECLARES-EMITTED (graph declares edges family).
#[test]
fn i_rust_mod_declares_emitted() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "pub mod handlers;\nmod internal;\nmod inline {{ }}").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let declares: Vec<_> = rels.iter().filter(|r| r.rel_type == "declares").collect();
    assert_eq!(
        declares.len(),
        2,
        "should extract file mods only, not inline: {declares:?}"
    );
    assert_eq!(declares[0].target_name, "handlers");
    assert_eq!(declares[0].detail, "handlers");
    assert_eq!(declares[1].target_name, "internal");
    assert_eq!(declares[1].detail, "internal");
}

#[test]
fn extracts_workspace_crate_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "use repolens_core::model::FileEntry;\nuse repolens_app::graph::GraphBuilder;\nuse crate::config::Settings;"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(
        imports.len(),
        3,
        "should include workspace crate imports: {imports:?}"
    );
    assert_eq!(imports[0].target_name, "FileEntry");
    assert_eq!(imports[0].detail, "repolens_core::model::FileEntry");
    assert_eq!(imports[1].target_name, "GraphBuilder");
    assert_eq!(imports[1].detail, "repolens_app::graph::GraphBuilder");
    assert_eq!(imports[2].target_name, "Settings");
    assert_eq!(imports[2].detail, "crate::config::Settings");
}

#[test]
fn still_filters_stdlib_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "use std::collections::HashMap;\nuse core::fmt;\nuse alloc::vec::Vec;\nuse crate::model::Entry;"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 1, "should only keep non-stdlib: {imports:?}");
    assert_eq!(imports[0].detail, "crate::model::Entry");
}

#[test]
fn type_ref_from_function_params() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn process(input: Settings, handler: &dyn EventHandler) -> Report {{\n    todo!()\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    let names: Vec<_> = refs.iter().map(|r| r.target_name.as_str()).collect();
    assert!(
        names.contains(&"Settings"),
        "should find Settings: {names:?}"
    );
    assert!(
        names.contains(&"EventHandler"),
        "should find EventHandler: {names:?}"
    );
    assert!(names.contains(&"Report"), "should find Report: {names:?}");
}

#[test]
fn type_ref_from_named_struct_fields() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "struct Server {{\n    config: Config,\n    handler: RequestHandler,\n    port: u16,\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    let names: Vec<_> = refs.iter().map(|r| r.target_name.as_str()).collect();
    assert!(names.contains(&"Config"), "should find Config: {names:?}");
    assert!(
        names.contains(&"RequestHandler"),
        "should find RequestHandler: {names:?}"
    );
    assert!(
        !names.contains(&"u16"),
        "should not contain primitive u16: {names:?}"
    );
}

#[test]
fn type_ref_from_tuple_struct_fields() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "struct Wrapper(Config, u32);").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    let names: Vec<_> = refs.iter().map(|r| r.target_name.as_str()).collect();
    assert!(names.contains(&"Config"), "should find Config: {names:?}");
    assert!(
        !names.contains(&"u32"),
        "should not contain primitive u32: {names:?}"
    );
}

#[test]
fn type_ref_from_trait_method_signature() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "trait Handler {{\n    fn handle(&self, input: Request) -> Response;\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    let names: Vec<_> = refs.iter().map(|r| r.target_name.as_str()).collect();
    assert!(names.contains(&"Request"), "should find Request: {names:?}");
    assert!(
        names.contains(&"Response"),
        "should find Response: {names:?}"
    );
}

#[test]
fn type_ref_filters_common_std_types() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn process(items: Vec<Config>, cache: HashMap<String, Entry>) -> Option<Report> {{\n    todo!()\n}}"
    ).unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    let names: Vec<_> = refs.iter().map(|r| r.target_name.as_str()).collect();
    assert!(names.contains(&"Config"), "should find Config: {names:?}");
    assert!(names.contains(&"Entry"), "should find Entry: {names:?}");
    assert!(names.contains(&"Report"), "should find Report: {names:?}");
    assert!(!names.contains(&"Vec"), "should filter Vec: {names:?}");
    assert!(
        !names.contains(&"HashMap"),
        "should filter HashMap: {names:?}"
    );
    assert!(
        !names.contains(&"String"),
        "should filter String: {names:?}"
    );
    assert!(
        !names.contains(&"Option"),
        "should filter Option: {names:?}"
    );
}

#[test]
fn use_as_alias() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "use serde::Serialize as Ser;").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 1, "imports: {imports:?}");
    assert_eq!(imports[0].target_name, "Ser");
    assert_eq!(imports[0].detail, "serde::Serialize");
}

#[test]
fn nested_use_groups() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "use crate::a::{{b::{{C, D}}, e::F}};").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 3, "imports: {imports:?}");
    let details: Vec<_> = imports.iter().map(|r| r.detail.as_str()).collect();
    assert!(details.contains(&"crate::a::b::C"), "details: {details:?}");
    assert!(details.contains(&"crate::a::b::D"), "details: {details:?}");
    assert!(details.contains(&"crate::a::e::F"), "details: {details:?}");
}

#[test]
fn grouped_use_self_leaf() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "use crate::model::{{self, Symbol}};").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 2, "imports: {imports:?}");
    let details: Vec<_> = imports.iter().map(|r| r.detail.as_str()).collect();
    assert!(
        details.contains(&"crate::model"),
        "self should expand to crate::model: {details:?}"
    );
    assert!(
        details.contains(&"crate::model::Symbol"),
        "Symbol should have full path: {details:?}"
    );
}

#[test]
fn nested_inline_module_imports_are_collected() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "mod inner {{\n    use beta::util::Helper;\n\n    pub fn make() -> Helper {{\n        todo!()\n    }}\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 1, "imports: {imports:?}");
    assert_eq!(imports[0].target_name, "Helper");
    assert_eq!(imports[0].detail, "beta::util::Helper");
}

#[test]
fn duplicate_leaf_imports_keep_distinct_paths() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "use alpha::util::Helper;\nuse beta::util::Helper;\nuse gamma::util::Helper as GammaHelper;"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 3, "imports: {imports:?}");
    let details: Vec<_> = imports.iter().map(|r| r.detail.as_str()).collect();
    assert!(
        details.contains(&"alpha::util::Helper"),
        "details: {details:?}"
    );
    assert!(
        details.contains(&"beta::util::Helper"),
        "details: {details:?}"
    );
    assert!(
        details.contains(&"gamma::util::Helper"),
        "details: {details:?}"
    );
}

#[test]
fn use_wildcard() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "use crate::prelude::*;").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 1, "imports: {imports:?}");
    assert_eq!(imports[0].target_name, "*");
    assert_eq!(imports[0].detail, "crate::prelude");
}

// --- Call-edge extraction (v1: bare + scoped calls only; spec 2026-06-11) ---

#[test]
fn extracts_bare_function_call() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "fn main() {{\n    compute();\n}}\nfn compute() {{}}").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].target_name, "compute");
    assert_eq!(calls[0].detail, "compute");
}

#[test]
fn extracts_scoped_function_call_targets_function_name() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn main() {{\n    helper::run();\n    crate::a::deep::go();\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(calls.len(), 2, "calls: {calls:?}");
    assert_eq!(calls[0].target_name, "run");
    assert_eq!(calls[0].detail, "helper::run");
    assert_eq!(calls[1].target_name, "go");
    assert_eq!(calls[1].detail, "crate::a::deep::go");
}

#[test]
fn extracts_associated_function_call_targets_type() {
    // Type::assoc() targets the TYPE (Java-parity): method_to_files["new"]
    // has many candidates, which would inflate unresolvedCount; the type's
    // defining file is unique.
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn main() {{\n    let s = Server::new(8080);\n    let c = crate::net::Client::connect();\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(calls.len(), 2, "calls: {calls:?}");
    assert_eq!(calls[0].target_name, "Server");
    assert_eq!(calls[0].detail, "Server::new");
    assert_eq!(calls[1].target_name, "Client");
    assert_eq!(calls[1].detail, "crate::net::Client::connect");
}

#[test]
fn receiver_method_calls_emit_no_calls_edges() {
    // v1 defers obj.method(): receiver type is unknown, name-only resolution
    // would attract unrelated callers (spec 2026-06-11, Codex D1).
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn main() {{\n    let v = vec![1];\n    v.len();\n    \"x\".to_string();\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "receiver method calls must not emit calls edges: {rels:?}"
    );
}

#[test]
fn stdlib_and_common_type_calls_are_filtered() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn main() {{\n    std::mem::swap(&mut 1, &mut 2);\n    core::mem::drop(3);\n    let s = String::from(\"x\");\n    let v = Vec::new();\n    let s2 = Self::make();\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "stdlib paths, common types, and Self must not emit calls edges: {rels:?}"
    );
}

#[test]
fn primitive_scoped_calls_are_filtered() {
    // u128::from(x) etc.: the lowercase primitive segment dodges the
    // uppercase-Type route, so v1 emitted target "from" — which then
    // file-attributed to whatever single file had an `impl From` (real
    // false edge observed on gm-crypto-rs: u128::from -> sm2/public_key.rs).
    // Map-split spec 2026-06-12, Layer 1.5: primitive-scoped calls emit
    // nothing, same posture as the COMMON_RUST_TYPES/Self filter.
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn main() {{\n    let a = u128::from(1u8);\n    let b = f64::from_bits(0);\n    let c = str::len(\"x\");\n    let d = usize::try_from(9i64);\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "primitive-scoped calls must not emit calls edges: {rels:?}"
    );
}

#[test]
fn use_aliased_std_calls_emit_no_calls_edges() {
    // Layer 1.6 (map-split spec): `use std::fs;` then `fs::write(..)` is a
    // stdlib call the path filter can't see — measured as the dominant new
    // false-edge class on the self-scan (37 of 369 calls edges resolved
    // fs::* to whatever local file uniquely defined a same-named free fn).
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "use std::fs;\nuse std::mem::take;\nuse std::io as stdio;\n\nfn main() {{\n    fs::write(\"a\", \"b\").unwrap();\n    let _x = take(&mut String::new());\n    stdio::stdin();\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "calls through std use-aliases (module, item, renamed) must not emit: {rels:?}"
    );
}

#[test]
fn local_module_call_still_emits_without_std_alias() {
    // Control for Layer 1.6: a local module named like a std module is only
    // filtered when a std use-binding actually shadows it in this file.
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "mod fs;\n\nfn main() {{\n    fs::write(\"a\");\n}}").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].detail, "fs::write");
}

#[test]
fn macro_invocations_emit_no_calls_edges() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        f,
        "fn main() {{\n    println!(\"hi\");\n    vec![1, 2];\n}}"
    )
    .unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.iter().all(|r| r.rel_type != "calls"),
        "macro invocations must not emit calls edges: {rels:?}"
    );
}

#[test]
fn same_named_functions_in_different_modules_both_survive_dedup() {
    // calls dedup keys on detail (like imports): a::run() and b::run() are
    // distinct relationships even though target_name collides.
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "fn main() {{\n    a::run();\n    b::run();\n}}").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(calls.len(), 2, "both scoped calls must survive: {calls:?}");
    let details: Vec<&str> = calls.iter().map(|r| r.detail.as_str()).collect();
    assert!(details.contains(&"a::run"));
    assert!(details.contains(&"b::run"));
}

#[test]
fn nested_calls_in_arguments_are_collected() {
    let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(f, "fn main() {{\n    outer(inner());\n}}").unwrap();
    let ext = RustRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert_eq!(calls.len(), 2, "outer and inner: {calls:?}");
}
