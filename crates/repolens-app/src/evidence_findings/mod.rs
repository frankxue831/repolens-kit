//! Evidence-aware finding emitter. Produces `Vec<EvidenceFinding>` from the
//! same inputs that drive `HealthAnalyzer` plus a `RepoContext` and an
//! optional `CargoLockState`. Lives here (not in `repolens-profile`) because
//! several findings need access to the file graph, which is owned by
//! `repolens-app`.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md`.
//!
//! Phase 1 emits six findings:
//! 1. F-NO-ENTRY-POINTS
//! 2. F-ORPHAN-FILES
//! 3. F-DOMINANT-MODULE
//! 4. F-HIGH-FAN-IN-FILE
//! 5. F-CARGO-LOCK-STATE
//! 6. F-TEST-INVENTORY-RATIO

use std::collections::HashMap;

use repolens_core::model::epistemic::{EpistemicConfidence, EvidenceGrade, KnowledgeLayer};
use repolens_core::model::evidence::{
    limitation_codes, metric_names, CargoLockState, EvidenceFinding, EvidenceItem, EvidenceScope,
    EvidenceSource, FindingClassification, FindingId, FindingSeverity, InferenceStrength,
    Limitation, RecommendedAction, RepoContextTag, ResponsibleInference, ScopeTime,
};
use repolens_core::model::graph::FileGraph;
use repolens_core::model::health::GraphHealthInput;
use repolens_core::model::profile::{ProjectProfile, ProjectType};
use repolens_core::model::summary::ScanSummary;
use repolens_core::path_classification::{is_indicator_tagged_module_path, is_non_orphan_source};
use repolens_profile::health::cycle_severity;
use repolens_profile::repo_context::RepoContext;

// ---------------------------------------------------------------------------
// Thresholds. Centralised so the spec can reference exact values.
// ---------------------------------------------------------------------------

const DOMINANT_MODULE_SHARE_THRESHOLD: f64 = 0.8;
const ORPHAN_RATIO_THRESHOLD: f64 = 0.05;
const HIGH_FAN_IN_MULTIPLIER: f64 = 2.0;
// Statistical-reliability and absolute-magnitude guards on F-HIGH-FAN-IN-FILE.
// Mean-multiplier thresholds collapse on tiny graphs (a 4-file repo with one
// reused helper gets flagged as "above 2× average") and on graphs where the
// mean inbound degree is well below 1 (every leaf-importer fires). Require at
// least 10 nodes for the mean to be meaningful, and at least 3 absolute
// inbound edges for the per-file finding to fire — anything below that is
// indistinguishable from normal helper-module reuse.
const HIGH_FAN_IN_MIN_GRAPH_NODES: usize = 10;
const HIGH_FAN_IN_MIN_INBOUND_EDGES: i32 = 3;
// Cap the number of F-HIGH-FAN-IN-FILE findings — anything below the top N
// by inbound degree is diminishing-returns noise that floods the agent's
// context. The 2026-05-11 dogfood pass against repolens-rs (a 386-file
// workspace) produced 48 fan-in findings without this cap.
const HIGH_FAN_IN_MAX_FINDINGS: usize = 10;
const TEST_RATIO_LOW_THRESHOLD: f64 = 0.1;
const TEST_RATIO_MIN_SOURCE_FILES: i32 = 5;

// ---------------------------------------------------------------------------
// Finding IDs (greppable strings).
// ---------------------------------------------------------------------------

/// Title of the F-ORPHAN-FILES finding when source files look unused. The
/// check reads imports only (plus HTML `<script src>` and CI commands), so
/// the title says "may" and names that limit.
pub const ORPHAN_TITLE: &str = "Some files may be unused (imports only)";
pub const F_NO_ENTRY_POINTS: &str = "F-NO-ENTRY-POINTS";
pub const F_ORPHAN_FILES: &str = "F-ORPHAN-FILES";
pub const F_DOMINANT_MODULE: &str = "F-DOMINANT-MODULE";
pub const F_HIGH_FAN_IN_FILE: &str = "F-HIGH-FAN-IN-FILE";
pub const F_CARGO_LOCK_STATE: &str = "F-CARGO-LOCK-STATE";
pub const F_TEST_INVENTORY_RATIO: &str = "F-TEST-INVENTORY-RATIO";
pub const F_DEPENDENCY_CYCLES: &str = "F-DEPENDENCY-CYCLES";
pub const F_LOCKFILE_MISSING_NON_CARGO: &str = "F-LOCKFILE-MISSING-NON-CARGO";
pub const F_LANGUAGE_EXTRACTION_GAP: &str = "F-LANGUAGE-EXTRACTION-GAP";

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct EvidenceFindingsBuilder<'a> {
    pub pack_id: &'a str,
    pub commit: Option<&'a str>,
    pub profile: &'a ProjectProfile,
    pub repo_context: &'a RepoContext,
    pub graph: Option<&'a FileGraph>,
    pub graph_health: Option<&'a GraphHealthInput>,
    pub scan: Option<&'a ScanSummary>,
    pub lockfile_state: Option<&'a CargoLockState>,
    /// Files referenced outside the import graph (HTML `<script src>`, CI
    /// commands); see [`crate::references`]. Treated as used by orphan checks.
    /// Whether the root Cargo project (root package + root workspace
    /// members) has a binary target, read from the repo root; see
    /// `repolens_profile::repo_context::cargo_root_has_binary_target`. When
    /// known, it decides F-CARGO-LOCK-STATE instead of the repo-wide
    /// `RustBinary` tag, which also counts scripts and nested workspaces.
    pub cargo_has_binary: Option<bool>,
    pub referenced_files: Option<&'a std::collections::HashSet<String>>,
    /// Computed `DependencyRisk` (build system, lockfile presence). Consumed
    /// by `F-LOCKFILE-MISSING-NON-CARGO` to detect non-Cargo build systems
    /// that lack a lockfile. Cargo-specific cases are covered by
    /// `F-CARGO-LOCK-STATE` via `lockfile_state` and a Rust-context check.
    pub dependency_risk: Option<&'a repolens_core::model::health::DependencyRisk>,
}

impl<'a> EvidenceFindingsBuilder<'a> {
    pub fn build(&self) -> Vec<EvidenceFinding> {
        let mut out: Vec<EvidenceFinding> = Vec::new();
        if let Some(f) = self.no_entry_points() {
            out.push(f);
        }
        if let Some(f) = self.orphan_files() {
            out.push(f);
        }
        if let Some(f) = self.dominant_module() {
            out.push(f);
        }
        if let Some(f) = self.high_fan_in_files() {
            out.push(f);
        }
        if let Some(f) = self.cargo_lock() {
            out.push(f);
        }
        if let Some(f) = self.test_inventory() {
            out.push(f);
        }
        if let Some(f) = self.dependency_cycles() {
            out.push(f);
        }
        if let Some(f) = self.lockfile_missing_non_cargo() {
            out.push(f);
        }
        if let Some(f) = self.language_extraction_gap() {
            out.push(f);
        }
        out
    }

    fn scope(&self, paths: Vec<String>, languages: Vec<String>) -> EvidenceScope {
        EvidenceScope {
            paths,
            time: ScopeTime {
                pack_id: self.pack_id.to_string(),
                commit: self.commit.map(str::to_string),
            },
            languages,
        }
    }

    fn indicator_notes(&self) -> Vec<String> {
        self.repo_context
            .notes
            .iter()
            .map(|m| m.note.clone())
            .collect()
    }
}

fn languages_from_profile(profile: &ProjectProfile) -> Vec<String> {
    profile.language_distribution.keys().cloned().collect()
}

fn compute_degree_map(graph: &FileGraph) -> HashMap<String, (i32, i32)> {
    let mut map: HashMap<String, (i32, i32)> = HashMap::new();
    for node in &graph.nodes {
        map.entry(node.id.clone()).or_insert((0, 0));
    }
    for edge in &graph.edges {
        map.entry(edge.target.clone()).or_insert((0, 0)).0 += 1;
        map.entry(edge.source.clone()).or_insert((0, 0)).1 += 1;
    }
    map
}

mod cargo_lock;
mod dependency_cycles;
mod dominant_module;
mod high_fan_in;
mod language_extraction_gap;
mod lockfile_missing;
mod no_entry_points;
mod orphan_files;
mod test_inventory;

#[cfg(test)]
mod tests;
