//! `repolens validate-plan --file <plan.md>` — deterministic warnings-only
//! plan linter.
//!
//! Reads a plan markdown file and `.repolens/known-constraints.md`, applies
//! a tiny set of regex/substring checks, and prints conservative warnings
//! about possible plan risks. Never certifies a plan valid — false negatives
//! are acceptable. Out of scope: AST validation, LLM/MCP, auto-grading,
//! memory writes, plan understanding beyond simple heuristics.

use anyhow::{anyhow, bail, Result};
use clap::Args;
use once_cell::sync::Lazy;
use regex::Regex;
use repolens_core::storage::RepoStoragePaths;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::brief::TEMPLATE_PLACEHOLDER_RE;
use crate::remember::parse_fence_marker;
use crate::resolve_root_from;

const HEADER: &str = "repolens validate-plan — warnings only. This is a guardrail, not proof.";

const DISCLAIMER_FINDINGS: &str = "RepoLens found possible plan risks. \
     These warnings do not prove the plan is invalid or valid. \
     Review against source files and tests.";

const DISCLAIMER_NO_FINDINGS: &str = "RepoLens found no possible plan risks. \
     This does not prove the plan is valid. \
     Review against source files and tests.";

const NO_CONSTRAINTS_NOTE: &str =
    "note: no real constraints loaded from .repolens/known-constraints.md \
     (file is missing, empty, or template-only). \
     Skipping constraint-aware checks (VP001, VP002, VP004).";

const KNOWN_CONSTRAINTS_FILENAME: &str = "known-constraints.md";

#[derive(Args, Debug)]
pub struct ValidatePlanArgs {
    /// Path to the plan markdown file to lint
    #[arg(long)]
    pub file: PathBuf,
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

pub fn run_validate_plan(args: ValidatePlanArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;

    let storage_dir = RepoStoragePaths::storage_dir(&root);
    if !storage_dir.is_dir() {
        bail!(
            "RepoLens workspace not found at {}. Run 'repolens init' first.",
            storage_dir.display()
        );
    }

    let plan_raw = fs::read_to_string(&args.file)
        .map_err(|e| anyhow!("Cannot read plan file {}: {}", args.file.display(), e))?;

    let constraints_path = storage_dir.join(KNOWN_CONSTRAINTS_FILENAME);
    let constraints_raw = match fs::read_to_string(&constraints_path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => bail!(
            "Cannot read constraints file {}: {}",
            constraints_path.display(),
            e
        ),
    };

    let plan = parse_plan(&plan_raw);
    let constraints = parse_constraints(&constraints_raw);
    let findings = run_checks(&plan, &constraints);

    print!(
        "{}",
        render_text(&findings, &constraints, &args.file.display().to_string())
    );

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanLine {
    lineno: usize,
    text: String,
    /// `text.to_ascii_lowercase()` cached so per-check substring scans don't
    /// re-allocate. Read-only after construction.
    lower: String,
    in_fence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Constraint {
    id: String,
    statement: String,
    heading_lineno: usize,
}

#[derive(Debug, Clone, Default)]
struct Constraints {
    entries: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConstraintRef {
    id: String,
    /// 1-based line number in `known-constraints.md`. `Some` when the
    /// warning is tied to a specific constraint heading; `None` when the
    /// warning lists multiple constraints (VP002).
    lineno: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Warning {
    id: &'static str,
    message: String,
    plan_lineno: Option<usize>,
    constraint: Option<ConstraintRef>,
    detail: Option<String>,
}

fn parse_plan(raw: &str) -> Vec<PlanLine> {
    let mut lines = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, line) in raw.lines().enumerate() {
        let marker = parse_fence_marker(line);
        // Treat the fence opener and closer lines as in_fence so an info
        // string like "```python" can't trip checks.
        let in_fence = match (fence, marker) {
            (Some((c, n)), Some((mc, mn))) if mc == c && mn >= n => {
                fence = None;
                true
            }
            (Some(_), _) => true,
            (None, Some((c, n))) => {
                fence = Some((c, n));
                true
            }
            (None, None) => false,
        };
        let text = line.to_string();
        let lower = text.to_ascii_lowercase();
        lines.push(PlanLine {
            lineno: i + 1,
            text,
            lower,
            in_fence,
        });
    }
    lines
}

static CONSTRAINT_HEADING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^###\s+(C-\d+)\s*(.*)$").unwrap());

/// Number of real `C-NNN` constraints `validate-plan` would enforce from
/// `raw` (the text of `known-constraints.md`). `repolens brief` uses this so
/// the two commands agree on what the file contains (#34).
pub(crate) fn enforced_constraint_count(raw: &str) -> usize {
    parse_constraints(raw).entries.len()
}

fn parse_constraints(raw: &str) -> Constraints {
    if raw.trim().is_empty() {
        return Constraints::default();
    }

    let lines: Vec<&str> = raw.lines().collect();
    let mut entries = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(cap) = CONSTRAINT_HEADING_RE.captures(line) else {
            continue;
        };
        let id = cap.get(1).unwrap().as_str().to_string();
        let rest = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        if heading_is_placeholder(rest) {
            continue;
        }

        let mut statement = String::new();
        let mut placeholder_statement = false;
        for next in lines.iter().skip(i + 1).take(10) {
            let l = next.trim();
            if let Some(rest_after_label) = l.strip_prefix("- **Statement:**") {
                let val = rest_after_label.trim();
                if heading_is_placeholder(val) {
                    placeholder_statement = true;
                } else {
                    statement = val
                        .trim_start_matches('_')
                        .trim_end_matches('_')
                        .to_string();
                }
                break;
            }
            if l.starts_with("##") {
                break;
            }
        }
        if placeholder_statement {
            continue;
        }
        entries.push(Constraint {
            id,
            statement,
            heading_lineno: i + 1,
        });
    }
    Constraints { entries }
}

/// True when `text` looks like template scaffold rather than a real value.
/// Catches `_<short-name>_`, `_<short-name>_ (retired YYYY-MM-DD)`, and
/// `_..._` — the placeholder shapes used in the pristine
/// `known-constraints.md` template. Reuses `brief::TEMPLATE_PLACEHOLDER_RE`
/// so the two modules cannot drift.
fn heading_is_placeholder(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty() && TEMPLATE_PLACEHOLDER_RE.is_match(t)
}

static SNAKE_CASE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b").unwrap());

const OUTPUT_KEYWORDS: &[&str] = &["field", "output", "schema", "json", "serializ", "serialise"];

fn lower_contains_output_keyword(lower: &str) -> bool {
    OUTPUT_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

fn nearby_output_keyword(lower: &str, token_byte_pos: usize, window: usize) -> bool {
    let start = token_byte_pos.saturating_sub(window);
    let end = (token_byte_pos + window).min(lower.len());
    lower_contains_output_keyword(&lower[start..end])
}

const VERIFICATION_PHRASES: &[&str] = &[
    "verify",
    "locate",
    "first read",
    "first check",
    "first find",
    "first look",
    "before editing",
    "before edit",
    "grep",
    "ripgrep",
    "rg ",
    "find the",
    "look up",
    "look at",
    "search for",
];

const FILE_OR_TYPE_NEEDLES: &[&str] = &[
    "crates/", "/src/", " src/", "/tests/", " tests/", ".rs", "struct ", "enum ", "trait ",
    "type ", "impl ", "::",
];

fn plan_mentions_verification(plan: &[PlanLine]) -> bool {
    plan.iter()
        .filter(|l| !l.in_fence)
        .any(|l| VERIFICATION_PHRASES.iter().any(|p| l.lower.contains(p)))
}

fn plan_names_file_or_type(plan: &[PlanLine]) -> bool {
    plan.iter()
        .any(|l| FILE_OR_TYPE_NEEDLES.iter().any(|n| l.lower.contains(n)))
}

fn plan_mentions_constraint_id(plan: &[PlanLine], id: &str) -> bool {
    let lower_id = id.to_ascii_lowercase();
    plan.iter().any(|l| l.lower.contains(&lower_id))
}

fn plan_mentions_any_output_keyword(plan: &[PlanLine]) -> bool {
    plan.iter()
        .filter(|l| !l.in_fence)
        .any(|l| lower_contains_output_keyword(&l.lower))
}

fn constraint_says_camelcase_for_output(c: &Constraint) -> bool {
    let s = c.statement.to_ascii_lowercase();
    s.contains("camelcase") && OUTPUT_KEYWORDS.iter().any(|kw| s.contains(kw))
}

/// Keywords that, if present anywhere in the plan, count as a test / snapshot
/// / doc mention and silence VP004. Short terms are matched as words so
/// unrelated text like `latest` or `doctor` cannot hide the warning.
static TEST_OR_DOC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\b(?:tests?|testing|tested|snapshot(?:s)?|insta|docs?|document(?:s|ed|ing|ation)?|readme|contract(?:s)?)\b",
    )
    .unwrap()
});

fn plan_mentions_test_or_doc(plan: &[PlanLine]) -> bool {
    plan.iter().any(|l| TEST_OR_DOC_RE.is_match(&l.lower))
}

fn check_vp001(plan: &[PlanLine], constraints: &Constraints) -> Vec<Warning> {
    let Some(c) = constraints
        .entries
        .iter()
        .find(|c| constraint_says_camelcase_for_output(c))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in plan {
        if line.in_fence {
            continue;
        }
        for m in SNAKE_CASE_RE.find_iter(&line.text) {
            if !nearby_output_keyword(&line.lower, m.start(), 80) {
                continue;
            }
            let token = m.as_str();
            out.push(Warning {
                id: "VP001",
                message: format!(
                    "possible snake_case identifier `{token}` near output/JSON/serialization \
                     language; this may violate {} (\"{}\")",
                    c.id, c.statement
                ),
                plan_lineno: Some(line.lineno),
                constraint: Some(ConstraintRef {
                    id: c.id.clone(),
                    lineno: Some(c.heading_lineno),
                }),
                detail: Some(format!("token `{}` (column {})", token, m.start() + 1)),
            });
        }
    }
    out
}

fn check_vp002(plan: &[PlanLine], constraints: &Constraints) -> Vec<Warning> {
    let applicable: Vec<&Constraint> = constraints
        .entries
        .iter()
        .filter(|c| constraint_says_camelcase_for_output(c))
        .collect();
    if applicable.is_empty() || !plan_mentions_any_output_keyword(plan) {
        return Vec::new();
    }
    let unmentioned: Vec<&Constraint> = applicable
        .into_iter()
        .filter(|c| !plan_mentions_constraint_id(plan, &c.id))
        .collect();
    if unmentioned.is_empty() {
        return Vec::new();
    }
    let ids: Vec<String> = unmentioned.iter().map(|c| c.id.clone()).collect();
    vec![Warning {
        id: "VP002",
        message: format!(
            "plan mentions output/JSON/field/schema/serialization language but does not cite the \
             applicable known constraint(s): {}. Consider citing the constraint id by name in the \
             plan so reviewers can confirm it was integrated.",
            ids.join(", ")
        ),
        plan_lineno: None,
        constraint: Some(ConstraintRef {
            id: ids.join(", "),
            lineno: None,
        }),
        detail: Some(format!(
            "applicable constraint(s): {}",
            unmentioned
                .iter()
                .map(|c| format!("{} (\"{}\")", c.id, c.statement))
                .collect::<Vec<_>>()
                .join("; ")
        )),
    }]
}

fn check_vp003(plan: &[PlanLine]) -> Vec<Warning> {
    if plan_names_file_or_type(plan) || plan_mentions_verification(plan) {
        return Vec::new();
    }
    vec![Warning {
        id: "VP003",
        message: "plan does not name an affected file/module/type/struct and does not state an \
                  explicit verification step. Consider naming the affected file or stating a \
                  step like \"first locate / verify the output type before editing\"."
            .to_string(),
        plan_lineno: None,
        constraint: None,
        detail: None,
    }]
}

fn check_vp004(plan: &[PlanLine], constraints: &Constraints) -> Vec<Warning> {
    let has_camel_constraint = constraints
        .entries
        .iter()
        .any(constraint_says_camelcase_for_output);
    if !has_camel_constraint {
        return Vec::new();
    }
    if !plan_mentions_any_output_keyword(plan) {
        return Vec::new();
    }
    if plan_mentions_test_or_doc(plan) {
        return Vec::new();
    }
    vec![Warning {
        id: "VP004",
        message: "plan changes an output contract but does not mention tests / snapshots / docs \
                  that exercise that contract. Consider naming the affected snapshot tests, \
                  JSON-shape integration tests, or contract docs that the change should keep in \
                  sync."
            .to_string(),
        plan_lineno: None,
        constraint: None,
        detail: None,
    }]
}

fn run_checks(plan: &[PlanLine], constraints: &Constraints) -> Vec<Warning> {
    let mut all = Vec::new();
    all.extend(check_vp001(plan, constraints));
    all.extend(check_vp002(plan, constraints));
    all.extend(check_vp003(plan));
    all.extend(check_vp004(plan, constraints));
    all.sort_by(|a, b| {
        a.id.cmp(b.id)
            .then_with(|| a.plan_lineno.cmp(&b.plan_lineno))
    });
    all
}

fn render_text(findings: &[Warning], constraints: &Constraints, plan_path: &str) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str("\n\n");

    if constraints.entries.is_empty() {
        out.push_str(NO_CONSTRAINTS_NOTE);
        out.push_str("\n\n");
    }

    if findings.is_empty() {
        out.push_str(DISCLAIMER_NO_FINDINGS);
        out.push('\n');
        return out;
    }

    out.push_str(DISCLAIMER_FINDINGS);
    out.push_str("\n\n");
    for w in findings {
        out.push_str(&render_warning(w, plan_path));
        out.push('\n');
    }
    let count = findings.len();
    let plural = if count == 1 { "warning" } else { "warnings" };
    out.push_str(&format!(
        "{count} possible {plural}. validate-plan does not certify plans; \
         review against source files and tests.\n"
    ));
    out
}

fn render_warning(w: &Warning, plan_path: &str) -> String {
    let mut out = format!("[warn] [{}] {}\n", w.id, w.message);
    let plan_loc = match w.plan_lineno {
        Some(n) => format!("{plan_path}:{n}"),
        None => plan_path.to_string(),
    };
    out.push_str(&format!("  plan: {plan_loc}\n"));
    if let Some(c) = &w.constraint {
        match c.lineno {
            Some(lineno) => out.push_str(&format!(
                "  constraint: {} (.repolens/known-constraints.md:{lineno})\n",
                c.id
            )),
            None => out.push_str(&format!("  constraint: {}\n", c.id)),
        }
    }
    if let Some(detail) = &w.detail {
        out.push_str(&format!("  detail: {detail}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camel_constraints() -> Constraints {
        Constraints {
            entries: vec![Constraint {
                id: "C-001".to_string(),
                statement: "All JSON output uses camelCase (Java contract parity).".to_string(),
                heading_lineno: 7,
            }],
        }
    }

    #[test]
    fn parse_plan_tracks_fences() {
        let plan = parse_plan("before fence\n```rust\nfn foo() {}\n```\nafter fence\n");
        assert!(!plan[0].in_fence);
        assert!(plan[1].in_fence);
        assert!(plan[2].in_fence);
        assert!(plan[3].in_fence);
        assert!(!plan[4].in_fence);
    }

    #[test]
    fn parse_plan_handles_long_fences_with_inner_short_fence() {
        let plan = parse_plan("````\n```\nstill inside\n```\n````\nout\n");
        for line in &plan[..5] {
            assert!(line.in_fence);
        }
        assert!(!plan[5].in_fence);
    }

    #[test]
    fn parse_plan_caches_lower() {
        let plan = parse_plan("Hello WORLD\n");
        assert_eq!(plan[0].lower, "hello world");
    }

    #[test]
    fn parse_constraints_empty_when_blank() {
        assert!(parse_constraints("").entries.is_empty());
        assert!(parse_constraints("   \n\t\n").entries.is_empty());
    }

    #[test]
    fn parse_constraints_skips_pristine_template() {
        let body = "<!-- header -->\n# Known\n\n### C-001 _<short-name>_\n\n\
                    - **Statement:** _..._\n- **Created:** YYYY-MM-DD\n";
        assert!(parse_constraints(body).entries.is_empty());
    }

    #[test]
    fn parse_constraints_skips_retired_placeholder_heading() {
        // Regression: `### C-000 _<short-name>_ (retired YYYY-MM-DD)` has
        // trailing text after the placeholder; the brief's regex still
        // matches the `_<...>_` substring, so it must be skipped.
        let body = "### C-000 _<short-name>_ (retired YYYY-MM-DD)\n";
        assert!(parse_constraints(body).entries.is_empty());
    }

    #[test]
    fn parse_constraints_extracts_real_entry() {
        let body = "# Known\n\n### C-001 JSON camelCase\n\n\
                    - **Statement:** All JSON output uses camelCase.\n";
        let c = parse_constraints(body);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.entries[0].id, "C-001");
        assert!(c.entries[0].statement.contains("camelCase"));
    }

    #[test]
    fn parse_constraints_keeps_real_entry_when_mixed_with_placeholder() {
        let body = "# Known\n\n\
                    ### C-001 JSON camelCase\n- **Statement:** All JSON output uses camelCase.\n\n\
                    ### C-002 _<short-name>_\n- **Statement:** _..._\n";
        let c = parse_constraints(body);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.entries[0].id, "C-001");
    }

    #[test]
    fn snake_case_re_matches_total_count() {
        let m: Vec<&str> = SNAKE_CASE_RE
            .find_iter("Add total_count to output")
            .map(|m| m.as_str())
            .collect();
        assert_eq!(m, vec!["total_count"]);
    }

    #[test]
    fn snake_case_re_skips_camel_and_uppercase() {
        for s in ["FOO_BAR", "totalCount", "noUnderscoresHere"] {
            for m in SNAKE_CASE_RE.find_iter(s) {
                let t = m.as_str();
                assert!(
                    !t.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                    "uppercase token leaked: {t}"
                );
                assert!(t.contains('_'), "no-underscore token matched: {t} (in {s})");
            }
        }
    }

    #[test]
    fn check_vp001_warns_on_snake_near_output_with_camel_constraint() {
        let plan =
            parse_plan("# Plan\nAdd a new field total_count to the compare-summaries output.\n");
        let warnings = check_vp001(&plan, &camel_constraints());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].id, "VP001");
        assert_eq!(warnings[0].plan_lineno, Some(2));
        assert_eq!(
            warnings[0].constraint.as_ref().map(|c| c.id.as_str()),
            Some("C-001")
        );
        assert!(warnings[0].message.contains("total_count"));
    }

    #[test]
    fn check_vp001_silent_on_camel_case() {
        let plan =
            parse_plan("# Plan\nAdd a new field totalCount to the compare-summaries output.\n");
        assert!(check_vp001(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp001_silent_inside_code_fence() {
        let plan =
            parse_plan("# Plan\n```rust\npub struct X { pub total_count: u32 }\n```\nDone.\n");
        assert!(check_vp001(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp001_silent_when_no_camel_constraint() {
        let plan =
            parse_plan("# Plan\nAdd a new field total_count to the compare-summaries output.\n");
        assert!(check_vp001(&plan, &Constraints::default()).is_empty());
    }

    #[test]
    fn check_vp001_silent_when_token_far_from_output_keyword() {
        let padding = "x ".repeat(60);
        let plan = parse_plan(&format!(
            "# Plan\n{padding}total_count is unrelated here.\nLater paragraph mentions output."
        ));
        assert!(check_vp001(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp002_warns_when_plan_mentions_output_without_citing_constraint() {
        let plan =
            parse_plan("# Plan\nAdd a new field totalCount to the compare-summaries output.\n");
        let warnings = check_vp002(&plan, &camel_constraints());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].id, "VP002");
        assert!(warnings[0].message.contains("C-001"));
    }

    #[test]
    fn check_vp002_silent_when_plan_cites_constraint_id() {
        let plan = parse_plan(
            "# Plan\nPer C-001, all output uses camelCase. \
             Add a new field totalCount to compare-summaries output.\n",
        );
        assert!(check_vp002(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp002_silent_when_plan_does_not_touch_output() {
        let plan = parse_plan("# Plan\nRefactor the internal helper for clarity.\n");
        assert!(check_vp002(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp002_silent_when_no_applicable_constraint() {
        let plan =
            parse_plan("# Plan\nAdd a new field totalCount to the compare-summaries output.\n");
        assert!(check_vp002(&plan, &Constraints::default()).is_empty());
    }

    #[test]
    fn check_vp003_warns_when_plan_lacks_grounding_and_verification() {
        let plan = parse_plan("# Plan\nAdd a totalCount field to the output.\n");
        let warnings = check_vp003(&plan);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].id, "VP003");
    }

    #[test]
    fn check_vp003_silent_when_plan_names_a_file() {
        let plan =
            parse_plan("# Plan\nEdit `crates/repolens-app/src/summary.rs` to add the field.\n");
        assert!(check_vp003(&plan).is_empty());
    }

    #[test]
    fn check_vp003_silent_when_plan_states_verification_step() {
        let plan = parse_plan(
            "# Plan\nFirst locate the compare-summaries output type before editing. \
             Then add the field.\n",
        );
        assert!(check_vp003(&plan).is_empty());
    }

    #[test]
    fn check_vp003_silent_when_plan_mentions_a_type() {
        let plan = parse_plan("# Plan\nExtend the `ComparisonResult` struct with the new field.\n");
        assert!(check_vp003(&plan).is_empty());
    }

    #[test]
    fn check_vp004_warns_when_output_change_lacks_test_or_doc_mention() {
        // Plan satisfies VP001/VP002/VP003 (camelCase, cites C-001, names a
        // file, states verification) but never mentions tests/snapshots/docs.
        let plan = parse_plan(
            "# Plan\nFirst locate the compare-summaries output type in \
             `crates/repolens-app/src/summary.rs` before editing. \
             Per C-001, add the totalCount field.\n",
        );
        let warnings = check_vp004(&plan, &camel_constraints());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].id, "VP004");
        assert_eq!(warnings[0].plan_lineno, None);
    }

    #[test]
    fn check_vp004_warns_when_output_change_only_contains_test_as_substring() {
        let plan = parse_plan(
            "# Plan\nFirst locate the latest JSON output field in \
             `crates/repolens-app/src/summary.rs` before editing. \
             Per C-001, add the totalCount field.\n",
        );
        let warnings = check_vp004(&plan, &camel_constraints());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].id, "VP004");
    }

    #[test]
    fn check_vp004_silent_when_plan_mentions_cargo_test() {
        let plan =
            parse_plan("# Plan\nAdd the totalCount field to the output. Then run `cargo test`.\n");
        assert!(check_vp004(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp004_silent_when_plan_mentions_snapshot_or_insta() {
        for body in [
            "# Plan\nAdd totalCount to output and update the insta snapshot.\n",
            "# Plan\nAdd totalCount to output and refresh snapshot fixtures.\n",
        ] {
            let plan = parse_plan(body);
            assert!(
                check_vp004(&plan, &camel_constraints()).is_empty(),
                "VP004 should be silent on body: {body}"
            );
        }
    }

    #[test]
    fn check_vp004_silent_when_plan_mentions_docs() {
        let plan = parse_plan("# Plan\nAdd totalCount to output and update the contract docs.\n");
        assert!(check_vp004(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp004_silent_when_plan_does_not_touch_output() {
        let plan = parse_plan("# Plan\nRefactor the internal helper for clarity.\n");
        assert!(check_vp004(&plan, &camel_constraints()).is_empty());
    }

    #[test]
    fn check_vp004_silent_when_no_camel_constraint() {
        let plan =
            parse_plan("# Plan\nAdd a new field totalCount to the compare-summaries output.\n");
        assert!(check_vp004(&plan, &Constraints::default()).is_empty());
    }

    #[test]
    fn run_checks_returns_warnings_in_id_then_lineno_order() {
        // This plan triggers VP001 (snake_case near output), VP002 (no
        // constraint citation), VP003 (no file/verification), and VP004
        // (no test/doc mention).
        let plan = parse_plan("# Plan\nAdd a new field total_count to the output.\n");
        let warnings = run_checks(&plan, &camel_constraints());
        let ids: Vec<&str> = warnings.iter().map(|w| w.id).collect();
        assert_eq!(ids, vec!["VP001", "VP002", "VP003", "VP004"]);
    }

    #[test]
    fn render_text_always_includes_header_and_disclaimer_when_clean() {
        let out = render_text(&[], &Constraints::default(), "plan.md");
        assert!(out.contains("warnings only"));
        assert!(out.contains("does not prove the plan is valid"));
    }

    #[test]
    fn render_text_emits_findings_disclaimer_when_warnings_present() {
        let plan = parse_plan("# Plan\nAdd field total_count to output.\n");
        let warnings = run_checks(&plan, &camel_constraints());
        let out = render_text(&warnings, &camel_constraints(), "plan.md");
        assert!(out.contains("RepoLens found possible plan risks"));
        assert!(out.contains("[VP001]"));
        assert!(out.contains("do not prove the plan is invalid or valid"));
    }

    #[test]
    fn render_text_no_findings_disclaimer_when_clean_with_constraints() {
        let plan = parse_plan(
            "# Plan\nFirst locate the compare-summaries output type in \
             `crates/repolens-app/src/summary.rs` before editing. \
             Per C-001, use totalCount. Run `cargo test`.\n",
        );
        let warnings = run_checks(&plan, &camel_constraints());
        assert!(
            warnings.is_empty(),
            "this plan should have no findings; got {warnings:#?}"
        );
        let out = render_text(&warnings, &camel_constraints(), "plan.md");
        assert!(out.contains("found no possible plan risks"));
    }

    #[test]
    fn render_text_emits_no_constraints_note_when_constraints_missing() {
        let plan = parse_plan("# Plan\nAdd a totalCount field somewhere.\n");
        let warnings = run_checks(&plan, &Constraints::default());
        let out = render_text(&warnings, &Constraints::default(), "plan.md");
        assert!(out.contains("no real constraints loaded"));
        assert!(!out.contains("[VP001]"));
        assert!(!out.contains("[VP002]"));
        assert!(!out.contains("[VP004]"));
    }
}
