use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Root bootstrap contract loaded from `bootstrap.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    pub version: String,
    pub repo: RepoContract,
    pub commands: CommandContract,
    pub task_types: Vec<String>,
    pub allow_paths: Vec<String>,
    pub deny_paths: Vec<String>,
    pub required_checks: BTreeMap<String, Vec<String>>,
    pub memory_policy: MemoryPolicy,
    pub trust_policy: TrustPolicy,
    pub skills: Vec<BootstrapSkill>,
}

/// Repository identity and root path for bootstrap work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoContract {
    pub name: String,
    pub root: String,
}

/// Command contract entries may be authored as shell strings or argv vectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BootstrapCommandSpec {
    LegacyString(String),
    Argv(Vec<String>),
}

impl BootstrapCommandSpec {
    pub fn normalize(&self) -> Result<Vec<String>> {
        match self {
            Self::LegacyString(command) => {
                let argv = shell_words::split(command)
                    .with_context(|| format!("invalid bootstrap command: {command}"))?;
                validate_argv(argv, "bootstrap command")
            }
            Self::Argv(argv) => validate_argv(argv.clone(), "bootstrap command"),
        }
    }
}

fn validate_argv(argv: Vec<String>, context: &str) -> Result<Vec<String>> {
    if argv.is_empty() {
        bail!("{context} cannot be empty");
    }
    if argv.iter().any(|part| part.is_empty()) {
        bail!("{context} cannot contain empty elements");
    }
    Ok(argv)
}

/// Conventional commands the bootstrap layer may invoke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandContract {
    pub build: BootstrapCommandSpec,
    pub test: BootstrapCommandSpec,
    pub fmt: BootstrapCommandSpec,
    pub clippy: BootstrapCommandSpec,
    pub scan: BootstrapCommandSpec,
    pub mcp: BootstrapCommandSpec,
}

/// Frozen argv-oriented command contract used by runtime consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenBootstrapCommands {
    pub build: Vec<String>,
    pub test: Vec<String>,
    pub fmt: Vec<String>,
    pub clippy: Vec<String>,
    pub scan: Vec<String>,
    pub mcp: Vec<String>,
}

/// Frozen bootstrap contract with normalized argv arrays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenBootstrapContract {
    pub version: String,
    pub commands: FrozenBootstrapCommands,
    pub required_checks: BTreeMap<String, Vec<Vec<String>>>,
    pub task_types: Vec<String>,
    pub allow_paths: Vec<String>,
    pub deny_paths: Vec<String>,
    pub skills: Vec<BootstrapSkill>,
}

/// Policy for when bootstrap workflows may persist memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryPolicy {
    pub require_anchors: bool,
    pub reject_unanchored_writes: bool,
}

/// Trust rules for how bootstrap workflows consume evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustPolicy {
    pub repo_understanding: String,
    pub cold_start_exception: String,
    pub structural_edit_minimum_grade: String,
    pub planning_minimum_grade: String,
    pub unaudited_usage: String,
}

/// Machine-readable summary of one bootstrap skill contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSkill {
    pub id: String,
    pub category: BootstrapSkillCategory,
    pub purpose: String,
    pub requires_repo_lens: bool,
    pub allows_cold_start: bool,
    pub allowed_evidence_grades: Vec<BootstrapEvidenceGrade>,
    pub writes_memory: bool,
    pub requires_anchors: bool,
}

/// Stable categories for the initial bootstrap skill set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapSkillCategory {
    Discovery,
    Planning,
    Execution,
    Verification,
    Memory,
}

/// Allowed evidence grades bootstrap skills may consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapEvidenceGrade {
    Verified,
    Derived,
    Unaudited,
}
