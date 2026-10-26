use std::path::Path;

/// Detects the programming language of a file based on its extension.
///
/// Returns a `&'static str` language name matching the Java `LanguageDetector` contract.
/// Returns `"Unknown"` for unrecognized extensions or files without extensions.
pub fn detect(path: &Path) -> &'static str {
    let ext = match path.extension() {
        Some(e) => e.to_string_lossy().to_lowercase(),
        None => return "Unknown",
    };

    match ext.as_str() {
        // JVM
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "scala" => "Scala",
        "gradle" | "groovy" => "Groovy",

        // JavaScript / TypeScript
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",

        // Systems
        "rs" => "Rust",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "go" => "Go",
        "swift" => "Swift",
        "zig" => "Zig",
        "dart" => "Dart",

        // Scripting
        "py" => "Python",
        "rb" => "Ruby",
        "php" => "PHP",
        "lua" => "Lua",
        "r" => "R",
        "sh" | "bash" | "zsh" => "Shell",

        // Data / query
        "sql" => "SQL",
        "json" => "JSON",
        "xml" => "XML",
        "yml" | "yaml" => "YAML",
        "toml" => "TOML",
        "properties" => "Properties",

        // Markup / style
        "md" => "Markdown",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" => "SCSS",
        "less" => "LESS",

        // Frameworks
        "vue" => "Vue",
        "svelte" => "Svelte",

        _ => "Unknown",
    }
}
