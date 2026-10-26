use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Project type classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectType {
    Cli,
    WebApp,
    Library,
    Unknown,
}

/// Build system metadata extracted from the repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSystem {
    pub name: String,
    /// Free-form additional detail; may be null in fixtures.
    pub details: Option<String>,
    pub config_file: String,
}

/// A detected framework / library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Framework {
    pub name: String,
    pub version: String,
    pub category: String,
}

/// A detected entry-point in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoint {
    /// Renamed from `type` (reserved keyword) to `entry_type`.
    #[serde(rename = "type")]
    pub entry_type: String,
    pub path: String,
    pub description: String,
}

/// Top-level directory module summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInfo {
    pub name: String,
    pub path: String,
    pub file_count: i32,
    pub loc: i32,
    pub primary_language: String,
}

/// Full project profile as written by `ProfileAnalyzer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProfile {
    pub name: String,
    /// `null` when no recognisable build system was detected (e.g. excluded-small).
    pub build_system: Option<BuildSystem>,
    pub frameworks: Vec<Framework>,
    pub project_type: ProjectType,
    pub entry_points: Vec<EntryPoint>,
    pub description: Option<String>,
    pub language_distribution: BTreeMap<String, i32>,
    pub modules: Vec<ModuleInfo>,
}
