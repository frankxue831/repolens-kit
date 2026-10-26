use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Compute the lowercase hex SHA-256 digest of a file's contents using streaming reads.
pub fn sha256_hex(path: &Path) -> io::Result<String> {
    let (sha, _) = sha256_and_line_count(path)?;
    Ok(sha)
}

/// Count the number of lines in a file (including blank lines) using streaming reads.
pub fn count_lines(path: &Path) -> io::Result<i32> {
    let (_, loc) = sha256_and_line_count(path)?;
    Ok(loc)
}

/// Compute SHA-256 and line count in a single pass over the file.
pub fn sha256_and_line_count(path: &Path) -> io::Result<(String, i32)> {
    let file = fs::File::open(path)?;
    let mut reader = io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut line_count: i64 = 0;
    let mut last_byte: Option<u8> = None;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        line_count += buf[..n].iter().filter(|&&b| b == b'\n').count() as i64;
        last_byte = Some(buf[n - 1]);
    }

    // Match BufRead::lines() semantics: each \n-terminated segment is a line,
    // plus a final line if the file is non-empty and doesn't end with \n.
    if last_byte.is_some_and(|b| b != b'\n') {
        line_count += 1;
    }

    let loc = i32::try_from(line_count)
        .map_err(|_| io::Error::other(format!("line count {line_count} exceeds i32::MAX")))?;

    Ok((hex::encode(hasher.finalize()), loc))
}

/// Returns `true` if the file appears to be binary (contains a null byte in the first 8192 bytes).
pub fn is_binary(path: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; 8192];
    let n = file.read(&mut buf)?;
    Ok(buf[..n].contains(&0))
}
