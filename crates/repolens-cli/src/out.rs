//! Standard output for the whole binary. `print!` and `println!` are
//! redefined in `main.rs` to call [`print`], so every command shares one rule
//! for a closed stdout.
//!
//! When the reader goes away early (`repolens scan | head -1`), the write
//! fails with `BrokenPipe`. std's macros panic on that ("failed printing to
//! stdout: Broken pipe"). Here the first `BrokenPipe` marks stdout closed and
//! every later write is dropped, so the command finishes its work (a scan
//! still writes its pack) and exits with its usual code, 0 on success, with
//! nothing on stderr. Any other write error (a full disk, `> /dev/full`)
//! stops the command with one line on stderr and exit code 1 instead of
//! std's panic and exit code 101 (#59).

use std::fmt;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static CLOSED: AtomicBool = AtomicBool::new(false);

/// Write `args` to stdout; a closed stdout is not an error.
pub fn print(args: fmt::Arguments<'_>) {
    if CLOSED.load(Ordering::Relaxed) {
        return;
    }
    if let Err(err) = io::stdout().lock().write_fmt(args) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            CLOSED.store(true, Ordering::Relaxed);
        } else {
            fail(&err);
        }
    }
}

/// Flush stdout at the end of a command, with the same rules as [`print`].
pub fn flush() {
    if CLOSED.load(Ordering::Relaxed) {
        return;
    }
    if let Err(err) = io::stdout().lock().flush() {
        if err.kind() != io::ErrorKind::BrokenPipe {
            fail(&err);
        }
    }
}

/// The one-line message for a failed stdout write.
pub fn write_error_message(err: &io::Error) -> String {
    format!("error: cannot write to standard output: {err}")
}

/// Report a failed stdout write once and exit 1. Stderr may be unusable
/// too, so its own write error is ignored.
pub fn fail(err: &io::Error) -> ! {
    let _ = writeln!(io::stderr(), "{}", write_error_message(err));
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_error_is_one_short_line() {
        let msg = write_error_message(&io::Error::from_raw_os_error(28));
        assert!(msg.starts_with("error: cannot write to standard output: "));
        assert!(!msg.contains('\n'));
        assert!(msg.chars().count() <= 80, "{msg}");
    }
}
