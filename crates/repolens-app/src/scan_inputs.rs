use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

pub const DEFAULT_MAX_FILE_BYTES: i64 = 524_288;
pub const DEFAULT_MAX_TOTAL_BYTES: i64 = 52_428_800;

#[derive(Debug, Clone)]
pub struct ScanInputs {
    pub follow_symlinks: bool,
    pub include_binaries: bool,
    pub max_file_bytes: i64,
    pub max_total_bytes: i64,
    pub reposnapshot_files: Vec<PathBuf>,
    pub reposnapshot_contents: Vec<Vec<u8>>,
    pub config_hash: String,
}

impl ScanInputs {
    pub fn resolve(repo_root: &Path) -> Result<Self> {
        let follow_symlinks = false;
        let include_binaries = false;
        let max_file_bytes = DEFAULT_MAX_FILE_BYTES;
        let max_total_bytes = DEFAULT_MAX_TOTAL_BYTES;

        let rules_path = repo_root.join(".reposnapshot.rules");
        let (reposnapshot_files, reposnapshot_contents) = if rules_path.exists() {
            let content = fs::read(&rules_path)?;
            debug!(path = %rules_path.display(), bytes = content.len(), "Loaded .reposnapshot.rules");
            (vec![rules_path], vec![content])
        } else {
            (vec![], vec![])
        };

        let config_hash = repolens_core::scan_config_hash::ScanConfigHash::compute(
            &reposnapshot_contents,
            follow_symlinks,
            include_binaries,
            max_file_bytes,
            max_total_bytes,
        );

        info!(hash = %config_hash, rules = reposnapshot_files.len(), "Scan inputs resolved");

        Ok(Self {
            follow_symlinks,
            include_binaries,
            max_file_bytes,
            max_total_bytes,
            reposnapshot_files,
            reposnapshot_contents,
            config_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_defaults_without_rules_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let inputs = ScanInputs::resolve(root).unwrap();

        assert!(!inputs.follow_symlinks);
        assert!(!inputs.include_binaries);
        assert_eq!(inputs.max_file_bytes, DEFAULT_MAX_FILE_BYTES);
        assert_eq!(inputs.max_total_bytes, DEFAULT_MAX_TOTAL_BYTES);
        assert!(inputs.reposnapshot_files.is_empty());
        assert!(inputs.reposnapshot_contents.is_empty());
        assert_eq!(inputs.config_hash.len(), 64);
    }

    #[test]
    fn resolve_includes_reposnapshot_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let rules_content = b"*.log\n*.tmp\n";
        fs::write(root.join(".reposnapshot.rules"), rules_content).unwrap();

        let inputs = ScanInputs::resolve(root).unwrap();

        assert_eq!(inputs.reposnapshot_files.len(), 1);
        assert_eq!(inputs.reposnapshot_contents.len(), 1);
        assert_eq!(inputs.reposnapshot_contents[0], rules_content);
        assert_eq!(inputs.config_hash.len(), 64);
    }

    #[test]
    fn hash_changes_when_rules_change() {
        let dir_no_rules = tempfile::tempdir().unwrap();
        let inputs_no_rules = ScanInputs::resolve(dir_no_rules.path()).unwrap();

        let dir_with_rules = tempfile::tempdir().unwrap();
        fs::write(
            dir_with_rules.path().join(".reposnapshot.rules"),
            b"*.log\n",
        )
        .unwrap();
        let inputs_with_rules = ScanInputs::resolve(dir_with_rules.path()).unwrap();

        assert_ne!(
            inputs_no_rules.config_hash, inputs_with_rules.config_hash,
            "hash should differ with/without rules"
        );
    }

    #[test]
    fn hash_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join(".reposnapshot.rules"), b"*.log\n").unwrap();

        let inputs1 = ScanInputs::resolve(root).unwrap();
        let inputs2 = ScanInputs::resolve(root).unwrap();

        assert_eq!(
            inputs1.config_hash, inputs2.config_hash,
            "same inputs should produce same hash"
        );
    }
}
