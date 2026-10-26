use repolens_extract::py_rel::PyRelationshipExtractor;
use repolens_extract::py_symbol::PySymbolExtractor;
use repolens_extract::{RelationshipExtractor, SymbolExtractor};
use std::io::Write;
use std::path::Path;

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/inputs/mixed-small")
        .join(name)
}

#[test]
fn extracts_worker_symbols() {
    let ext = PySymbolExtractor;
    let symbols = ext.extract(&fixture_path("py/worker.py"));
    assert_eq!(symbols.len(), 2, "symbols: {symbols:?}");

    assert_eq!(symbols[0].symbol_type, "class");
    assert_eq!(symbols[0].name, "Worker");
    assert_eq!(symbols[0].qualified_name.as_deref(), Some("Worker"));
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));

    assert_eq!(symbols[1].symbol_type, "function");
    assert_eq!(symbols[1].name, "process");
    assert_eq!(symbols[1].qualified_name.as_deref(), Some("process"));
    assert_eq!(symbols[1].visibility.as_deref(), Some("public"));
}

#[test]
fn extracts_top_level_variables() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        f,
        "MAX_SIZE = 100\n__version__ = \"1.0\"\n_cache = {{}}\nconfig: Config = load()"
    )
    .unwrap();
    let ext = PySymbolExtractor;
    let symbols = ext.extract(f.path());
    assert_eq!(symbols.len(), 4, "symbols: {symbols:?}");
    assert_eq!(symbols[0].name, "MAX_SIZE");
    assert_eq!(symbols[0].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[1].name, "__version__");
    assert_eq!(symbols[1].visibility.as_deref(), Some("public"));
    assert_eq!(symbols[2].name, "_cache");
    assert_eq!(symbols[2].visibility.as_deref(), Some("private"));
    assert_eq!(symbols[3].name, "config");
    assert_eq!(symbols[3].visibility.as_deref(), Some("public"));
}

#[test]
fn skips_indented_defs() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(f, "class Foo:\n    def bar(self):\n        pass").unwrap();
    let ext = PySymbolExtractor;
    let symbols = ext.extract(f.path());
    assert_eq!(
        symbols.len(),
        1,
        "should only find top-level class, symbols: {symbols:?}"
    );
    assert_eq!(symbols[0].name, "Foo");
}

#[test]
fn empty_on_invalid_file() {
    let ext = PySymbolExtractor;
    let symbols = ext.extract(Path::new("/nonexistent/foo.py"));
    assert!(symbols.is_empty());
}

// --- Relationship extractor tests ---

#[test]
fn worker_has_no_relationships() {
    let ext = PyRelationshipExtractor;
    let rels = ext.extract(&fixture_path("py/worker.py"));
    assert!(rels.is_empty(), "rels: {rels:?}");
}

#[test]
fn extracts_relative_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        f,
        "from .utils import helper\nfrom ..core.base import Base\nfrom . import config"
    )
    .unwrap();
    let ext = PyRelationshipExtractor;
    let rels = ext.extract(f.path());
    assert_eq!(rels.len(), 3, "rels: {rels:?}");
    assert_eq!(rels[0].detail, "./utils");
    assert_eq!(rels[1].detail, "../core/base");
    assert_eq!(rels[2].detail, "./config");
}

/// #23: absolute imports are emitted so the graph can resolve the ones that
/// name the repo's own packages (stdlib ones are dropped at resolution).
#[test]
fn extracts_absolute_imports() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        f,
        "import os\nimport app.render.runner as r, json\nfrom app.config.loader import (\n    load_config,\n    Config as Cfg,\n)"
    )
    .unwrap();
    let ext = PyRelationshipExtractor;
    let rels = ext.extract(f.path());
    let details: Vec<&str> = rels.iter().map(|r| r.detail.as_str()).collect();
    assert_eq!(
        details,
        vec![
            "os",
            "app.render.runner",
            "json",
            "app.config.loader",
            "app.config.loader.load_config",
            "app.config.loader.Config",
        ],
        "rels: {rels:?}"
    );
    assert!(rels.iter().all(|r| r.rel_type == "imports"));
    // Module relationships carry the module as target; per-name ones carry
    // the name (resolved only if it is a submodule).
    assert_eq!(rels[3].target_name, "app.config.loader");
    assert_eq!(rels[4].target_name, "load_config");
}

/// #22: imports inside `if TYPE_CHECKING:` are type-only; the `else:`
/// branch and code after the block are normal imports.
#[test]
fn marks_type_checking_imports_type_only() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        f,
        "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from .models import (\n        Request,\n    )\n    import app.types\nelse:\n    from .compat import Mapping\n\nfrom .hooks import default_hooks\n\ndef f():\n    if typing.TYPE_CHECKING:\n        from .auth import Auth\n    from .utils import x\n"
    )
    .unwrap();
    let ext = PyRelationshipExtractor;
    let rels = ext.extract(f.path());
    let kind = |detail: &str| {
        rels.iter()
            .find(|r| r.detail == detail)
            .map(|r| r.rel_type.as_str())
            .unwrap_or_else(|| panic!("{detail} missing: {rels:?}"))
    };
    assert_eq!(kind("./models"), "type_import");
    assert_eq!(kind("app.types"), "type_import");
    assert_eq!(kind("./compat"), "imports");
    assert_eq!(kind("./hooks"), "imports");
    assert_eq!(kind("./auth"), "type_import");
    assert_eq!(kind("./utils"), "imports");
}

/// Import-looking lines inside docstrings and comments are not imports.
#[test]
fn ignores_imports_in_docstrings_and_comments() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        f,
        "\"\"\"Usage:\n\nfrom .models import Request\n\"\"\"\n# from .auth import Auth\nfrom .hooks import default_hooks  # from .x import y"
    )
    .unwrap();
    let ext = PyRelationshipExtractor;
    let rels = ext.extract(f.path());
    let details: Vec<&str> = rels.iter().map(|r| r.detail.as_str()).collect();
    assert_eq!(details, vec!["./hooks"], "rels: {rels:?}");
}

#[test]
fn extracts_class_inheritance() {
    let mut f = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        f,
        "class Foo(Bar, Baz):\n    pass\nclass MyError(Exception):\n    pass"
    )
    .unwrap();
    let ext = PyRelationshipExtractor;
    let rels = ext.extract(f.path());
    let extends: Vec<_> = rels.iter().filter(|r| r.rel_type == "extends").collect();
    assert_eq!(
        extends.len(),
        2,
        "should find Bar and Baz but filter Exception, extends: {extends:?}"
    );
    assert!(extends.iter().any(|r| r.target_name == "Bar"));
    assert!(extends.iter().any(|r| r.target_name == "Baz"));
}
