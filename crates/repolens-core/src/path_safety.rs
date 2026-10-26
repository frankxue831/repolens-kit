use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

fn invalid_input(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

fn looks_like_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with("\\\\")
        || path.starts_with("//")
}

pub fn validate_relative_subpath(path: &str) -> io::Result<()> {
    if path.is_empty() {
        return Err(invalid_input("path must not be empty"));
    }
    if looks_like_windows_absolute(path) {
        return Err(invalid_input(format!(
            "path must be relative and portable: {path}"
        )));
    }

    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(invalid_input(format!("path must be relative: {path}")));
    }

    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(invalid_input(format!(
                    "path must not escape its base directory: {path}"
                )));
            }
        }
    }

    Ok(())
}

pub fn validate_prefixed_subpath(path: &str, prefix: &str) -> io::Result<()> {
    validate_relative_subpath(path)?;

    let mut components = Path::new(path).components();
    let Some(Component::Normal(first)) = components.next() else {
        return Err(invalid_input(format!("path must start with {prefix}/")));
    };
    if first != std::ffi::OsStr::new(prefix) || components.next().is_none() {
        return Err(invalid_input(format!("path must start with {prefix}/")));
    }

    Ok(())
}

pub fn canonicalize_within_base(base: &Path, candidate: &Path) -> io::Result<PathBuf> {
    let canonical_base = fs::canonicalize(base)?;
    let canonical_candidate = fs::canonicalize(candidate)?;
    if !canonical_candidate.starts_with(&canonical_base) {
        return Err(invalid_input(format!(
            "path escapes base directory: {}",
            candidate.display()
        )));
    }
    Ok(canonical_candidate)
}

pub fn resolve_existing_subpath(base: &Path, path: &str) -> io::Result<PathBuf> {
    validate_relative_subpath(path)?;
    canonicalize_within_base(base, &base.join(path))
}

pub fn resolve_existing_prefixed_subpath(
    base: &Path,
    path: &str,
    prefix: &str,
) -> io::Result<PathBuf> {
    validate_prefixed_subpath(path, prefix)?;
    canonicalize_within_base(base, &base.join(path))
}

/// Read a pack-managed file at `content_path` (relative to `pack_dir`), enforcing
/// the `files/` prefix invariant and rejecting files larger than `max_bytes`.
pub fn read_packed_file_capped(
    pack_dir: &Path,
    content_path: &str,
    max_bytes: u64,
) -> io::Result<String> {
    let full_path = resolve_existing_prefixed_subpath(pack_dir, content_path, "files")?;
    let size = fs::metadata(&full_path)?.len();
    if size > max_bytes {
        return Err(io::Error::other(format!(
            "file exceeds configured size limit of {max_bytes} bytes: {content_path}"
        )));
    }
    fs::read_to_string(&full_path)
}

pub fn resolve_subpath_for_create(base: &Path, path: &str) -> io::Result<PathBuf> {
    resolve_subpath_for_create_inner(base, path, None)
}

pub fn resolve_prefixed_subpath_for_create(
    base: &Path,
    path: &str,
    prefix: &str,
) -> io::Result<PathBuf> {
    resolve_subpath_for_create_inner(base, path, Some(prefix))
}

fn resolve_subpath_for_create_inner(
    base: &Path,
    path: &str,
    prefix: Option<&str>,
) -> io::Result<PathBuf> {
    match prefix {
        Some(p) => validate_prefixed_subpath(path, p)?,
        None => validate_relative_subpath(path)?,
    }

    let canonical_base = fs::canonicalize(base)?;
    let mut current = canonical_base.clone();
    for component in Path::new(path).components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let resolved = fs::canonicalize(&current)?;
                if !resolved.starts_with(&canonical_base) {
                    return Err(invalid_input(format!(
                        "path escapes base directory via symlink: {path}"
                    )));
                }
                current = resolved;
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => break,
            Err(err) => return Err(err),
        }
    }

    Ok(base.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_dir_segments() {
        let err = validate_relative_subpath("../secret.txt").unwrap_err();
        assert!(err.to_string().contains("escape"));
    }

    #[test]
    fn rejects_windows_absolute_paths() {
        let err = validate_relative_subpath("C:\\secret.txt").unwrap_err();
        assert!(err.to_string().contains("relative"));
    }

    #[test]
    fn validates_prefixed_subpaths() {
        validate_prefixed_subpath("files/src/app.rs", "files").unwrap();
        assert!(validate_prefixed_subpath("src/app.rs", "files").is_err());
    }
}
