use serde::{Deserialize, Serialize};

pub const LOCKFILE_NAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "go.sum",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "pdm.lock",
];

/// The standard lockfiles of one build system, as named by the profile
/// (`npm`, `Go`, `Python`, `Cargo`, ...). `details` is the profile's build
/// system detail; for Python it names the lock tool (`poetry`, `uv`, `pdm`,
/// `pipenv`). `None` means the build system has no standard lockfile
/// (Maven, Gradle without opt-in locking, plain pip/setuptools) or is
/// unknown, so a missing lockfile is not a finding (#25).
pub fn lockfiles_for_build_system(
    build_system: &str,
    details: Option<&str>,
) -> Option<&'static [&'static str]> {
    match build_system.to_ascii_lowercase().as_str() {
        "npm" | "yarn" | "pnpm" | "node" => {
            Some(&["package-lock.json", "yarn.lock", "pnpm-lock.yaml"])
        }
        "go" | "gomod" => Some(&["go.sum"]),
        "poetry" => Some(&["poetry.lock"]),
        "python" | "pip" | "setuptools" | "pyproject" => {
            match details.map(str::to_ascii_lowercase).as_deref() {
                Some("poetry") => Some(&["poetry.lock"]),
                Some("uv") => Some(&["uv.lock"]),
                Some("pdm") => Some(&["pdm.lock"]),
                Some("pipenv") => Some(&["Pipfile.lock"]),
                _ => None,
            }
        }
        "cargo" => Some(&["Cargo.lock"]),
        _ => None,
    }
}

/// Module structure metrics extracted from the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Structure {
    pub module_count: i32,
    pub dominant_module: String,
    pub dominant_module_share: f64,
    pub entry_point_count: i32,
}

/// Dependency risk assessment derived from build files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyRisk {
    pub build_system: Option<String>,
    pub framework_count: i32,
    pub missing_version_count: i32,
    pub lockfile_present: bool,
    pub risk_level: String,
}

/// Full health report as written by `HealthAnalyzer`.
///
/// The former `signals` field was retired in the 2026-05-12 Phase 4B Step 2
/// amendment along with the `Signal` struct itself. Each signal had a
/// catalogued evidence-finding equivalent: `dependency-cycles` →
/// F-DEPENDENCY-CYCLES, `orphan-files` → F-ORPHAN-FILES, `low-test-ratio` →
/// F-TEST-INVENTORY-RATIO. See
/// `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` §"Phase 4
/// amendments (Part B Step 2 — retire signals + add F-DEPENDENCY-CYCLES)".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub structure: Structure,
    pub dependency_risk: DependencyRisk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_health: Option<GraphHealthInput>,
    /// Evidence-aware findings, carrying the four-part calibration block
    /// (observation, responsible inference, limitations, action) per
    /// finding. Always serialized — see [`crate::model::summary`]
    /// `RepoSummary.evidence_findings` for the rationale.
    #[serde(default)]
    pub evidence_findings: Vec<crate::model::evidence::EvidenceFinding>,
}

/// Pre-computed graph analysis results passed to the health analyzer.
///
/// Lives in `repolens-core` so that `repolens-profile` can consume it
/// without depending on `repolens-app` (which owns `GraphAnalyzer`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphHealthInput {
    pub cycle_count: usize,
    pub largest_cycle_size: usize,
    pub files_in_cycles_count: usize,
    pub orphan_count: usize,
    pub orphan_files: Vec<String>,
    pub test_to_source_ratio: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_health_report() -> HealthReport {
        HealthReport {
            structure: Structure {
                module_count: 3,
                dominant_module: "src".to_string(),
                dominant_module_share: 0.72,
                entry_point_count: 2,
            },
            dependency_risk: DependencyRisk {
                build_system: Some("cargo".to_string()),
                framework_count: 5,
                missing_version_count: 1,
                lockfile_present: true,
                risk_level: "LOW".to_string(),
            },
            graph_health: None,
            evidence_findings: vec![],
        }
    }

    #[test]
    fn health_report_round_trip() {
        let original = sample_health_report();
        let json = serde_json::to_string(&original).expect("serialization failed");
        let restored: HealthReport = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(original, restored);

        // Verify camelCase keys
        assert!(
            json.contains("\"dependencyRisk\""),
            "expected dependencyRisk key"
        );
        assert!(json.contains("\"moduleCount\""), "expected moduleCount key");
        assert!(
            json.contains("\"dominantModuleShare\""),
            "expected dominantModuleShare key"
        );
    }

    #[test]
    fn dependency_risk_null_build_system() {
        let risk = DependencyRisk {
            build_system: None,
            framework_count: 2,
            missing_version_count: 0,
            lockfile_present: false,
            risk_level: "MEDIUM".to_string(),
        };
        let json = serde_json::to_string(&risk).expect("serialization failed");
        assert!(
            json.contains("\"buildSystem\":null"),
            "expected buildSystem to be null, got: {}",
            json
        );
    }

    #[test]
    fn graph_health_input_round_trip() {
        let input = GraphHealthInput {
            cycle_count: 2,
            largest_cycle_size: 5,
            files_in_cycles_count: 7,
            orphan_count: 3,
            orphan_files: vec!["src/unused.rs".to_string(), "src/old.rs".to_string()],
            test_to_source_ratio: 0.15,
        };
        let json = serde_json::to_string(&input).expect("serialization failed");
        let restored: GraphHealthInput =
            serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(input, restored);
        assert!(
            json.contains("\"cycleCount\""),
            "expected camelCase key cycleCount"
        );
        assert!(
            json.contains("\"largestCycleSize\""),
            "expected camelCase key largestCycleSize"
        );
        assert!(
            json.contains("\"filesInCyclesCount\""),
            "expected camelCase key filesInCyclesCount"
        );
    }

    #[test]
    fn health_report_with_graph_health() {
        let report = HealthReport {
            structure: Structure {
                module_count: 2,
                dominant_module: "src".to_string(),
                dominant_module_share: 0.6,
                entry_point_count: 1,
            },
            dependency_risk: DependencyRisk {
                build_system: Some("cargo".to_string()),
                framework_count: 1,
                missing_version_count: 0,
                lockfile_present: true,
                risk_level: "low".to_string(),
            },
            graph_health: Some(GraphHealthInput {
                cycle_count: 1,
                largest_cycle_size: 3,
                files_in_cycles_count: 3,
                orphan_count: 0,
                orphan_files: vec![],
                test_to_source_ratio: 0.2,
            }),
            evidence_findings: vec![],
        };
        let json = serde_json::to_string(&report).expect("serialization failed");
        assert!(json.contains("\"graphHealth\""), "expected graphHealth key");
        assert!(
            json.contains("\"filesInCyclesCount\""),
            "expected filesInCyclesCount inside graphHealth"
        );
        let restored: HealthReport = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(report, restored);
    }

    #[test]
    fn health_report_without_graph_health() {
        let report = sample_health_report();
        let json = serde_json::to_string(&report).expect("serialization failed");
        assert!(
            !json.contains("\"graphHealth\""),
            "graphHealth should be absent when None"
        );
    }
}
