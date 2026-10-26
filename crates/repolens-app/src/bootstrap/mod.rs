mod command_runner;
mod planner;
mod service;
mod store;

pub use service::BootstrapService;
pub use store::{BootstrapRunStore, TryActiveRunLockResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartBootstrapRunRequest {
    pub root: std::path::PathBuf,
    pub task_type: String,
    pub goal: String,
    pub intended_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishBootstrapRunRequest {
    pub root: std::path::PathBuf,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBootstrapRunResponse {
    pub run_id: String,
    pub run_dir: String,
    pub plan_path: String,
    pub bounded_paths: Vec<String>,
    pub required_checks: Vec<Vec<String>>,
    pub discovery: repolens_core::model::bootstrap_run::BootstrapDiscoverySummary,
}
