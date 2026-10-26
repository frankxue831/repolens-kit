use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::model::bootstrap::{
    BootstrapConfig, BootstrapSkill, FrozenBootstrapCommands, FrozenBootstrapContract,
};

/// Snapshot of the parsed bootstrap contract and the normalized runtime form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapContractSnapshot {
    pub raw: BootstrapConfig,
    pub frozen: FrozenBootstrapContract,
    pub hash_sha256: String,
}

/// Thin lookup wrapper over the validated bootstrap skill definitions.
#[derive(Debug, Clone)]
pub struct BootstrapSkillRegistry {
    skills: Vec<BootstrapSkill>,
    by_id: BTreeMap<String, usize>,
}

impl BootstrapSkillRegistry {
    pub fn from_config(config: &BootstrapConfig) -> Result<Self> {
        validate_bootstrap_config(config)?;

        let by_id = config
            .skills
            .iter()
            .enumerate()
            .map(|(index, skill)| (skill.id.clone(), index))
            .collect();

        Ok(Self {
            skills: config.skills.clone(),
            by_id,
        })
    }

    pub fn get(&self, id: &str) -> Option<&BootstrapSkill> {
        self.by_id.get(id).map(|index| &self.skills[*index])
    }

    pub fn all(&self) -> impl Iterator<Item = &BootstrapSkill> {
        self.skills.iter()
    }
}

/// Loads and validates the root bootstrap contract.
pub fn load_bootstrap_config(path: impl AsRef<Path>) -> Result<BootstrapConfig> {
    let path = path.as_ref();
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let config: BootstrapConfig = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    validate_bootstrap_config(&config)?;
    Ok(config)
}

/// Loads, validates, hashes, and freezes the root bootstrap contract.
pub fn load_bootstrap_contract_snapshot(
    path: impl AsRef<Path>,
) -> Result<BootstrapContractSnapshot> {
    let path = path.as_ref();
    let raw = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let hash_sha256 = hash_bytes_sha256(&raw);
    let config: BootstrapConfig = serde_yaml::from_slice(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    validate_bootstrap_config(&config)?;
    let frozen = freeze_bootstrap_contract(&config)?;

    Ok(BootstrapContractSnapshot {
        raw: config,
        frozen,
        hash_sha256,
    })
}

pub fn hash_file_sha256(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let raw = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(hash_bytes_sha256(&raw))
}

fn hash_bytes_sha256(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

/// Converts the parsed bootstrap config into argv-oriented runtime data.
pub fn freeze_bootstrap_contract(config: &BootstrapConfig) -> Result<FrozenBootstrapContract> {
    validate_bootstrap_config(config)?;

    let commands = FrozenBootstrapCommands {
        build: config.commands.build.normalize()?,
        test: config.commands.test.normalize()?,
        fmt: config.commands.fmt.normalize()?,
        clippy: config.commands.clippy.normalize()?,
        scan: config.commands.scan.normalize()?,
        mcp: config.commands.mcp.normalize()?,
    };

    let required_checks = freeze_required_checks(config)?;

    Ok(FrozenBootstrapContract {
        version: config.version.clone(),
        commands,
        required_checks,
        task_types: config.task_types.clone(),
        allow_paths: config.allow_paths.clone(),
        deny_paths: config.deny_paths.clone(),
        skills: config.skills.clone(),
    })
}

fn freeze_required_checks(config: &BootstrapConfig) -> Result<BTreeMap<String, Vec<Vec<String>>>> {
    config
        .required_checks
        .iter()
        .map(|(name, checks)| {
            let frozen_checks = checks
                .iter()
                .map(|check| {
                    shell_words::split(check)
                        .with_context(|| {
                            format!("invalid requiredChecks entry for {name}: {check}")
                        })
                        .and_then(|argv| {
                            if argv.is_empty() {
                                bail!("requiredChecks entry for {name} cannot be empty");
                            }
                            if argv.iter().any(|part| part.is_empty()) {
                                bail!(
                                    "requiredChecks entry for {name} cannot contain empty elements"
                                );
                            }
                            Ok(argv)
                        })
                })
                .collect::<Result<Vec<_>>>()?;

            Ok((name.clone(), frozen_checks))
        })
        .collect::<Result<BTreeMap<_, _>>>()
}

/// Enforces the minimal invariants Phase 0 expects from the bootstrap contract.
pub fn validate_bootstrap_config(config: &BootstrapConfig) -> Result<()> {
    if !config.memory_policy.require_anchors {
        bail!("memoryPolicy.requireAnchors must stay true for bootstrap skills");
    }
    if !config.memory_policy.reject_unanchored_writes {
        bail!("memoryPolicy.rejectUnanchoredWrites must stay true for bootstrap skills");
    }

    validate_bootstrap_commands(config)?;
    validate_required_checks(config)?;

    let task_types: BTreeSet<&str> = config.task_types.iter().map(String::as_str).collect();
    for key in config.required_checks.keys() {
        if !task_types.contains(key.as_str()) {
            bail!(
                "requiredChecks references unknown taskType {}: declare it under taskTypes",
                key
            );
        }
    }

    let mut seen_skill_ids = BTreeSet::new();
    for skill in &config.skills {
        if !seen_skill_ids.insert(skill.id.as_str()) {
            bail!("duplicate skill id: {}", skill.id);
        }
        if skill.writes_memory && !skill.requires_anchors {
            bail!(
                "skill {} writes memory and must keep requiresAnchors true",
                skill.id
            );
        }
    }

    Ok(())
}

fn validate_bootstrap_commands(config: &BootstrapConfig) -> Result<()> {
    config.commands.build.normalize()?;
    config.commands.test.normalize()?;
    config.commands.fmt.normalize()?;
    config.commands.clippy.normalize()?;
    config.commands.scan.normalize()?;
    config.commands.mcp.normalize()?;
    Ok(())
}

fn validate_required_checks(config: &BootstrapConfig) -> Result<()> {
    freeze_required_checks(config).map(|_| ())
}
