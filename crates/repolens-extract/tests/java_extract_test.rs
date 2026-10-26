use repolens_extract::java_rel::JavaRelationshipExtractor;
use repolens_extract::java_symbol::JavaSymbolExtractor;
use repolens_extract::{RelationshipExtractor, SymbolExtractor};
use std::io::Write;
use std::path::Path;

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs/java-small")
        .join(name)
}

#[test]
fn extracts_app_symbols() {
    let ext = JavaSymbolExtractor::new();
    let symbols = ext.extract(&fixture_path("src/main/java/com/example/App.java"));
    assert_eq!(symbols.len(), 3, "symbols: {symbols:?}");

    assert_eq!(symbols[0].symbol_type, "class");
    assert_eq!(symbols[0].name, "App");
    assert_eq!(
        symbols[0].qualified_name.as_deref(),
        Some("com.example.App")
    );
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));

    assert_eq!(symbols[1].symbol_type, "method");
    assert_eq!(symbols[1].name, "main");
    assert_eq!(
        symbols[1].qualified_name.as_deref(),
        Some("void main(String[])")
    );
    assert_eq!(symbols[1].visibility.as_deref(), Some("public"));

    assert_eq!(symbols[2].symbol_type, "method");
    assert_eq!(symbols[2].name, "run");
    assert_eq!(
        symbols[2].qualified_name.as_deref(),
        Some("String run(String)")
    );
    assert_eq!(symbols[2].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_helper_symbols() {
    let ext = JavaSymbolExtractor::new();
    let symbols = ext.extract(&fixture_path("src/main/java/com/example/Helper.java"));
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");

    assert_eq!(symbols[0].symbol_type, "class");
    assert_eq!(symbols[0].name, "Helper");
    assert_eq!(
        symbols[0].qualified_name.as_deref(),
        Some("com.example.Helper")
    );

    assert_eq!(symbols[1].symbol_type, "method");
    assert_eq!(symbols[1].name, "greet");
    assert_eq!(
        symbols[1].qualified_name.as_deref(),
        Some("String greet(String)")
    );
}

#[test]
fn empty_on_invalid_file() {
    let ext = JavaSymbolExtractor::new();
    let symbols = ext.extract(Path::new("/nonexistent/Foo.java"));
    assert!(symbols.is_empty());
}

#[test]
fn field_qualified_name_is_type() {
    let mut f = tempfile::NamedTempFile::with_suffix(".java").unwrap();
    writeln!(
        f,
        "package com.test;\npublic class Foo {{\n    private String name;\n    public int count;\n}}"
    )
    .unwrap();
    let ext = JavaSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    let fields: Vec<_> = symbols
        .iter()
        .filter(|s| s.symbol_type == "field")
        .collect();
    assert_eq!(fields.len(), 2, "fields: {fields:?}");
    assert_eq!(fields[0].name, "name");
    assert_eq!(fields[0].qualified_name.as_deref(), Some("String"));
    assert_eq!(fields[0].visibility.as_deref(), Some("private"));
    assert_eq!(fields[1].name, "count");
    assert_eq!(fields[1].qualified_name.as_deref(), Some("int"));
    assert_eq!(fields[1].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_nested_class_members() {
    let mut f = tempfile::NamedTempFile::with_suffix(".java").unwrap();
    writeln!(
        f,
        "package com.test;\npublic class Outer {{\n    public static class Inner {{\n        public void doStuff() {{}}\n    }}\n}}"
    )
    .unwrap();
    let ext = JavaSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"Outer".to_string()),
        "missing Outer, got: {names:?}"
    );
    assert!(
        names.contains(&&"Inner".to_string()),
        "missing Inner (nested), got: {names:?}"
    );
    assert!(
        names.contains(&&"doStuff".to_string()),
        "missing doStuff (nested method), got: {names:?}"
    );
}

// --- Relationship extractor tests ---

#[test]
fn extracts_app_relationships() {
    let ext = JavaRelationshipExtractor::new();
    let rels = ext.extract(&fixture_path("src/main/java/com/example/App.java"));

    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(imports.len(), 1, "imports: {imports:?}");
    assert_eq!(imports[0].detail, "com.example.Helper");

    let calls: Vec<_> = rels.iter().filter(|r| r.rel_type == "calls").collect();
    assert!(
        !calls.is_empty(),
        "should extract method calls, got: {rels:?}"
    );
    assert!(
        calls.iter().any(|r| r.detail.contains("greet")),
        "should find greet call, calls: {calls:?}"
    );

    let type_refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    assert!(
        type_refs.iter().any(|r| r.target_name == "Helper"),
        "should find Helper type_ref, type_refs: {type_refs:?}"
    );
}

#[test]
fn helper_has_no_imports() {
    let ext = JavaRelationshipExtractor::new();
    let rels = ext.extract(&fixture_path("src/main/java/com/example/Helper.java"));
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert!(imports.is_empty());
}

#[test]
fn rel_empty_on_invalid_file() {
    let ext = JavaRelationshipExtractor::new();
    let rels = ext.extract(Path::new("/nonexistent/Foo.java"));
    assert!(rels.is_empty());
}

#[test]
fn type_ref_from_params_and_returns() {
    let mut f = tempfile::NamedTempFile::with_suffix(".java").unwrap();
    writeln!(
        f,
        "package com.test;\npublic class Svc {{\n    public Foo process(Bar b) {{ return null; }}\n}}"
    )
    .unwrap();
    let ext = JavaRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let type_refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    assert!(
        type_refs.iter().any(|r| r.target_name == "Foo"),
        "should find return type Foo, type_refs: {type_refs:?}"
    );
    assert!(
        type_refs.iter().any(|r| r.target_name == "Bar"),
        "should find param type Bar, type_refs: {type_refs:?}"
    );
}

#[test]
fn deduplicates_relationships() {
    let mut f = tempfile::NamedTempFile::with_suffix(".java").unwrap();
    writeln!(
        f,
        "package com.test;\npublic class Svc {{\n    Foo a;\n    Foo b;\n    public Foo get() {{ return null; }}\n}}"
    )
    .unwrap();
    let ext = JavaRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let foo_refs: Vec<_> = rels
        .iter()
        .filter(|r| r.rel_type == "type_ref" && r.target_name == "Foo")
        .collect();
    assert_eq!(
        foo_refs.len(),
        1,
        "Foo type_ref should be deduplicated, got: {foo_refs:?}"
    );
}
