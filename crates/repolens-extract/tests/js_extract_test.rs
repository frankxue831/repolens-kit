use repolens_extract::js_rel::JsRelationshipExtractor;
use repolens_extract::js_symbol::JsSymbolExtractor;
use repolens_extract::{RelationshipExtractor, SymbolExtractor};
use std::io::Write;
use std::path::Path;

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs/mixed-small")
        .join(name)
}

fn js_small_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs/js-small")
        .join(name)
}

#[test]
fn extracts_js_exported_function() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&fixture_path("src/util.js"));
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "function");
    assert_eq!(symbols[0].name, "formatMessage");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("formatMessage"));
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_ts_exported_function() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&fixture_path("src/index.ts"));
    assert_eq!(symbols.len(), 1, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "function");
    assert_eq!(symbols[0].name, "main");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_class_and_interface() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(
        f,
        "export class Foo extends Bar {{}}\ninterface Baz {{}}\nexport default class Qux {{}}"
    )
    .unwrap();
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 3, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "class");
    assert_eq!(symbols[0].name, "Foo");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].symbol_type, "interface");
    assert_eq!(symbols[1].name, "Baz");
    assert_eq!(symbols[1].visibility.as_deref(), Some("module-private"));
    assert_eq!(symbols[2].symbol_type, "class");
    assert_eq!(symbols[2].name, "Qux");
    assert_eq!(symbols[2].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_arrow_functions() {
    let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
    writeln!(
        f,
        "export const handler = () => {{}}\nconst internal = async () => {{}}"
    )
    .unwrap();
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "handler");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].name, "internal");
    assert_eq!(symbols[1].visibility.as_deref(), Some("module-private"));
}

#[test]
fn extracts_type_alias_and_enum() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(
        f,
        "export type Config = {{ key: string }}\nexport enum Status {{ Active, Inactive }}"
    )
    .unwrap();
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");
    assert_eq!(symbols[0].symbol_type, "type");
    assert_eq!(symbols[0].name, "Config");
    assert_eq!(symbols[1].symbol_type, "enum");
    assert_eq!(symbols[1].name, "Status");
}

#[test]
fn empty_on_invalid_file() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(Path::new("/nonexistent/foo.js"));
    assert!(symbols.is_empty());
}

#[test]
fn ts_extracts_class_methods_with_qualified_names() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&js_small_path("src/service.ts"));
    let methods: Vec<_> = symbols
        .iter()
        .filter(|s| s.symbol_type == "method")
        .collect();
    assert!(
        methods
            .iter()
            .any(|m| m.qualified_name.as_deref() == Some("UserService.constructor")),
        "methods: {methods:?}"
    );
    assert!(
        methods
            .iter()
            .any(|m| m.qualified_name.as_deref() == Some("UserService.getUser")),
        "methods: {methods:?}"
    );
    assert!(
        methods
            .iter()
            .any(|m| m.qualified_name.as_deref() == Some("UserService.dispose")),
        "methods: {methods:?}"
    );
}

#[test]
fn ts_extracts_abstract_class() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&js_small_path("src/service.ts"));
    let abstract_class = symbols
        .iter()
        .find(|s| s.name == "AbstractHandler")
        .expect("should find AbstractHandler");
    assert_eq!(abstract_class.symbol_type, "class");
    assert_eq!(abstract_class.visibility.as_deref(), Some("public"));
}

#[test]
fn ts_extracts_interface_type_enum() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&js_small_path("src/types.ts"));
    let names: Vec<_> = symbols.iter().map(|s| (&s.name, &s.symbol_type)).collect();
    assert!(
        names.contains(&(&"Disposable".to_string(), &"interface".to_string())),
        "symbols: {names:?}"
    );
    assert!(
        names.contains(&(&"Serializable".to_string(), &"interface".to_string())),
        "symbols: {names:?}"
    );
    assert!(
        names.contains(&(&"Config".to_string(), &"type".to_string())),
        "symbols: {names:?}"
    );
    assert!(
        names.contains(&(&"Status".to_string(), &"enum".to_string())),
        "symbols: {names:?}"
    );
}

#[test]
fn js_extracts_class_and_methods() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&js_small_path("src/base-service.js"));
    let class = symbols
        .iter()
        .find(|s| s.name == "BaseService")
        .expect("should find BaseService");
    assert_eq!(class.symbol_type, "class");
    let methods: Vec<_> = symbols
        .iter()
        .filter(|s| s.symbol_type == "method")
        .collect();
    assert!(
        methods
            .iter()
            .any(|m| m.qualified_name.as_deref() == Some("BaseService.constructor")),
        "methods: {methods:?}"
    );
    assert!(
        methods
            .iter()
            .any(|m| m.qualified_name.as_deref() == Some("BaseService.initialize")),
        "methods: {methods:?}"
    );
}

#[test]
fn js_cjs_exports_marked_public() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&js_small_path("src/util.js"));
    let format_name = symbols
        .iter()
        .find(|s| s.name == "formatName")
        .expect("should find formatName");
    assert_eq!(
        format_name.visibility.as_deref(),
        Some("public"),
        "CJS-exported function should be public"
    );
    let validate = symbols
        .iter()
        .find(|s| s.name == "validateConfig")
        .expect("should find validateConfig");
    assert_eq!(
        validate.visibility.as_deref(),
        Some("public"),
        "CJS-exported function should be public"
    );
}

#[test]
fn ts_extracts_exported_arrow_function() {
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(&js_small_path("src/app.ts"));
    let create_app = symbols
        .iter()
        .find(|s| s.name == "createApp")
        .expect("should find createApp");
    assert_eq!(create_app.symbol_type, "function");
    assert_eq!(create_app.visibility.as_deref(), Some("public"));
}

#[test]
fn nested_function_not_extracted() {
    let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
    writeln!(f, "function outer() {{\n  function inner() {{}}\n}}").unwrap();
    let ext = JsSymbolExtractor::new();
    let symbols = ext.extract(f.path());
    assert_eq!(
        symbols.len(),
        1,
        "only outer should be extracted, got: {symbols:?}"
    );
    assert_eq!(symbols[0].name, "outer");
}

// --- Relationship extractor tests ---

#[test]
fn extracts_relative_import() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&fixture_path("src/index.ts"));
    assert_eq!(rels.len(), 1, "rels: {rels:?}");
    assert_eq!(rels[0].rel_type, "imports");
    assert_eq!(rels[0].detail, "./util");
}

#[test]
fn util_has_no_imports() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&fixture_path("src/util.js"));
    assert!(rels.is_empty());
}

#[test]
fn extracts_require_and_reexport() {
    let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
    writeln!(
        f,
        "const foo = require('./bar')\nexport {{ baz }} from '../qux'"
    )
    .unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert_eq!(rels.len(), 2, "rels: {rels:?}");
    assert_eq!(rels[0].detail, "./bar");
    assert_eq!(rels[1].detail, "../qux");
}

#[test]
fn skips_external_packages() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(
        f,
        "import React from 'react'\nimport {{ foo }} from './local'"
    )
    .unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert_eq!(
        rels.len(),
        1,
        "should skip non-relative imports, rels: {rels:?}"
    );
    assert_eq!(rels[0].detail, "./local");
}

#[test]
fn extracts_class_extends() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(f, "class Foo extends Bar {{}}").unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    assert!(
        rels.iter()
            .any(|r| r.rel_type == "extends" && r.target_name == "Bar"),
        "rels: {rels:?}"
    );
}

// --- Tree-sitter-specific relationship tests ---

#[test]
fn ts_extracts_extends_and_implements() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&js_small_path("src/service.ts"));
    assert!(
        rels.iter()
            .any(|r| r.rel_type == "extends" && r.target_name == "BaseService"),
        "should find extends BaseService, rels: {rels:?}"
    );
    assert!(
        rels.iter()
            .any(|r| r.rel_type == "implements" && r.target_name == "Disposable"),
        "should find implements Disposable, rels: {rels:?}"
    );
}

#[test]
fn ts_extracts_abstract_class_extends() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&js_small_path("src/service.ts"));
    // AbstractHandler extends BaseService
    let extends_rels: Vec<_> = rels.iter().filter(|r| r.rel_type == "extends").collect();
    assert!(
        extends_rels.iter().any(|r| r.target_name == "BaseService"),
        "abstract class should extend BaseService, extends: {extends_rels:?}"
    );
}

#[test]
fn ts_extracts_interface_extends() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&js_small_path("src/types.ts"));
    assert!(
        rels.iter()
            .any(|r| r.rel_type == "extends" && r.target_name == "Disposable"),
        "Serializable should extend Disposable, rels: {rels:?}"
    );
}

#[test]
fn ts_extracts_type_refs() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&js_small_path("src/app.ts"));
    let type_refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    assert!(
        type_refs.iter().any(|r| r.target_name == "Config"),
        "should find type_ref to Config, type_refs: {type_refs:?}"
    );
    assert!(
        type_refs.iter().any(|r| r.target_name == "UserService"),
        "should find type_ref to UserService, type_refs: {type_refs:?}"
    );
    assert!(
        type_refs.iter().any(|r| r.target_name == "Status"),
        "should find type_ref to Status, type_refs: {type_refs:?}"
    );
}

#[test]
fn ts_filters_builtin_types() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(
        f,
        "function process(items: Array<string>, cb: Function): Promise<void> {{}}\nfunction custom(s: MyService): void {{}}"
    )
    .unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let type_refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    // Array, Function, Promise, string, void are filtered
    assert!(
        !type_refs.iter().any(|r| r.target_name == "Array"),
        "Array should be filtered, type_refs: {type_refs:?}"
    );
    assert!(
        !type_refs.iter().any(|r| r.target_name == "Promise"),
        "Promise should be filtered, type_refs: {type_refs:?}"
    );
    assert!(
        type_refs.iter().any(|r| r.target_name == "MyService"),
        "MyService should NOT be filtered, type_refs: {type_refs:?}"
    );
}

#[test]
fn ts_filters_single_letter_generics() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(f, "function identity<T>(value: T): T {{ return value; }}").unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let type_refs: Vec<_> = rels.iter().filter(|r| r.rel_type == "type_ref").collect();
    assert!(
        type_refs.is_empty(),
        "single-letter generic T should be filtered, type_refs: {type_refs:?}"
    );
}

#[test]
fn ts_deduplicates_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(
        f,
        "import {{ foo }} from './util'\nimport {{ bar }} from './util'"
    )
    .unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let imports: Vec<_> = rels.iter().filter(|r| r.rel_type == "imports").collect();
    assert_eq!(
        imports.len(),
        1,
        "duplicate import paths should be deduped, imports: {imports:?}"
    );
}

#[test]
fn ts_extracts_reexport() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&js_small_path("src/types.ts"));
    assert!(
        rels.iter()
            .any(|r| r.rel_type == "imports" && r.detail == "./base-service"),
        "re-export from ./base-service should produce imports rel, rels: {rels:?}"
    );
}

#[test]
fn js_extracts_require_import() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(&js_small_path("src/util.js"));
    assert!(
        rels.iter()
            .any(|r| r.rel_type == "imports" && r.detail == "./types"),
        "require('./types') should produce imports rel, rels: {rels:?}"
    );
}

#[test]
fn rel_empty_on_invalid_file() {
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(Path::new("/nonexistent/foo.ts"));
    assert!(rels.is_empty());
}

/// TypeScript type-only imports and re-exports are `type_import`; anything
/// that brings in a runtime value stays `imports`.
#[test]
fn ts_type_only_imports_are_marked() {
    let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
    writeln!(
        f,
        "import type {{ A }} from \"./a.js\";\nimport {{ type B, type C }} from \"./b.js\";\nimport {{ type D, e }} from \"./d.js\";\nimport * as util from \"./util.js\";\nexport type {{ F }} from \"./f.js\";\nexport {{ g }} from \"./g.js\";"
    )
    .unwrap();
    let ext = JsRelationshipExtractor::new();
    let rels = ext.extract(f.path());
    let kind = |detail: &str| {
        rels.iter()
            .find(|r| r.detail == detail)
            .map(|r| r.rel_type.clone())
            .unwrap_or_else(|| panic!("{detail} missing: {rels:?}"))
    };
    assert_eq!(kind("./a.js"), "type_import");
    assert_eq!(kind("./b.js"), "type_import");
    assert_eq!(kind("./d.js"), "imports");
    assert_eq!(kind("./util.js"), "imports");
    assert_eq!(kind("./f.js"), "type_import");
    assert_eq!(kind("./g.js"), "imports");
}
