//! The `language_detection_matches_golden_for_all_scenarios` test in this
//! file is evidence for the `I-LANG-FIXTURE-PARITY` invariant named in
//! `docs/superpowers/specs/2026-04-14-language-and-profile-catalogue.md`.
//!
//! Any rename, deletion, or `#[ignore]`-ing of that test MUST be
//! accompanied by a tier demotion of the Language Classification family
//! in `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`
//! §5 and a corresponding update to the catalogue spec.

use repolens_core::model::file_entry::FileEntry;
use repolens_extract::lang::detect;
use repolens_extract::registry::SymbolExtractorRegistry;
use std::path::Path;

fn load_golden_entries(scenario: &str) -> Vec<FileEntry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scenarios")
        .join(scenario)
        .join("pack/files-0001.json");
    let data = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn input_root(scenario: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs")
        .join(scenario)
}

#[test]
fn java_small_symbol_parity() {
    let golden = load_golden_entries("java-small");
    let registry = SymbolExtractorRegistry::new();
    let root = input_root("java-small");

    for entry in &golden {
        if !entry.included || entry.symbols.is_empty() {
            continue;
        }
        let language = detect(Path::new(&entry.path));
        let extractor = registry
            .for_language(language)
            .unwrap_or_else(|| panic!("no extractor for {language} ({})", entry.path));
        let actual = extractor.extract(&root.join(&entry.path));

        assert_eq!(
            actual.len(),
            entry.symbols.len(),
            "symbol count mismatch for {}: expected {}, got {} — actual: {actual:?}",
            entry.path,
            entry.symbols.len(),
            actual.len()
        );

        for (i, (act, exp)) in actual.iter().zip(entry.symbols.iter()).enumerate() {
            assert_eq!(
                act.symbol_type, exp.symbol_type,
                "{}[{}] type: {:?} vs {:?}",
                entry.path, i, act.symbol_type, exp.symbol_type
            );
            assert_eq!(
                act.name, exp.name,
                "{}[{}] name: {:?} vs {:?}",
                entry.path, i, act.name, exp.name
            );
            assert_eq!(
                act.qualified_name, exp.qualified_name,
                "{}[{}] qualifiedName: {:?} vs {:?}",
                entry.path, i, act.qualified_name, exp.qualified_name
            );
            assert_eq!(
                act.visibility, exp.visibility,
                "{}[{}] visibility: {:?} vs {:?}",
                entry.path, i, act.visibility, exp.visibility
            );
        }
    }
}

#[test]
fn mixed_small_symbol_parity() {
    let golden = load_golden_entries("mixed-small");
    let registry = SymbolExtractorRegistry::new();
    let root = input_root("mixed-small");

    for entry in &golden {
        if !entry.included || entry.symbols.is_empty() {
            continue;
        }
        let language = detect(Path::new(&entry.path));
        let extractor = match registry.for_language(language) {
            Some(e) => e,
            None => continue, // Skip languages without extractors (e.g., "Unknown")
        };
        let actual = extractor.extract(&root.join(&entry.path));

        assert_eq!(
            actual.len(),
            entry.symbols.len(),
            "symbol count mismatch for {}: expected {}, got {} — actual: {actual:?}",
            entry.path,
            entry.symbols.len(),
            actual.len()
        );

        for (i, (act, exp)) in actual.iter().zip(entry.symbols.iter()).enumerate() {
            assert_eq!(
                act.symbol_type, exp.symbol_type,
                "{}[{}] type mismatch",
                entry.path, i
            );
            assert_eq!(act.name, exp.name, "{}[{}] name mismatch", entry.path, i);
            assert_eq!(
                act.qualified_name, exp.qualified_name,
                "{}[{}] qualifiedName mismatch",
                entry.path, i
            );
            assert_eq!(
                act.visibility, exp.visibility,
                "{}[{}] visibility mismatch",
                entry.path, i
            );
        }
    }
}

#[test]
fn excluded_small_symbol_parity() {
    let golden = load_golden_entries("excluded-small");
    let registry = SymbolExtractorRegistry::new();
    let root = input_root("excluded-small");

    for entry in &golden {
        if !entry.included || entry.symbols.is_empty() {
            continue;
        }
        let language = detect(Path::new(&entry.path));
        let extractor = registry
            .for_language(language)
            .unwrap_or_else(|| panic!("no extractor for {language} ({})", entry.path));
        let actual = extractor.extract(&root.join(&entry.path));

        assert_eq!(
            actual, entry.symbols,
            "excluded-small parity mismatch for {}",
            entry.path
        );
    }
}

#[test]
fn language_detection_matches_golden_for_all_scenarios() {
    for scenario in ["java-small", "mixed-small", "excluded-small"] {
        let golden = load_golden_entries(scenario);
        for entry in &golden {
            if !entry.included || entry.language.is_empty() {
                continue;
            }
            let detected = detect(Path::new(&entry.path));
            assert_eq!(
                detected, entry.language,
                "language detection mismatch for {scenario}/{}: expected {:?}, got {:?}",
                entry.path, entry.language, detected
            );
        }
    }
}
