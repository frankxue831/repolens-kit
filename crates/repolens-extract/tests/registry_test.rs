use repolens_extract::registry::{RelationshipExtractorRegistry, SymbolExtractorRegistry};
use std::path::Path;

#[test]
fn symbol_registry_returns_extractor_for_supported_languages() {
    let reg = SymbolExtractorRegistry::new();
    assert!(reg.for_language("Java").is_some());
    assert!(reg.for_language("JavaScript").is_some());
    assert!(reg.for_language("TypeScript").is_some());
    assert!(reg.for_language("Python").is_some());
    assert!(reg.for_language("Go").is_some());
    assert!(reg.for_language("Rust").is_some());
}

#[test]
fn symbol_registry_returns_none_for_unsupported() {
    let reg = SymbolExtractorRegistry::new();
    assert!(reg.for_language("Ruby").is_none());
    assert!(reg.for_language("Unknown").is_none());
}

#[test]
fn relationship_registry_returns_extractor_for_supported_languages() {
    let reg = RelationshipExtractorRegistry::new();
    assert!(reg.for_language("Java").is_some());
    assert!(reg.for_language("JavaScript").is_some());
    assert!(reg.for_language("TypeScript").is_some());
    assert!(reg.for_language("Python").is_some());
    assert!(reg.for_language("Go").is_some());
    assert!(reg.for_language("Rust").is_some());
}

#[test]
fn relationship_registry_returns_none_for_unsupported() {
    let reg = RelationshipExtractorRegistry::new();
    assert!(reg.for_language("C++").is_none());
    assert!(reg.for_language("").is_none());
}

#[test]
fn js_and_ts_produce_same_output() {
    let reg = SymbolExtractorRegistry::new();
    let js = reg.for_language("JavaScript").unwrap();
    let ts = reg.for_language("TypeScript").unwrap();

    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/mixed-small/src/util.js");
    let js_symbols = js.extract(&path);
    let ts_symbols = ts.extract(&path);
    assert_eq!(js_symbols, ts_symbols);
}
