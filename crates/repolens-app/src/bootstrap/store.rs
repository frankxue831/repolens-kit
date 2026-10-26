use anyhow::{ensure, Context, Result};
use fs4::FileExt;
use repolens_core::model::bootstrap_run::{BootstrapRunPlan, BootstrapRunReport};
use repolens_core::storage::RepoStoragePaths;
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct BootstrapRunStore;

pub struct ActiveRunLock {
    repo_root: PathBuf,
    run_id: String,
    active_file: std::fs::File,
}

pub enum ActiveRunLockResult {
    Locked(ActiveRunLock),
    NotActive,
}

pub enum TryActiveRunLockResult {
    Locked(ActiveRunLock),
    NotActive,
    Busy,
}

impl BootstrapRunStore {
    pub const TERMINAL_REPORT_POLL_ATTEMPTS: usize = 100;
    pub const TERMINAL_REPORT_POLL_INTERVAL: std::time::Duration =
        std::time::Duration::from_millis(10);

    // Compile-time guard: load_report relies on at least one poll iteration so the
    // loop always returns before falling through to the unreachable!() below.
    const _NONZERO_POLL_ATTEMPTS: () = assert!(Self::TERMINAL_REPORT_POLL_ATTEMPTS > 0);

    pub fn begin_run(repo_root: &Path, run_id: &str, plan: &BootstrapRunPlan) -> Result<PathBuf> {
        let bootstrap_dir = RepoStoragePaths::bootstrap_dir(repo_root);
        std::fs::create_dir_all(&bootstrap_dir)?;

        let active = RepoStoragePaths::bootstrap_active_path(repo_root);
        let current = RepoStoragePaths::bootstrap_current_path(repo_root);
        let run_dir = RepoStoragePaths::bootstrap_run_dir(repo_root, run_id);
        let commands_dir = run_dir.join("commands");

        let mut active_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&active)
            .with_context(|| format!("failed to acquire {}", active.display()))?;

        // Hold an exclusive fs lock on ACTIVE for the duration of begin_run so
        // concurrent observers that open the freshly-created file cannot see
        // half-written intermediate state.
        active_file
            .try_lock_exclusive()
            .with_context(|| format!("failed to lock {}", active.display()))?;

        let result = (|| -> Result<PathBuf> {
            std::fs::create_dir_all(&commands_dir)?;
            atomic_write(
                &run_dir.join("plan.json"),
                &serde_json::to_vec_pretty(plan)?,
            )?;
            atomic_write(&current, run_id.as_bytes())?;
            active_file.write_all(run_id.as_bytes())?;
            active_file.flush()?;
            Ok(run_dir.clone())
        })();

        match result {
            Ok(run_dir) => {
                let _ = FileExt::unlock(&active_file);
                drop(active_file);
                Ok(run_dir)
            }
            Err(err) => {
                let _ = FileExt::unlock(&active_file);
                drop(active_file);
                cleanup_partial_run_state(&active, &current, &run_dir, run_id);
                Err(err)
            }
        }
    }

    pub fn load_plan(repo_root: &Path, run_id: &str) -> Result<BootstrapRunPlan> {
        let path = RepoStoragePaths::bootstrap_run_dir(repo_root, run_id).join("plan.json");
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn load_report(repo_root: &Path, run_id: &str) -> Result<Option<BootstrapRunReport>> {
        let path = RepoStoragePaths::bootstrap_run_dir(repo_root, run_id).join("report.json");
        for attempt in 0..Self::TERMINAL_REPORT_POLL_ATTEMPTS {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(err) => return Err(err.into()),
            };

            match serde_json::from_slice(&bytes) {
                Ok(report) => return Ok(Some(report)),
                Err(err)
                    if err.classify() == serde_json::error::Category::Eof
                        && attempt + 1 < Self::TERMINAL_REPORT_POLL_ATTEMPTS =>
                {
                    std::thread::sleep(Self::TERMINAL_REPORT_POLL_INTERVAL);
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!("terminal report polling exhausted without returning");
    }

    pub fn try_lock_active_run(repo_root: &Path, run_id: &str) -> Result<TryActiveRunLockResult> {
        let active_path = RepoStoragePaths::bootstrap_active_path(repo_root);
        let active_file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&active_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TryActiveRunLockResult::NotActive);
            }
            Err(err) => return Err(err.into()),
        };

        match active_file.try_lock_exclusive() {
            Ok(()) => Ok(TryActiveRunLockResult::Locked(ActiveRunLock {
                repo_root: repo_root.to_path_buf(),
                run_id: run_id.to_string(),
                active_file,
            })),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                Ok(TryActiveRunLockResult::Busy)
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn assert_active_run_ownership(lock: &mut ActiveRunLock) -> Result<()> {
        let active_run_id = read_active_runtime_owner_from_file(&mut lock.active_file)?;
        ensure!(
            active_run_id == lock.run_id,
            "active bootstrap run belongs to {}",
            active_run_id
        );
        Ok(())
    }

    pub fn finalize_terminal_transition(
        lock: ActiveRunLock,
        report: &BootstrapRunReport,
    ) -> Result<()> {
        let run_dir = RepoStoragePaths::bootstrap_run_dir(&lock.repo_root, &lock.run_id);
        atomic_write(
            &run_dir.join("report.json"),
            &serde_json::to_vec_pretty(report)?,
        )?;

        Self::clear_terminal_markers(lock)
    }

    pub fn clear_terminal_markers(lock: ActiveRunLock) -> Result<()> {
        let ActiveRunLock {
            repo_root,
            active_file,
            ..
        } = lock;
        let current_path = RepoStoragePaths::bootstrap_current_path(&repo_root);
        remove_if_exists(&current_path)?;

        let active_path = RepoStoragePaths::bootstrap_active_path(&repo_root);
        drop(active_file);
        remove_if_exists(&active_path)?;
        Ok(())
    }

    pub fn finalize_terminal_transition_without_active(
        repo_root: &Path,
        run_id: &str,
        report: &BootstrapRunReport,
    ) -> Result<()> {
        let run_dir = RepoStoragePaths::bootstrap_run_dir(repo_root, run_id);
        atomic_write(
            &run_dir.join("report.json"),
            &serde_json::to_vec_pretty(report)?,
        )?;

        let current_path = RepoStoragePaths::bootstrap_current_path(repo_root);
        remove_if_owned(&current_path, run_id)?;
        Ok(())
    }

    pub fn cleanup_stale_terminal_markers(repo_root: &Path, run_id: &str) -> Result<()> {
        Self::cleanup_stale_terminal_markers_internal(repo_root, run_id, false)
    }

    pub fn cleanup_stale_terminal_markers_with_hint(repo_root: &Path, run_id: &str) -> Result<()> {
        Self::cleanup_stale_terminal_markers_internal(repo_root, run_id, true)
    }
}

impl BootstrapRunStore {
    fn cleanup_stale_terminal_markers_internal(
        repo_root: &Path,
        run_id: &str,
        allow_malformed_active_hint: bool,
    ) -> Result<()> {
        let current_path = RepoStoragePaths::bootstrap_current_path(repo_root);
        let current_owned = is_owned_by_run(&current_path, "bootstrap CURRENT", run_id)?;

        match Self::try_lock_active_run(repo_root, run_id)? {
            TryActiveRunLockResult::Locked(mut lock) => {
                let should_clear_active = match Self::assert_active_run_ownership(&mut lock) {
                    Ok(()) => true,
                    Err(err)
                        if allow_malformed_active_hint
                            && current_owned
                            && is_malformed_error(&err) =>
                    {
                        true
                    }
                    Err(err) if is_wrong_owner_error(&err) || is_malformed_error(&err) => false,
                    Err(err) => return Err(err),
                };

                if should_clear_active {
                    Self::clear_stale_terminal_markers(lock)?;
                    return Ok(());
                }
            }
            TryActiveRunLockResult::NotActive | TryActiveRunLockResult::Busy => {}
        }

        if current_owned {
            remove_if_owned(&current_path, run_id)?;
        }

        Ok(())
    }

    fn clear_stale_terminal_markers(lock: ActiveRunLock) -> Result<()> {
        let ActiveRunLock {
            repo_root,
            run_id,
            active_file,
        } = lock;
        let current_path = RepoStoragePaths::bootstrap_current_path(&repo_root);
        remove_if_owned(&current_path, &run_id)?;

        let active_path = RepoStoragePaths::bootstrap_active_path(&repo_root);
        drop(active_file);
        remove_if_exists(&active_path)?;
        Ok(())
    }
}

fn read_nonempty_owner(path: &Path, label: &str) -> Result<String> {
    let owner = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let owner = owner.trim().to_string();
    ensure!(!owner.is_empty(), "{} is malformed or empty", label);
    Ok(owner)
}

fn read_nonempty_owner_from_file(file: &mut std::fs::File, label: &str) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut owner = String::new();
    file.read_to_string(&mut owner)?;
    let owner = owner.trim().to_string();
    ensure!(!owner.is_empty(), "{} is malformed or empty", label);
    Ok(owner)
}

fn read_active_runtime_owner_from_file(file: &mut std::fs::File) -> Result<String> {
    let owner = read_nonempty_owner_from_file(file, "bootstrap ACTIVE")?;
    ensure!(
        is_valid_runtime_run_id(&owner),
        "bootstrap ACTIVE is malformed or empty"
    );
    Ok(owner)
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn remove_if_owned(path: &Path, run_id: &str) -> Result<()> {
    match read_nonempty_owner(path, "bootstrap CURRENT") {
        Ok(owner) if owner == run_id => remove_if_exists(path),
        Ok(_) => Ok(()),
        Err(err) if is_missing_or_malformed_error(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

fn is_owned_by_run(path: &Path, label: &str, run_id: &str) -> Result<bool> {
    match read_nonempty_owner(path, label) {
        Ok(owner) => Ok(owner == run_id),
        Err(err) if is_missing_or_malformed_error(&err) => Ok(false),
        Err(err) => Err(err),
    }
}

fn is_missing_or_malformed_error(err: &anyhow::Error) -> bool {
    is_malformed_error(err)
        || err
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
}

fn is_malformed_error(err: &anyhow::Error) -> bool {
    err.to_string().contains("malformed or empty")
}

fn is_wrong_owner_error(err: &anyhow::Error) -> bool {
    err.to_string().contains("active bootstrap run belongs to ")
}

pub(crate) fn is_valid_runtime_run_id(run_id: &str) -> bool {
    const PREFIX: &str = "run-";
    const RUN_ID_LEN: usize = 56;

    if run_id.len() != RUN_ID_LEN || !run_id.starts_with(PREFIX) {
        return false;
    }

    let bytes = run_id.as_bytes();
    if bytes[12] != b'-' || bytes[19] != b'-' {
        return false;
    }

    if !bytes[4..12].iter().all(|byte| byte.is_ascii_digit())
        || !bytes[13..19].iter().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }

    uuid::Uuid::parse_str(&run_id[20..]).is_ok()
}

fn cleanup_partial_run_state(active: &Path, current: &Path, run_dir: &Path, run_id: &str) {
    let _ = std::fs::remove_file(active);
    let _ = std::fs::remove_file(tmp_path_for(active));

    if let Ok(current_run_id) = std::fs::read_to_string(current) {
        if current_run_id.trim() == run_id {
            let _ = std::fs::remove_file(current);
        }
    }
    let _ = std::fs::remove_file(tmp_path_for(current));

    if run_dir.exists() {
        let _ = std::fs::remove_dir_all(run_dir);
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path_for(path);
    std::fs::write(&tmp, bytes).with_context(|| format!("failed to stage {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("failed to commit {}", path.display()))?;
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut os = OsString::from(path.as_os_str());
    os.push(".tmp");
    PathBuf::from(os)
}
