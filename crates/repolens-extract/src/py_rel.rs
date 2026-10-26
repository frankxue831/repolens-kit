use crate::relationship::{Relationship, TYPE_IMPORT};
use crate::RelationshipExtractor;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use tracing::warn;

/// Regex-based relationship extractor for Python files.
pub struct PyRelationshipExtractor;

// `from <module> import <names>`; module may be relative (`.`, `..x.y`) or absolute (`a.b`).
static FROM_IMPORT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^from\s+(\.*[\w.]*)\s+import\s+(.+)$").unwrap());

// `import a.b.c [as x], d`
static IMPORT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^import\s+(.+)$").unwrap());

// `if TYPE_CHECKING:` / `if typing.TYPE_CHECKING:` (also `t.TYPE_CHECKING`).
static TYPE_CHECKING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^if\s+(?:\w+\.)?TYPE_CHECKING\s*:\s*(?:#.*)?$").unwrap());

// Class with inheritance: class Foo(Bar, Baz):
static CLASS_EXTENDS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^class\s+\w+\(([\w\s,]+)\)\s*:").unwrap());

static IDENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z_]\w*$").unwrap());
static DOTTED_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z_]\w*(\.[A-Za-z_]\w*)*$").unwrap());

/// Python builtin base classes to filter from extends relationships.
static BUILTIN_BASES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "object",
        "Exception",
        "BaseException",
        "dict",
        "list",
        "tuple",
        "set",
        "frozenset",
        "str",
        "int",
        "float",
        "bool",
        "type",
        "ABC",
        "Protocol",
    ]
    .into_iter()
    .collect()
});

impl RelationshipExtractor for PyRelationshipExtractor {
    fn extract(&self, path: &Path) -> Vec<Relationship> {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to read {}: {e}", path.display());
                return Vec::new();
            }
        };
        extract_from_source(&source)
    }
}

/// One logical Python line: physical lines joined across open brackets and
/// trailing backslashes, with its starting indentation.
struct LogicalLine {
    indent: usize,
    text: String,
}

/// Split source into logical lines, skipping comments and the bodies of
/// triple-quoted strings (so a docstring that shows `from x import y` is not
/// read as an import).
fn logical_lines(source: &str) -> Vec<LogicalLine> {
    let mut out = Vec::new();
    let mut in_triple: Option<&'static str> = None;
    let mut pending: Option<LogicalLine> = None;
    let mut depth: i32 = 0;

    for raw in source.lines() {
        if let Some(q) = in_triple {
            if raw.matches(q).count() % 2 == 1 {
                in_triple = None;
            }
            continue;
        }
        let code = strip_comment(raw);
        let trimmed = code.trim();
        if pending.is_none() {
            if trimmed.is_empty() {
                continue;
            }
            for q in ["\"\"\"", "'''"] {
                if trimmed.matches(q).count() % 2 == 1 {
                    in_triple = Some(q);
                }
            }
            if in_triple.is_some() && (trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''"))
            {
                continue;
            }
            let indent = raw.len() - raw.trim_start().len();
            pending = Some(LogicalLine {
                indent,
                text: String::new(),
            });
        }
        let line = pending.as_mut().unwrap();
        let mut piece = trimmed.to_string();
        let continued = piece.ends_with('\\');
        if continued {
            piece.pop();
        }
        if !line.text.is_empty() {
            line.text.push(' ');
        }
        line.text.push_str(piece.trim());
        for c in trimmed.chars() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 && !continued {
            depth = 0;
            out.push(pending.take().unwrap());
        }
    }
    if let Some(line) = pending {
        out.push(line);
    }
    out
}

/// Drop a trailing `# comment` that is not inside a string literal.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev = '\0';
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q && prev != '\\' => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return &line[..i],
            None => {}
        }
        prev = c;
    }
    line
}

/// Extract relationships from Python source text.
pub fn extract_from_source(source: &str) -> Vec<Relationship> {
    let mut rels = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut push =
        |rel_type: &str, target: String, detail: String, rels: &mut Vec<Relationship>| {
            if seen.insert((rel_type.to_string(), target.clone(), detail.clone())) {
                rels.push(Relationship {
                    rel_type: rel_type.to_string(),
                    target_name: target,
                    detail,
                });
            }
        };

    // Indentation of each open `if TYPE_CHECKING:` block.
    let mut type_checking: Vec<usize> = Vec::new();

    for line in logical_lines(source) {
        while let Some(&block_indent) = type_checking.last() {
            if line.indent <= block_indent {
                type_checking.pop();
            } else {
                break;
            }
        }
        let text = line.text.as_str();
        if TYPE_CHECKING_RE.is_match(text) {
            type_checking.push(line.indent);
            continue;
        }
        let import_type = if type_checking.is_empty() {
            "imports"
        } else {
            TYPE_IMPORT
        };

        if let Some(caps) = FROM_IMPORT_RE.captures(text) {
            let module = &caps[1];
            let names = import_names(&caps[2]);
            if module.is_empty() {
                continue;
            }
            let dots = module.chars().take_while(|c| *c == '.').count();
            if dots > 0 {
                let rest = &module[dots..];
                let prefix = if dots == 1 {
                    "./".to_string()
                } else {
                    "../".repeat(dots - 1)
                };
                if rest.is_empty() {
                    // `from . import config`: each name is a sibling module.
                    for name in names {
                        push(
                            import_type,
                            name.clone(),
                            format!("{prefix}{name}"),
                            &mut rels,
                        );
                    }
                } else {
                    if !DOTTED_RE.is_match(rest) {
                        continue;
                    }
                    let detail = format!("{prefix}{}", rest.replace('.', "/"));
                    push(import_type, detail.clone(), detail, &mut rels);
                }
            } else {
                if !DOTTED_RE.is_match(module) {
                    continue;
                }
                // Absolute `from a.b import c`: the module itself, plus each
                // name in case it is a submodule (`a/b/c.py`). A name that is
                // not a module is dropped at resolution, not counted.
                push(
                    import_type,
                    module.to_string(),
                    module.to_string(),
                    &mut rels,
                );
                for name in names {
                    push(
                        import_type,
                        name.clone(),
                        format!("{module}.{name}"),
                        &mut rels,
                    );
                }
            }
            continue;
        }

        if let Some(caps) = IMPORT_RE.captures(text) {
            for part in caps[1].split(',') {
                let module = part.split_whitespace().next().unwrap_or("");
                if DOTTED_RE.is_match(module) {
                    push(
                        import_type,
                        module.to_string(),
                        module.to_string(),
                        &mut rels,
                    );
                }
            }
            continue;
        }

        // Class inheritance (top-level only)
        if line.indent == 0 {
            if let Some(caps) = CLASS_EXTENDS_RE.captures(text) {
                for base in caps[1].split(',') {
                    let base = base.trim();
                    if !base.is_empty() && !BUILTIN_BASES.contains(base) {
                        push("extends", base.to_string(), base.to_string(), &mut rels);
                    }
                }
            }
        }
    }

    rels
}

/// Names from the right-hand side of `from x import ...`, without aliases,
/// parentheses or `*`.
fn import_names(rhs: &str) -> Vec<String> {
    rhs.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .filter_map(|n| n.split_whitespace().next())
        .filter(|n| IDENT_RE.is_match(n))
        .map(|n| n.to_string())
        .collect()
}
