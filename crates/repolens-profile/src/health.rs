use repolens_core::model::health::{
    DependencyRisk, GraphHealthInput, HealthReport, Structure, LOCKFILE_NAMES,
};
use repolens_core::model::profile::ProjectProfile;
use repolens_core::pack_reader::PackIndex;
use tracing::info;

/// Classify cycle severity based on count and largest cycle size.
///
/// Rules (first match wins):
/// - cycle_count >= 6 OR largest_cycle_size > 6 → high
/// - cycle_count >= 3 OR largest_cycle_size > 3 → medium
/// - otherwise (cycle_count > 0)                → low
pub fn cycle_severity(cycle_count: usize, largest_cycle_size: usize) -> &'static str {
    if cycle_count >= 6 || largest_cycle_size > 6 {
        "high"
    } else if cycle_count >= 3 || largest_cycle_size > 3 {
        "medium"
    } else {
        "low"
    }
}

/// Analyzes a `PackIndex` and `ProjectProfile` to produce a `HealthReport`.
/// Graph-derived observations flow through `EvidenceFindingsBuilder` since
/// Phase 4B Step 2 retired the legacy `signals` field.
pub struct HealthAnalyzer;

impl HealthAnalyzer {
    /// Analyze the provided pack index, profile, and (optional) graph-health
    /// input and return a `HealthReport`.
    ///
    /// Phase 4B Step 2 (2026-05-12): the legacy `signals` field was retired
    /// in favour of catalogued evidence findings. The former `FileGraph`
    /// parameter is no longer required — graph-derived observations now flow
    /// through `EvidenceFindingsBuilder` (F-ORPHAN-FILES, F-DEPENDENCY-CYCLES,
    /// F-TEST-INVENTORY-RATIO).
    pub fn analyze(
        index: &PackIndex,
        profile: &ProjectProfile,
        graph_health: Option<&GraphHealthInput>,
    ) -> HealthReport {
        info!(
            entry_count = index.entries.len(),
            module_count = profile.modules.len(),
            framework_count = profile.frameworks.len(),
            "HealthAnalyzer::analyze starting"
        );

        let structure = Self::compute_structure(profile);
        let dependency_risk = Self::compute_dependency_risk(index, profile);

        info!(
            risk_level = %dependency_risk.risk_level,
            "HealthAnalyzer::analyze complete"
        );

        HealthReport {
            structure,
            dependency_risk,
            graph_health: graph_health.cloned(),
            evidence_findings: vec![],
        }
    }

    fn compute_structure(profile: &ProjectProfile) -> Structure {
        let module_count = profile.modules.len() as i32;
        let total_loc: i32 = profile.modules.iter().map(|m| m.loc).sum();

        let dominant = profile.modules.iter().max_by_key(|m| m.loc);

        let (dominant_module, dominant_module_share) = match dominant {
            Some(m) => {
                let share = if total_loc > 0 {
                    m.loc as f64 / total_loc as f64
                } else {
                    0.0
                };
                (m.name.clone(), share)
            }
            None => ("(none)".to_string(), 0.0),
        };

        let entry_point_count = profile.entry_points.len() as i32;

        Structure {
            module_count,
            dominant_module,
            dominant_module_share,
            entry_point_count,
        }
    }

    fn compute_dependency_risk(index: &PackIndex, profile: &ProjectProfile) -> DependencyRisk {
        let build_system = profile.build_system.as_ref().map(|b| b.name.clone());
        let framework_count = profile.frameworks.len() as i32;
        let missing_version_count = 0; // stubbed

        let lockfile_present = index
            .entries
            .iter()
            .any(|e| LOCKFILE_NAMES.contains(&e.path.as_str()));

        // Only a build system with a standard lockfile can miss one (#25).
        let expects_lockfile = profile.build_system.as_ref().is_some_and(|b| {
            repolens_core::model::health::lockfiles_for_build_system(&b.name, b.details.as_deref())
                .is_some()
        });
        let risk_level =
            Self::compute_risk_level(lockfile_present, expects_lockfile, framework_count);

        DependencyRisk {
            build_system,
            framework_count,
            missing_version_count,
            lockfile_present,
            risk_level,
        }
    }

    fn compute_risk_level(
        lockfile_present: bool,
        has_build_system: bool,
        framework_count: i32,
    ) -> String {
        if !lockfile_present && has_build_system && framework_count > 2 {
            "high".to_string()
        } else if (!lockfile_present && has_build_system) || framework_count > 3 {
            "medium".to_string()
        } else {
            "low".to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::FileEntry;
    use repolens_core::model::health::GraphHealthInput;
    use repolens_core::model::manifest::Manifest;
    use repolens_core::model::profile::{BuildSystem, Framework, ModuleInfo, ProjectType};
    use repolens_core::pack_reader::PackIndex;
    use repolens_core::serde_util::java_epoch_seconds::JavaEpochSeconds;
    use rust_decimal::Decimal;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn make_manifest() -> Manifest {
        Manifest {
            schema_version: "1".to_string(),
            schema_url: "".to_string(),
            timestamp: JavaEpochSeconds(Decimal::from_str("1000000.0").unwrap()),
            chunks: vec![],
            scan_mode: None,
            scan_scope: None,
            base_pack_id: None,
            base_head_commit: None,
            head_commit: None,
            worktree_dirty: None,
            scan_config_hash: None,
            reprocessed_files: None,
            deleted_files: None,
            carried_forward_files: None,
        }
    }

    fn make_index(entries: Vec<FileEntry>) -> PackIndex {
        PackIndex {
            manifest: make_manifest(),
            entries,
        }
    }

    fn make_entry(path: &str) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            sha256: "abc123".to_string(),
            language: "".to_string(),
            loc: 0,
            included: true,
            reason: "included".to_string(),
            content_path: None,
            symbols: vec![],
        }
    }

    fn make_profile(
        modules: Vec<ModuleInfo>,
        frameworks: Vec<Framework>,
        build_system: Option<BuildSystem>,
    ) -> ProjectProfile {
        ProjectProfile {
            name: "test".to_string(),
            project_type: ProjectType::Unknown,
            entry_points: vec![],
            description: None,
            language_distribution: BTreeMap::new(),
            modules,
            frameworks,
            build_system,
        }
    }

    fn make_module(name: &str, loc: i32) -> ModuleInfo {
        ModuleInfo {
            name: name.to_string(),
            path: name.to_string(),
            file_count: 1,
            loc,
            primary_language: "Rust".to_string(),
        }
    }

    fn make_framework(name: &str) -> Framework {
        Framework {
            name: name.to_string(),
            version: "1.0".to_string(),
            category: "backend".to_string(),
        }
    }

    fn make_build_system(name: &str) -> BuildSystem {
        BuildSystem {
            name: name.to_string(),
            details: None,
            config_file: "Cargo.toml".to_string(),
        }
    }

    #[test]
    fn structure_metrics_from_modules() {
        let modules = vec![make_module("src", 800), make_module("test", 200)];
        let profile = make_profile(modules, vec![], None);
        let index = make_index(vec![]);
        let report = HealthAnalyzer::analyze(&index, &profile, None);

        assert_eq!(report.structure.module_count, 2);
        assert_eq!(report.structure.dominant_module, "src");
        let share = report.structure.dominant_module_share;
        assert!((share - 0.8).abs() < 1e-9, "expected ~0.8, got {}", share);
    }

    #[test]
    fn lockfile_detection() {
        let modules = vec![make_module("src", 100)];
        let build_sys = Some(make_build_system("npm"));
        let frameworks = vec![make_framework("React"), make_framework("Express")];

        // Without lockfile
        let profile_no_lock = make_profile(modules.clone(), frameworks.clone(), build_sys.clone());
        let index_no_lock = make_index(vec![make_entry("package.json")]);
        let report_no = HealthAnalyzer::analyze(&index_no_lock, &profile_no_lock, None);
        assert!(!report_no.dependency_risk.lockfile_present);
        assert_eq!(report_no.dependency_risk.risk_level, "medium");

        // With package-lock.json
        let profile_with_lock = make_profile(modules, frameworks, build_sys);
        let index_with_lock = make_index(vec![
            make_entry("package.json"),
            make_entry("package-lock.json"),
        ]);
        let report_yes = HealthAnalyzer::analyze(&index_with_lock, &profile_with_lock, None);
        assert!(report_yes.dependency_risk.lockfile_present);
        assert_eq!(report_yes.dependency_risk.risk_level, "low");
    }

    /// #25: Maven has no standard lockfile, so a missing one adds no risk.
    #[test]
    fn maven_without_lockfile_is_not_a_risk() {
        let modules = vec![make_module("src", 100)];
        let frameworks = vec![make_framework("Spring")];
        let profile = make_profile(modules, frameworks, Some(make_build_system("Maven")));
        let index = make_index(vec![make_entry("pom.xml")]);
        let report = HealthAnalyzer::analyze(&index, &profile, None);
        assert!(!report.dependency_risk.lockfile_present);
        assert_eq!(report.dependency_risk.risk_level, "low");
    }

    #[test]
    fn uv_lock_counts_as_a_lockfile() {
        let profile = make_profile(
            vec![make_module("src", 100)],
            vec![],
            Some(make_build_system("Python")),
        );
        let index = make_index(vec![make_entry("pyproject.toml"), make_entry("uv.lock")]);
        let report = HealthAnalyzer::analyze(&index, &profile, None);
        assert!(report.dependency_risk.lockfile_present);
    }

    #[test]
    fn high_risk_no_lockfile_plus_many_frameworks() {
        let modules = vec![make_module("src", 100)];
        let frameworks = vec![
            make_framework("React"),
            make_framework("Express"),
            make_framework("Django"),
        ];
        let build_sys = Some(make_build_system("npm"));
        let profile = make_profile(modules, frameworks, build_sys);
        let index = make_index(vec![make_entry("package.json")]);
        let report = HealthAnalyzer::analyze(&index, &profile, None);

        assert_eq!(report.dependency_risk.risk_level, "high");
    }

    #[test]
    fn graph_health_carried_through_to_report() {
        let index = PackIndex {
            manifest: make_manifest(),
            entries: vec![],
        };
        let profile = make_profile(vec![make_module("src", 100)], vec![], None);
        let graph_health = GraphHealthInput {
            cycle_count: 2,
            largest_cycle_size: 5,
            files_in_cycles_count: 6,
            orphan_count: 0,
            orphan_files: vec![],
            test_to_source_ratio: 0.2,
        };
        let report = HealthAnalyzer::analyze(&index, &profile, Some(&graph_health));
        // graph_health should be carried through verbatim — evidence-aware
        // findings (F-DEPENDENCY-CYCLES, F-ORPHAN-FILES, F-TEST-INVENTORY-RATIO)
        // consume it via `EvidenceFindingsBuilder`. The legacy `signals`
        // field was retired in Phase 4B Step 2 (2026-05-12).
        assert!(report.graph_health.is_some());
        let gh = report.graph_health.as_ref().unwrap();
        assert_eq!(gh.files_in_cycles_count, 6);
        assert_eq!(gh.largest_cycle_size, 5);
        assert_eq!(gh.cycle_count, 2);
    }

    #[test]
    fn test_cycle_severity_low() {
        assert_eq!(cycle_severity(1, 2), "low");
        assert_eq!(cycle_severity(2, 3), "low");
    }

    #[test]
    fn test_cycle_severity_medium() {
        assert_eq!(cycle_severity(3, 2), "medium");
        assert_eq!(cycle_severity(1, 4), "medium");
        assert_eq!(cycle_severity(5, 3), "medium");
    }

    #[test]
    fn test_cycle_severity_high() {
        assert_eq!(cycle_severity(6, 2), "high");
        assert_eq!(cycle_severity(1, 7), "high");
        assert_eq!(cycle_severity(10, 10), "high");
    }
}
