use anyhow::{bail, Result};
use repolens_core::model::bootstrap_run::BootstrapCommandResult;
use std::fs::File;
use std::path::{Path, PathBuf};

pub struct BootstrapCommandRunner;

impl BootstrapCommandRunner {
    pub fn run_all(
        repo_root: &Path,
        run_dir: &Path,
        commands: &[Vec<String>],
    ) -> Result<Vec<BootstrapCommandResult>> {
        let commands_dir = run_dir.join("commands");

        commands
            .iter()
            .enumerate()
            .map(|(index, argv)| Self::run_one(repo_root, &commands_dir, index, argv))
            .collect()
    }

    fn run_one(
        repo_root: &Path,
        commands_dir: &Path,
        index: usize,
        argv: &[String],
    ) -> Result<BootstrapCommandResult> {
        let Some(program) = argv.first() else {
            bail!("bootstrap command argv cannot be empty");
        };

        let stdout_path = commands_dir.join(format!("{index:02}.stdout.log"));
        let stderr_path = commands_dir.join(format!("{index:02}.stderr.log"));

        let stdout_file = create_log_file(&stdout_path)?;
        let stderr_file = create_log_file(&stderr_path)?;

        let status = std::process::Command::new(program)
            .args(&argv[1..])
            .current_dir(repo_root)
            .stdin(std::process::Stdio::null())
            .stdout(stdout_file)
            .stderr(stderr_file)
            .status();

        match status {
            Ok(status) => Ok(BootstrapCommandResult {
                argv: argv.to_vec(),
                exit_code: status.code(),
                stdout_log_path: stdout_path.to_string_lossy().to_string(),
                stderr_log_path: stderr_path.to_string_lossy().to_string(),
                succeeded: status.success(),
            }),
            Err(err) => {
                std::fs::write(&stderr_path, err.to_string().as_bytes())?;
                Ok(BootstrapCommandResult {
                    argv: argv.to_vec(),
                    exit_code: None,
                    stdout_log_path: stdout_path.to_string_lossy().to_string(),
                    stderr_log_path: stderr_path.to_string_lossy().to_string(),
                    succeeded: false,
                })
            }
        }
    }
}

fn create_log_file(path: &PathBuf) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(File::create(path)?)
}
