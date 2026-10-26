//! Evidence for the Language Classification observation invariants named in
//! `docs/superpowers/specs/2026-04-14-language-and-profile-catalogue.md`.
//!
//! Any rename, deletion, or `#[ignore]`-ing of a test in this file MUST be
//! accompanied by a tier demotion of the Language Classification family in
//! `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`
//! §5 and a corresponding update to the catalogue spec.

use repolens_extract::lang::detect;
use std::path::Path;

#[test]
fn detects_java() {
    assert_eq!(detect(Path::new("Foo.java")), "Java");
}

#[test]
fn detects_javascript_variants() {
    assert_eq!(detect(Path::new("app.js")), "JavaScript");
    assert_eq!(detect(Path::new("app.jsx")), "JavaScript");
    assert_eq!(detect(Path::new("app.mjs")), "JavaScript");
    assert_eq!(detect(Path::new("app.cjs")), "JavaScript");
}

#[test]
fn detects_typescript_variants() {
    assert_eq!(detect(Path::new("app.ts")), "TypeScript");
    assert_eq!(detect(Path::new("app.tsx")), "TypeScript");
    assert_eq!(detect(Path::new("app.mts")), "TypeScript");
}

#[test]
fn detects_python() {
    assert_eq!(detect(Path::new("main.py")), "Python");
}

#[test]
fn detects_rust() {
    assert_eq!(detect(Path::new("lib.rs")), "Rust");
}

#[test]
fn detects_go() {
    assert_eq!(detect(Path::new("main.go")), "Go");
}

#[test]
fn detects_c_and_cpp() {
    assert_eq!(detect(Path::new("main.c")), "C");
    assert_eq!(detect(Path::new("util.h")), "C");
    assert_eq!(detect(Path::new("main.cpp")), "C++");
    assert_eq!(detect(Path::new("main.cc")), "C++");
    assert_eq!(detect(Path::new("main.cxx")), "C++");
    assert_eq!(detect(Path::new("main.hpp")), "C++");
}

#[test]
fn detects_shell_variants() {
    assert_eq!(detect(Path::new("run.sh")), "Shell");
    assert_eq!(detect(Path::new("run.bash")), "Shell");
    assert_eq!(detect(Path::new("run.zsh")), "Shell");
}

#[test]
fn detects_markup_and_config() {
    assert_eq!(detect(Path::new("README.md")), "Markdown");
    assert_eq!(detect(Path::new("page.html")), "HTML");
    assert_eq!(detect(Path::new("page.htm")), "HTML");
    assert_eq!(detect(Path::new("data.json")), "JSON");
    assert_eq!(detect(Path::new("config.yml")), "YAML");
    assert_eq!(detect(Path::new("config.yaml")), "YAML");
    assert_eq!(detect(Path::new("layout.xml")), "XML");
    assert_eq!(detect(Path::new("config.toml")), "TOML");
}

#[test]
fn detects_kotlin_variants() {
    assert_eq!(detect(Path::new("Main.kt")), "Kotlin");
    assert_eq!(detect(Path::new("build.kts")), "Kotlin");
}

#[test]
fn detects_web_frameworks() {
    assert_eq!(detect(Path::new("App.vue")), "Vue");
    assert_eq!(detect(Path::new("App.svelte")), "Svelte");
}

#[test]
fn detects_css_variants() {
    assert_eq!(detect(Path::new("style.css")), "CSS");
    assert_eq!(detect(Path::new("style.scss")), "SCSS");
    assert_eq!(detect(Path::new("style.less")), "LESS");
}

#[test]
fn detects_misc_languages() {
    assert_eq!(detect(Path::new("query.sql")), "SQL");
    assert_eq!(detect(Path::new("build.gradle")), "Groovy");
    assert_eq!(detect(Path::new("Build.groovy")), "Groovy");
    assert_eq!(detect(Path::new("server.rb")), "Ruby");
    assert_eq!(detect(Path::new("App.swift")), "Swift");
    assert_eq!(detect(Path::new("App.scala")), "Scala");
    assert_eq!(detect(Path::new("App.dart")), "Dart");
    assert_eq!(detect(Path::new("main.zig")), "Zig");
    assert_eq!(detect(Path::new("analysis.r")), "R");
    assert_eq!(detect(Path::new("script.lua")), "Lua");
    assert_eq!(detect(Path::new("index.php")), "PHP");
    assert_eq!(detect(Path::new("app.properties")), "Properties");
}

#[test]
fn unknown_extension() {
    assert_eq!(detect(Path::new("Makefile")), "Unknown");
    assert_eq!(detect(Path::new("readme.xyz")), "Unknown");
}

#[test]
fn no_extension() {
    assert_eq!(detect(Path::new("Dockerfile")), "Unknown");
}

#[test]
fn case_insensitive() {
    assert_eq!(detect(Path::new("Main.JAVA")), "Java");
    assert_eq!(detect(Path::new("app.Py")), "Python");
}
