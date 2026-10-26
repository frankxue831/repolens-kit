use anyhow::{Context, Result};
use repolens_core::discovery::FileRecord;
use std::fs;
use std::path::Path;

pub const EXCLUDED_MAX_FILE_BYTES_REASON: &str = "excluded:max_file_bytes";
pub const EXCLUDED_MAX_TOTAL_BYTES_REASON: &str = "excluded:max_total_bytes";

// `max_total_bytes` counts only new or changed content materialized in the
// current scan. Carried-forward entries are hardlinked from the base pack
// (see `MergedPackWriter::write`) and do not consume the budget.

fn file_len(path: &Path) -> Result<i64> {
    let len = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .len();
    i64::try_from(len).context("file length exceeds i64::MAX")
}

pub fn apply_full_scan_budgets(
    records: &[FileRecord],
    max_file_bytes: i64,
    max_total_bytes: i64,
) -> Result<Vec<FileRecord>> {
    let mut total_bytes = 0i64;
    let mut limited = Vec::with_capacity(records.len());

    for record in records {
        if !record.included {
            limited.push(record.clone());
            continue;
        }

        let size = file_len(&record.absolute_path)?;
        let mut adjusted = record.clone();
        if size > max_file_bytes {
            adjusted.included = false;
            adjusted.reason = EXCLUDED_MAX_FILE_BYTES_REASON.to_string();
        } else if total_bytes.saturating_add(size) > max_total_bytes {
            adjusted.included = false;
            adjusted.reason = EXCLUDED_MAX_TOTAL_BYTES_REASON.to_string();
        } else {
            total_bytes += size;
        }
        limited.push(adjusted);
    }

    Ok(limited)
}

pub fn changed_file_budget_reason(
    path: &Path,
    remaining_total_bytes: &mut i64,
    max_file_bytes: i64,
) -> Result<Option<&'static str>> {
    let size = file_len(path)?;
    if size > max_file_bytes {
        return Ok(Some(EXCLUDED_MAX_FILE_BYTES_REASON));
    }
    if size > *remaining_total_bytes {
        return Ok(Some(EXCLUDED_MAX_TOTAL_BYTES_REASON));
    }
    *remaining_total_bytes -= size;
    Ok(None)
}
