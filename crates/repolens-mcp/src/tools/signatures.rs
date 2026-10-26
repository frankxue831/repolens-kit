/// Extract declaration-level lines from source code, stripping function/method bodies.
///
/// Returns a condensed view containing imports, type/struct/class declarations,
/// function/method signatures, and module-level constants — but not implementation
/// bodies. Uses brace-depth tracking to skip nested code blocks.
pub fn extract_signatures(lines: &[&str], language: &str) -> String {
    match language {
        "Java" => extract_java_signatures(lines),
        "Rust" => extract_rust_signatures(lines),
        "Go" => extract_go_signatures(lines),
        "Python" => extract_python_signatures(lines),
        "JavaScript" | "TypeScript" => extract_js_signatures(lines),
        _ => extract_generic_signatures(lines),
    }
}

/// Java: keep package, imports, class/interface/enum declarations, method signatures.
/// Skip method bodies via brace tracking.
fn extract_java_signatures(lines: &[&str]) -> String {
    let mut out = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut in_body = false;
    let mut body_start_depth: i32 = 0;

    for line in lines {
        let trimmed = line.trim();

        // Always keep package and imports
        if trimmed.starts_with("package ") || trimmed.starts_with("import ") {
            out.push(*line);
            continue;
        }

        // Always keep annotations
        if trimmed.starts_with('@') {
            if !in_body {
                out.push(*line);
            }
            continue;
        }

        // Track braces
        let open = line.chars().filter(|&c| c == '{').count() as i32;
        let close = line.chars().filter(|&c| c == '}').count() as i32;

        if in_body {
            brace_depth += open - close;
            if brace_depth <= body_start_depth {
                in_body = false;
                // Emit closing brace with ellipsis
                if trimmed == "}" {
                    out.push("    // ...}");
                }
            }
            continue;
        }

        brace_depth += open - close;

        // Class/interface/enum declarations — keep and enter body tracking at class level
        if is_java_type_decl(trimmed) {
            out.push(*line);
            continue;
        }

        // Method signature: line with { that's inside a class
        if brace_depth >= 2 && open > 0 && is_java_method_sig(trimmed) {
            // Emit signature without the body
            let sig_end = line.find('{').unwrap_or(line.len());
            let sig = line[..sig_end].trim_end();
            out.push(sig);
            if open > close {
                in_body = true;
                body_start_depth = brace_depth - (open - close);
            }
            continue;
        }

        // Field declarations at class level
        if brace_depth >= 1 && trimmed.ends_with(';') && !trimmed.starts_with("//") {
            out.push(*line);
            continue;
        }

        // Closing braces at top level
        if trimmed == "}" && brace_depth <= 1 {
            out.push(*line);
        }
    }

    out.join("\n")
}

fn is_java_type_decl(trimmed: &str) -> bool {
    let keywords = ["class ", "interface ", "enum ", "record ", "@interface "];
    let modifiers = [
        "public ",
        "private ",
        "protected ",
        "abstract ",
        "final ",
        "static ",
        "sealed ",
    ];
    let rest = strip_modifiers(trimmed, &modifiers);
    keywords.iter().any(|k| rest.contains(k))
}

fn is_java_method_sig(trimmed: &str) -> bool {
    // Method signatures contain parentheses and are not control flow
    trimmed.contains('(')
        && !trimmed.starts_with("if ")
        && !trimmed.starts_with("if(")
        && !trimmed.starts_with("for ")
        && !trimmed.starts_with("for(")
        && !trimmed.starts_with("while ")
        && !trimmed.starts_with("while(")
        && !trimmed.starts_with("switch ")
        && !trimmed.starts_with("switch(")
        && !trimmed.starts_with("try ")
        && !trimmed.starts_with("catch ")
        && !trimmed.starts_with("catch(")
        && !trimmed.starts_with("//")
}

/// Rust: keep use/mod/pub declarations, fn/struct/enum/trait/impl signatures.
fn extract_rust_signatures(lines: &[&str]) -> String {
    let mut out = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut in_body = false;
    let mut body_start_depth: i32 = 0;

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with("//") {
            if !in_body && !trimmed.is_empty() {
                out.push(*line);
            }
            continue;
        }

        let open = line.chars().filter(|&c| c == '{').count() as i32;
        let close = line.chars().filter(|&c| c == '}').count() as i32;

        if in_body {
            brace_depth += open - close;
            if brace_depth <= body_start_depth {
                in_body = false;
            }
            continue;
        }

        brace_depth += open - close;

        // Always keep use/mod/extern/attribute lines
        if trimmed.starts_with("use ")
            || trimmed.starts_with("pub use ")
            || trimmed.starts_with("mod ")
            || trimmed.starts_with("pub mod ")
            || trimmed.starts_with("pub(crate) mod ")
            || trimmed.starts_with("extern ")
            || trimmed.starts_with("#[")
            || trimmed.starts_with("#![")
        {
            out.push(*line);
            continue;
        }

        // Keep const/static/type declarations
        if (trimmed.starts_with("pub ")
            || trimmed.starts_with("pub(crate) ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("static ")
            || trimmed.starts_with("type "))
            && trimmed.ends_with(';')
        {
            out.push(*line);
            continue;
        }

        // Function/method signatures
        if is_rust_fn_sig(trimmed) {
            let sig_end = line.find('{').unwrap_or(line.len());
            let sig = line[..sig_end].trim_end();
            out.push(sig);
            if open > close {
                in_body = true;
                body_start_depth = brace_depth - (open - close);
            }
            continue;
        }

        // Struct/enum/trait/impl declarations
        if is_rust_type_decl(trimmed) {
            out.push(*line);
            // For struct/enum with body, skip the body
            if open > close && !trimmed.contains("impl") && !trimmed.contains("trait") {
                in_body = true;
                body_start_depth = brace_depth - (open - close);
            }
            continue;
        }

        // Keep field declarations inside structs at depth 1
        if brace_depth == 1 && (trimmed.ends_with(',') || trimmed.ends_with(';')) {
            out.push(*line);
            continue;
        }

        // Closing braces at top level
        if trimmed == "}" && brace_depth <= 0 {
            out.push(*line);
        }
    }

    out.join("\n")
}

fn is_rust_fn_sig(trimmed: &str) -> bool {
    let stripped = strip_modifiers(
        trimmed,
        &[
            "pub ",
            "pub(crate) ",
            "pub(super) ",
            "async ",
            "unsafe ",
            "const ",
            "default ",
        ],
    );
    stripped.starts_with("fn ")
}

fn is_rust_type_decl(trimmed: &str) -> bool {
    let stripped = strip_modifiers(trimmed, &["pub ", "pub(crate) ", "pub(super) ", "unsafe "]);
    stripped.starts_with("struct ")
        || stripped.starts_with("enum ")
        || stripped.starts_with("trait ")
        || stripped.starts_with("impl ")
        || stripped.starts_with("impl<")
}

/// Go: keep package, imports, type/func/var/const declarations.
fn extract_go_signatures(lines: &[&str]) -> String {
    let mut out = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut in_body = false;

    for line in lines {
        let trimmed = line.trim();

        let open = line.chars().filter(|&c| c == '{').count() as i32;
        let close = line.chars().filter(|&c| c == '}').count() as i32;

        if in_body {
            brace_depth += open - close;
            if brace_depth <= 0 {
                in_body = false;
            }
            continue;
        }

        brace_depth += open - close;

        if trimmed.starts_with("package ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("import (")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("var ")
            || trimmed.starts_with("const ")
        {
            out.push(*line);
            continue;
        }

        if trimmed.starts_with("func ") {
            let sig_end = line.find('{').unwrap_or(line.len());
            let sig = line[..sig_end].trim_end();
            out.push(sig);
            if open > close {
                in_body = true;
            }
            continue;
        }

        // Import block lines
        if brace_depth == 1
            && trimmed.starts_with('"')
            && (trimmed.ends_with('"') || trimmed.ends_with(')'))
        {
            out.push(*line);
        }
    }

    out.join("\n")
}

/// Python: keep imports, class/def signatures, decorators, module-level assignments.
/// Skips function bodies but preserves nested class/def declarations.
fn extract_python_signatures(lines: &[&str]) -> String {
    let mut out = Vec::new();
    // Stack of indent levels where we entered a def body to skip
    let mut skip_indent: Option<usize> = None;

    for line in lines {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.is_empty() {
            continue;
        }

        // If we're skipping a function body, check if we've dedented back
        if let Some(body_indent) = skip_indent {
            if indent > body_indent {
                // Still inside the body — but keep nested class/def declarations
                if trimmed.starts_with("class ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("async def ")
                    || trimmed.starts_with('@')
                {
                    out.push(*line);
                    if !trimmed.starts_with('@') {
                        skip_indent = Some(indent);
                    }
                }
                continue;
            }
            skip_indent = None;
        }

        // Always keep imports
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            out.push(*line);
            continue;
        }

        // Keep decorators
        if trimmed.starts_with('@') {
            out.push(*line);
            continue;
        }

        // Keep class declarations (don't skip class body — we want nested defs)
        if trimmed.starts_with("class ") {
            out.push(*line);
            continue;
        }

        // Keep function declarations, then skip body
        if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
            out.push(*line);
            skip_indent = Some(indent);
            continue;
        }

        // Module-level assignments (indent 0)
        if indent == 0 && trimmed.contains('=') && !trimmed.starts_with('#') {
            out.push(*line);
        }
    }

    out.join("\n")
}

/// JS/TS: keep imports, exports, class/function declarations.
fn extract_js_signatures(lines: &[&str]) -> String {
    let mut out = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut in_body = false;
    let mut body_start_depth: i32 = 0;

    for line in lines {
        let trimmed = line.trim();

        let open = line.chars().filter(|&c| c == '{').count() as i32;
        let close = line.chars().filter(|&c| c == '}').count() as i32;

        if in_body {
            brace_depth += open - close;
            if brace_depth <= body_start_depth {
                in_body = false;
            }
            continue;
        }

        brace_depth += open - close;

        // Keep imports and exports
        if trimmed.starts_with("import ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("require(")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("var ")
        {
            // For function assignments, strip the body
            if open > close && (trimmed.contains("=>") || trimmed.contains("function")) {
                let sig_end = line.find('{').unwrap_or(line.len());
                let sig = line[..sig_end].trim_end();
                out.push(sig);
                in_body = true;
                body_start_depth = brace_depth - (open - close);
                continue;
            }
            out.push(*line);
            continue;
        }

        // Function/class declarations
        if trimmed.starts_with("function ")
            || trimmed.starts_with("async function ")
            || trimmed.starts_with("class ")
        {
            let sig_end = line.find('{').unwrap_or(line.len());
            let sig = line[..sig_end].trim_end();
            out.push(sig);
            if open > close {
                in_body = true;
                body_start_depth = brace_depth - (open - close);
            }
            continue;
        }

        // Method definitions inside classes (at depth 1)
        if brace_depth >= 1 && trimmed.contains('(') && open > 0 {
            let stripped = strip_modifiers(trimmed, &["async ", "static ", "get ", "set "]);
            if stripped.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '#') {
                let sig_end = line.find('{').unwrap_or(line.len());
                let sig = line[..sig_end].trim_end();
                out.push(sig);
                if open > close {
                    in_body = true;
                    body_start_depth = brace_depth - (open - close);
                }
                continue;
            }
        }

        // Closing brace at top level
        if trimmed == "}" && brace_depth <= 0 {
            out.push(*line);
        }
    }

    out.join("\n")
}

/// Generic fallback: keep lines that look like declarations.
fn extract_generic_signatures(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && (trimmed.starts_with("import ")
                    || trimmed.starts_with("from ")
                    || trimmed.starts_with("use ")
                    || trimmed.starts_with("package ")
                    || trimmed.starts_with("module ")
                    || trimmed.starts_with("class ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("func ")
                    || trimmed.starts_with("function ")
                    || trimmed.starts_with("pub ")
                    || trimmed.starts_with("export ")
                    || trimmed.starts_with("type ")
                    || trimmed.starts_with("interface ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("enum ")
                    || trimmed.starts_with("trait ")
                    || trimmed.starts_with("const ")
                    || trimmed.starts_with("var ")
                    || trimmed.starts_with("let ")
                    || trimmed.starts_with('#'))
        })
        .copied()
        .collect::<Vec<&str>>()
        .join("\n")
}

fn strip_modifiers<'a>(s: &'a str, modifiers: &[&str]) -> &'a str {
    let mut rest = s;
    loop {
        let before = rest;
        for m in modifiers {
            if let Some(stripped) = rest.strip_prefix(m) {
                rest = stripped;
            }
        }
        if rest == before {
            break;
        }
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_signatures_strips_fn_bodies() {
        let source = r#"use crate::model::FileEntry;
use std::path::Path;

pub struct Config {
    pub name: String,
    pub debug: bool,
}

impl Config {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            debug: false,
        }
    }

    pub fn validate(&self) -> bool {
        !self.name.is_empty()
    }
}

pub fn run(config: &Config) {
    println!("{}", config.name);
    if config.debug {
        println!("debug mode");
    }
}"#;
        let lines: Vec<&str> = source.lines().collect();
        let result = extract_signatures(&lines, "Rust");

        assert!(result.contains("use crate::model::FileEntry;"));
        assert!(result.contains("pub struct Config"));
        assert!(result.contains("pub fn new(name: &str) -> Self"));
        assert!(result.contains("pub fn validate(&self) -> bool"));
        assert!(result.contains("pub fn run(config: &Config)"));
        // Bodies should be stripped
        assert!(!result.contains("println!"));
        assert!(!result.contains("is_empty"));
    }

    #[test]
    fn java_signatures_strips_method_bodies() {
        let source = r#"package com.example;

import java.util.List;

public class App {
    private String name;

    public App(String name) {
        this.name = name;
    }

    public String getName() {
        return name;
    }

    public static void main(String[] args) {
        App app = new App("test");
        System.out.println(app.getName());
    }
}"#;
        let lines: Vec<&str> = source.lines().collect();
        let result = extract_signatures(&lines, "Java");

        assert!(result.contains("package com.example;"));
        assert!(result.contains("import java.util.List;"));
        assert!(result.contains("public class App"));
        assert!(result.contains("private String name;"));
        assert!(result.contains("public App(String name)"));
        assert!(result.contains("public String getName()"));
        assert!(result.contains("public static void main(String[] args)"));
        // Bodies should be stripped
        assert!(!result.contains("this.name"));
        assert!(!result.contains("System.out.println"));
    }

    #[test]
    fn python_signatures_keeps_defs() {
        let source = r#"import os
from pathlib import Path

MAX_SIZE = 1024

class Config:
    def __init__(self, name):
        self.name = name
        self.debug = False

    def validate(self):
        return bool(self.name)

def run(config):
    print(config.name)
"#;
        let lines: Vec<&str> = source.lines().collect();
        let result = extract_signatures(&lines, "Python");

        assert!(result.contains("import os"));
        assert!(result.contains("from pathlib import Path"));
        assert!(result.contains("MAX_SIZE = 1024"));
        assert!(result.contains("class Config:"));
        assert!(result.contains("def __init__(self, name):"));
        assert!(result.contains("def validate(self):"));
        assert!(result.contains("def run(config):"));
        // Bodies should be stripped
        assert!(!result.contains("self.name = name"));
        assert!(!result.contains("print(config.name)"));
    }

    #[test]
    fn outline_is_cheaper_than_full() {
        let source = "fn main() {\n    println!(\"hello\");\n    let x = 42;\n    for i in 0..x {\n        println!(\"{i}\");\n    }\n}\n";
        let lines: Vec<&str> = source.lines().collect();
        let sigs = extract_signatures(&lines, "Rust");
        assert!(
            sigs.len() < source.len(),
            "signatures ({}) should be shorter than full ({})",
            sigs.len(),
            source.len()
        );
    }
}
