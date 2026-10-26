//! Regressions for the import graph (dogfood round 4):
//! - #21: name-matched edges (`calls`, `type_ref`, ...) only count when they
//!   line up with a real import, so a bare `Field` / `Entry` / `equals` / `A`
//!   no longer links production code to a test file (gson, zod).
//! - #22: Python imports under `if TYPE_CHECKING:` (and TypeScript
//!   `import type`) are `type_import` edges and stay out of cycles (requests).
//! - #23: absolute Python imports of the repo's own packages resolve against
//!   the package root (ManimSimpleRender).
//!
//! Each fixture is a few small files shaped like the real case, not a copy
//! of the repo.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use repolens_app::graph::{GraphBuildResult, GraphBuilder};
use repolens_app::graph_analysis::GraphAnalyzer;
use repolens_app::scan::ScanService;
use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::graph::FileGraph;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn read_entries(pack_dir: &Path) -> Vec<FileEntry> {
    let mut chunks: Vec<_> = fs::read_dir(pack_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("files-"))
        .map(|e| e.path())
        .collect();
    chunks.sort();
    chunks
        .iter()
        .flat_map(|c| {
            serde_json::from_str::<Vec<FileEntry>>(&fs::read_to_string(c).unwrap()).unwrap()
        })
        .collect()
}

fn build(root: &Path) -> GraphBuildResult {
    let scan = ScanService::new().run(root, false, 100).unwrap();
    let entries = read_entries(&scan.pack_dir);
    GraphBuilder::new().build(&entries, &scan.pack_dir, Some(root))
}

fn has_edge(g: &FileGraph, source: &str, target: &str, edge_type: &str) -> bool {
    g.edges
        .iter()
        .any(|e| e.source == source && e.target == target && e.edge_type == edge_type)
}

fn edge_list(g: &FileGraph) -> Vec<String> {
    g.edges
        .iter()
        .map(|e| format!("{} -> {} ({})", e.source, e.target, e.edge_type))
        .collect()
}

fn orphans(g: &FileGraph) -> HashSet<String> {
    GraphAnalyzer::find_orphans(g, &HashSet::new())
        .orphan_files
        .into_iter()
        .collect()
}

// ---------------------------------------------------------------------------
// #21 — gson: bare names resolved to test files
// ---------------------------------------------------------------------------

const GSON_MAIN: &str = "gson/src/main/java/com/google/gson";
const GSON_TEST: &str = "gson/src/test/java/com/google/gson";
const EXTRAS_MAIN: &str = "extras/src/main/java/com/google/gson/typeadapters";
const EXTRAS_TEST: &str = "extras/src/test/java/com/google/gson/typeadapters";

/// gson @ e2292f9: `FieldAttributes` uses `java.lang.reflect.Field`, and
/// `RuntimeTypeAdapterFactory` uses `Map.Entry`, `entry.getValue()` and
/// `equals`. The only repo files declaring `Field`, `Entry`, `getValue` and
/// `equals` are tests, and `src/main` never imports them.
fn gson_fixture(root: &Path) {
    write(
        root,
        &format!("{GSON_MAIN}/FieldAttributes.java"),
        "package com.google.gson;\n\nimport java.lang.reflect.Field;\n\npublic final class FieldAttributes {\n  private final Field field;\n\n  public FieldAttributes(Field f) {\n    this.field = f;\n  }\n\n  public boolean sameAs(Object other) {\n    return field.equals(other);\n  }\n}\n",
    );
    // Same package, no import needed: this edge must stay.
    write(
        root,
        &format!("{GSON_MAIN}/Gson.java"),
        "package com.google.gson;\n\nimport com.google.gson.internal.Excluder;\n\npublic final class Gson {\n  private Excluder excluder;\n  private FieldAttributes attributes;\n}\n",
    );
    write(
        root,
        &format!("{GSON_MAIN}/internal/Excluder.java"),
        "package com.google.gson.internal;\n\npublic final class Excluder {\n  public boolean excludeClass(Class<?> c) {\n    return false;\n  }\n}\n",
    );
    write(
        root,
        &format!("{GSON_TEST}/functional/ParameterizedTypesTest.java"),
        "package com.google.gson.functional;\n\nimport com.google.gson.Gson;\n\npublic class ParameterizedTypesTest {\n  private Gson gson;\n\n  private static class Field {\n    int value;\n  }\n}\n",
    );
    write(
        root,
        &format!("{GSON_TEST}/functional/CollectionTest.java"),
        "package com.google.gson.functional;\n\nimport com.google.gson.Gson;\n\npublic class CollectionTest {\n  private Gson gson;\n\n  private static class Entry {\n    int value;\n  }\n}\n",
    );
    write(
        root,
        &format!("{GSON_TEST}/ParameterizedTypeFixtures.java"),
        "package com.google.gson;\n\npublic class ParameterizedTypeFixtures {\n  public static class MyParameterizedType {\n    public Object getValue() {\n      return null;\n    }\n  }\n}\n",
    );
    write(
        root,
        &format!("{EXTRAS_MAIN}/RuntimeTypeAdapterFactory.java"),
        "package com.google.gson.typeadapters;\n\nimport com.google.gson.Gson;\nimport java.util.Map;\n\npublic final class RuntimeTypeAdapterFactory {\n  private Class<?> baseType;\n\n  void read(Gson gson, Map<String, String> labels, Class<?> type) {\n    for (Map.Entry<String, String> entry : labels.entrySet()) {\n      entry.getValue();\n    }\n    baseType.equals(type);\n  }\n}\n",
    );
    // Same Java package as the factory, but in the test source set, which
    // production code can't see.
    write(
        root,
        &format!("{EXTRAS_TEST}/PostConstructAdapterFactoryTest.java"),
        "package com.google.gson.typeadapters;\n\npublic class PostConstructAdapterFactoryTest {\n  @Override\n  public boolean equals(Object o) {\n    return false;\n  }\n}\n",
    );
}

#[test]
fn gson_bare_names_do_not_link_main_to_test() {
    let dir = tempfile::tempdir().unwrap();
    gson_fixture(dir.path());
    let g = build(dir.path()).graph;

    let main_to_test: Vec<String> = g
        .edges
        .iter()
        .filter(|e| e.source.contains("/src/main/") && e.target.contains("/src/test/"))
        .map(|e| format!("{} -> {} ({})", e.source, e.target, e.edge_type))
        .collect();
    assert!(
        main_to_test.is_empty(),
        "src/main must not depend on src/test: {main_to_test:?}"
    );

    // Real edges are kept: an import, and a same-package type reference.
    assert!(
        has_edge(
            &g,
            &format!("{GSON_MAIN}/Gson.java"),
            &format!("{GSON_MAIN}/internal/Excluder.java"),
            "imports"
        ),
        "{:?}",
        edge_list(&g)
    );
    assert!(
        has_edge(
            &g,
            &format!("{GSON_MAIN}/Gson.java"),
            &format!("{GSON_MAIN}/FieldAttributes.java"),
            "type_ref"
        ),
        "same-package reference needs no import: {:?}",
        edge_list(&g)
    );

    let cycles = GraphAnalyzer::find_cycles(&g);
    assert_eq!(cycles.total_cycles, 0, "{:?}", cycles.cycles);
}

// ---------------------------------------------------------------------------
// #22 — requests: cycle closed only by `if TYPE_CHECKING:` imports
// ---------------------------------------------------------------------------

const REQ: &str = "src/requests";

/// psf/requests @ 611c616: models imports auth/cookies/hooks/exceptions at
/// runtime; each of those imports `.models` only inside `if TYPE_CHECKING:`.
fn requests_fixture(root: &Path) {
    write(root, "pyproject.toml", "[project]\nname = \"requests\"\n");
    write(
        root,
        &format!("{REQ}/__init__.py"),
        "from . import utils\nfrom .models import PreparedRequest, Request, Response\n",
    );
    write(
        root,
        &format!("{REQ}/models.py"),
        "\"\"\"\nrequests.models\n\nfrom .models import nothing  (docstring, not an import)\n\"\"\"\nimport datetime\n\nfrom .auth import HTTPBasicAuth\nfrom .cookies import (\n    _copy_cookie_jar,\n    cookiejar_from_dict,\n)\nfrom .exceptions import HTTPError\nfrom .hooks import default_hooks\nfrom .utils import guess_filename\n\n\nclass Request:\n    pass\n\n\nclass PreparedRequest:\n    pass\n\n\nclass Response:\n    pass\n",
    );
    write(
        root,
        &format!("{REQ}/auth.py"),
        "from __future__ import annotations\n\nfrom typing import TYPE_CHECKING\n\nfrom ._internal_utils import to_native_string\n\nif TYPE_CHECKING:\n    from .models import PreparedRequest\n\n\nclass HTTPBasicAuth:\n    def __call__(self, r: PreparedRequest) -> PreparedRequest:\n        return r\n",
    );
    write(
        root,
        &format!("{REQ}/cookies.py"),
        "import typing\n\nif typing.TYPE_CHECKING:\n    from .models import (\n        PreparedRequest,\n        Request,\n    )\nelse:\n    from ._internal_utils import to_native_string\n\n\ndef cookiejar_from_dict(d):\n    return d\n\n\ndef _copy_cookie_jar(j):\n    return j\n",
    );
    write(
        root,
        &format!("{REQ}/exceptions.py"),
        "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:  # pragma: no cover\n    from .models import Response\n\n\nclass HTTPError(IOError):\n    pass\n",
    );
    write(
        root,
        &format!("{REQ}/hooks.py"),
        "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from .models import Response\n\n\ndef default_hooks():\n    return {}\n",
    );
    write(
        root,
        &format!("{REQ}/utils.py"),
        "from typing import TYPE_CHECKING\n\nfrom ._internal_utils import to_native_string\n\nif TYPE_CHECKING:\n    from .models import PreparedRequest\n\n\ndef guess_filename(obj):\n    return None\n",
    );
    write(
        root,
        &format!("{REQ}/_internal_utils.py"),
        "def to_native_string(s):\n    return s\n",
    );
}

#[test]
fn requests_type_checking_imports_do_not_form_a_cycle() {
    let dir = tempfile::tempdir().unwrap();
    requests_fixture(dir.path());
    let g = build(dir.path()).graph;

    let models = format!("{REQ}/models.py");
    for f in ["auth", "cookies", "exceptions", "hooks", "utils"] {
        let file = format!("{REQ}/{f}.py");
        assert!(
            has_edge(&g, &models, &file, "imports"),
            "runtime import models -> {f}: {:?}",
            edge_list(&g)
        );
        assert!(
            has_edge(&g, &file, &models, "type_import"),
            "typing-only import {f} -> models must be marked type_import: {:?}",
            edge_list(&g)
        );
        assert!(
            !has_edge(&g, &file, &models, "imports"),
            "{f} -> models must not be a runtime import: {:?}",
            edge_list(&g)
        );
    }
    // The `else:` branch of `if TYPE_CHECKING:` runs: a normal import.
    assert!(has_edge(
        &g,
        &format!("{REQ}/cookies.py"),
        &format!("{REQ}/_internal_utils.py"),
        "imports"
    ));

    let cycles = GraphAnalyzer::find_cycles(&g);
    assert_eq!(
        cycles.total_cycles, 0,
        "only typing-only imports close the loop: {:?}",
        cycles.cycles
    );
    // A typing-only import still means the file is used.
    assert!(!orphans(&g).contains(&models));
}

// ---------------------------------------------------------------------------
// #23 — ManimSimpleRender: absolute imports of the repo's own packages
// ---------------------------------------------------------------------------

/// frankxue831/ManimSimpleRender @ c829728: `explainer/render/adapter.py`
/// imports `app.config.loader`, `app.render.command_builder` and
/// `app.render.runner` by absolute path; before the fix none resolved.
fn manim_fixture(root: &Path) {
    write(
        root,
        "pyproject.toml",
        "[project]\nname = \"manim-simple-render\"\n",
    );
    write(root, "app/__init__.py", "");
    write(root, "app/config/__init__.py", "");
    write(
        root,
        "app/config/loader.py",
        "import json\nfrom pathlib import Path\n\n\ndef load_config(path):\n    return json.loads(Path(path).read_text())\n",
    );
    write(root, "app/render/__init__.py", "");
    write(
        root,
        "app/render/command_builder.py",
        "from app.config.loader import load_config\n\n\ndef build_command(cfg):\n    return [\"manim\"]\n",
    );
    write(
        root,
        "app/render/runner.py",
        "import subprocess\n\nfrom app.render.command_builder import build_command\n\n\nclass RenderRunner:\n    def run(self, cfg):\n        return subprocess.run(build_command(cfg))\n",
    );
    write(root, "explainer/__init__.py", "");
    write(root, "explainer/render/__init__.py", "");
    write(
        root,
        "explainer/render/adapter.py",
        "\"\"\"Adapter from storyboard to the render pipeline.\"\"\"\n\nfrom __future__ import annotations\n\nimport os\nfrom dataclasses import dataclass\n\nfrom app.config.loader import load_config\nfrom app.render.command_builder import build_command\nfrom app.render.runner import RenderRunner\nfrom app.render import runner as runner_module\nimport app.render.runner\n\n\n@dataclass\nclass Adapter:\n    runner: RenderRunner\n",
    );
    write(
        root,
        "tests/test_runner.py",
        "import unittest\n\nfrom app.render.runner import RenderRunner\nfrom explainer.render.adapter import Adapter\n\n\nclass RunnerTest(unittest.TestCase):\n    pass\n",
    );
}

#[test]
fn manim_absolute_imports_resolve_from_the_package_root() {
    let dir = tempfile::tempdir().unwrap();
    manim_fixture(dir.path());
    let result = build(dir.path());
    let g = &result.graph;

    let adapter = "explainer/render/adapter.py";
    for target in [
        "app/config/loader.py",
        "app/render/command_builder.py",
        "app/render/runner.py",
    ] {
        assert!(
            has_edge(g, adapter, target, "imports"),
            "{adapter} -> {target}: {:?}",
            edge_list(g)
        );
    }
    assert!(has_edge(
        g,
        "app/render/runner.py",
        "app/render/command_builder.py",
        "imports"
    ));
    assert!(has_edge(
        g,
        "tests/test_runner.py",
        "explainer/render/adapter.py",
        "imports"
    ));

    // stdlib (`os`, `json`, `pathlib`, `dataclasses`, `subprocess`) and
    // imported names that are classes or functions are external, not
    // unresolved.
    let per_lang = result.metrics.per_language.as_ref().unwrap();
    assert_eq!(
        per_lang.get("Python").map(|v| v[1]),
        Some(0),
        "unresolved Python relationships: {per_lang:?}"
    );

    // The imported modules, and the packages that contain them, are used.
    let o = orphans(g);
    for used in [
        "app/config/loader.py",
        "app/render/command_builder.py",
        "app/render/runner.py",
        "app/__init__.py",
        "app/render/__init__.py",
        "explainer/render/adapter.py",
    ] {
        assert!(!o.contains(used), "{used} wrongly reported unused: {o:?}");
    }
}

/// `src/` layout: `import mypkg.core` resolves to `src/mypkg/core.py`.
#[test]
fn python_absolute_imports_resolve_in_src_layout() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "pyproject.toml", "[project]\nname = \"mypkg\"\n");
    write(
        root,
        "src/mypkg/__init__.py",
        "from mypkg.core import run\n",
    );
    write(root, "src/mypkg/core.py", "def run():\n    pass\n");
    write(root, "tests/test_core.py", "import mypkg.core\n");
    let g = build(root).graph;
    assert!(has_edge(
        &g,
        "src/mypkg/__init__.py",
        "src/mypkg/core.py",
        "imports"
    ));
    assert!(has_edge(
        &g,
        "tests/test_core.py",
        "src/mypkg/core.py",
        "imports"
    ));
}

// ---------------------------------------------------------------------------
// zod: generic `A` resolved to a test file (#21), TS `.js` specifiers,
// TS `import type` (same idea as #22)
// ---------------------------------------------------------------------------

const ZOD: &str = "packages/zod/src/v4";

/// colinhacks/zod: the reported 14-file High cycle ran through
/// `classic/tests/lazy.test.ts`, which nothing imports. Production files'
/// generic type parameter `A` was matched by name to the `type A` that test
/// declares. Imports use the TS ESM style (`./util.js` for `util.ts`).
fn zod_fixture(root: &Path) {
    write(
        root,
        &format!("{ZOD}/core/util.ts"),
        "import type { $ZodType } from \"./schemas.js\";\n\nexport function clone<A>(inst: A): A {\n  return inst;\n}\n\nexport type Schema = $ZodType;\n",
    );
    write(
        root,
        &format!("{ZOD}/core/schemas.ts"),
        "import * as util from \"./util.js\";\n\nexport interface $ZodType {\n  _zod: unknown;\n}\n\nexport function lazy<A>(getter: () => A): A {\n  return util.clone(getter());\n}\n",
    );
    write(
        root,
        &format!("{ZOD}/classic/tests/lazy.test.ts"),
        "import { expect, test } from \"vitest\";\nimport * as core from \"../../core/schemas.js\";\n\ntype A = { a: A | null };\n\ntest(\"lazy\", () => {\n  const a: A = { a: null };\n  expect(core.lazy(() => a)).toBe(a);\n});\n",
    );
}

#[test]
fn zod_generic_names_and_type_imports_do_not_form_a_cycle() {
    let dir = tempfile::tempdir().unwrap();
    zod_fixture(dir.path());
    let g = build(dir.path()).graph;

    let test_file = format!("{ZOD}/classic/tests/lazy.test.ts");
    let into_test: Vec<String> = g
        .edges
        .iter()
        .filter(|e| e.target == test_file)
        .map(|e| format!("{} -> {} ({})", e.source, e.target, e.edge_type))
        .collect();
    assert!(
        into_test.is_empty(),
        "nothing imports the test: {into_test:?}"
    );

    let schemas = format!("{ZOD}/core/schemas.ts");
    let util = format!("{ZOD}/core/util.ts");
    // `./util.js` names the emitted file; the source is util.ts.
    assert!(
        has_edge(&g, &schemas, &util, "imports"),
        "{:?}",
        edge_list(&g)
    );
    assert!(has_edge(&g, &test_file, &schemas, "imports"));
    // `import type` is erased at compile time.
    assert!(
        has_edge(&g, &util, &schemas, "type_import"),
        "{:?}",
        edge_list(&g)
    );

    let cycles = GraphAnalyzer::find_cycles(&g);
    assert_eq!(cycles.total_cycles, 0, "{:?}", cycles.cycles);
}

// ---------------------------------------------------------------------------
// Script-folder imports: a script outside any package imports a module that
// sits beside it (Python puts the script's folder on sys.path)
// ---------------------------------------------------------------------------

const SCRIPTS: &str = ".github/scripts";

/// frankxue831/gm-crypto-rs: `.github/scripts/v115_replay.py:16` has
/// `from v115_collect import Collector, ApiError, REPOSITORY`, with
/// `v115_collect.py` in the same folder and no `__init__.py`.
fn script_sibling_fixture(root: &Path) {
    write(
        root,
        &format!("{SCRIPTS}/v115_replay.py"),
        "import argparse\nimport json\n\nfrom v115_collect import Collector, ApiError, REPOSITORY\nimport v115_gate_rules as rules\nfrom app.config import load\n",
    );
    write(
        root,
        &format!("{SCRIPTS}/v115_collect.py"),
        "REPOSITORY = \"x/y\"\n\n\nclass ApiError(Exception):\n    pass\n\n\nclass Collector:\n    pass\n",
    );
    write(
        root,
        &format!("{SCRIPTS}/v115_gate_rules.py"),
        "RULE_VERSION = 1\n",
    );
    // Same module path exists at the package root and beside the script:
    // the script's own folder wins (it is first on sys.path).
    write(root, "app/__init__.py", "");
    write(root, "app/config.py", "def load():\n    pass\n");
    write(
        root,
        &format!("{SCRIPTS}/app/config.py"),
        "def load():\n    pass\n",
    );
    // Inside a package (has __init__.py), `import helpers` is NOT resolved
    // against the module's own folder: Python 3 has no implicit relative
    // imports.
    write(root, "lib/__init__.py", "");
    write(root, "lib/core.py", "import helpers\n");
    write(root, "lib/helpers.py", "X = 1\n");
}

#[test]
fn script_imports_of_sibling_modules_resolve() {
    let dir = tempfile::tempdir().unwrap();
    script_sibling_fixture(dir.path());
    let result = build(dir.path());
    let g = &result.graph;

    let replay = format!("{SCRIPTS}/v115_replay.py");
    assert!(
        has_edge(g, &replay, &format!("{SCRIPTS}/v115_collect.py"), "imports"),
        "{:?}",
        edge_list(g)
    );
    assert!(
        has_edge(
            g,
            &replay,
            &format!("{SCRIPTS}/v115_gate_rules.py"),
            "imports"
        ),
        "{:?}",
        edge_list(g)
    );
    // The script folder takes precedence over the package root.
    assert!(
        has_edge(g, &replay, &format!("{SCRIPTS}/app/config.py"), "imports"),
        "{:?}",
        edge_list(g)
    );
    assert!(
        !has_edge(g, &replay, "app/config.py", "imports"),
        "{:?}",
        edge_list(g)
    );
    // No sibling fallback inside a package.
    assert!(
        !has_edge(g, "lib/core.py", "lib/helpers.py", "imports"),
        "{:?}",
        edge_list(g)
    );
    // stdlib (`argparse`, `json`) with no file beside the script: external.
    let from_replay: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.source == replay)
        .map(|e| e.target.as_str())
        .collect();
    assert_eq!(from_replay.len(), 3, "{from_replay:?}");
}

/// QA on #28: a script outside any package and the package root both have a
/// module with the same name. Python loads the one beside the script (its
/// folder is `sys.path[0]`); a module inside a package still resolves from the
/// package root.
#[test]
fn script_folder_module_wins_over_package_root_module_of_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "pyproject.toml", "[project]\nname = \"demo\"\n");
    write(root, "src/util.py", "X = 1\n");
    write(root, "src/demo/__init__.py", "");
    write(root, "src/demo/core.py", "import util\n");
    write(root, "tools/util.py", "Y = 2\n");
    write(root, "tools/run.py", "import util\nfrom util import Y\n");
    let result = build(root);
    let g = &result.graph;

    assert!(
        has_edge(g, "tools/run.py", "tools/util.py", "imports"),
        "{:?}",
        edge_list(g)
    );
    assert!(
        !has_edge(g, "tools/run.py", "src/util.py", "imports"),
        "{:?}",
        edge_list(g)
    );
    // Inside a package: no script-folder lookup, package root as before.
    assert!(
        has_edge(g, "src/demo/core.py", "src/util.py", "imports"),
        "{:?}",
        edge_list(g)
    );
}

/// #31: a relative import of a path that isn't in the scan (gm-crypto-web's
/// `www/app.js` -> `./pkg/gm_web.js`, a build output) is recorded in the
/// graph metrics; resolved and library imports are not.
#[test]
fn unresolved_relative_imports_are_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "www/app.js",
        "import init, * as g from './pkg/gm_web.js';\nimport { h } from './helper.js';\nimport React from 'react';\nexport const run = () => g.sm4(h);\n",
    );
    write(root, "www/helper.js", "export const h = 1;\n");
    let result = build(root);
    let u = result
        .metrics
        .unresolved_imports
        .expect("unresolved imports recorded");
    assert_eq!(u.count, 1, "{u:?}");
    assert_eq!(u.sample[0].source, "www/app.js");
    assert_eq!(u.sample[0].import, "./pkg/gm_web.js");
}
