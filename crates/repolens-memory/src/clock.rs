//! Minimal injectable clock for the memory layer.
//!
//! Production code reads the real UTC time. Tests (and only tests) can pin
//! "now" by setting `REPOLENS_CLOCK_OVERRIDE` to an RFC-3339 timestamp in the
//! environment of a `repolens mcp` process, which lets end-to-end tests move
//! time forward to exercise decay. An unparsable value is ignored.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Environment variable that overrides [`now`] (RFC-3339, e.g.
/// `2026-01-01T00:00:00Z`). Intended for tests.
pub const CLOCK_OVERRIDE_ENV: &str = "REPOLENS_CLOCK_OVERRIDE";

/// Current time for memory bookkeeping (creation, access, decay).
pub fn now() -> OffsetDateTime {
    std::env::var(CLOCK_OVERRIDE_ENV)
        .ok()
        .and_then(|v| parse_override(&v))
        .unwrap_or_else(OffsetDateTime::now_utc)
}

/// True when [`CLOCK_OVERRIDE_ENV`] holds a valid timestamp. Callers treat
/// this as a "what if" clock and must not persist anything derived from it.
pub fn is_overridden() -> bool {
    std::env::var(CLOCK_OVERRIDE_ENV)
        .ok()
        .and_then(|v| parse_override(&v))
        .is_some()
}

fn parse_override(v: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(v.trim(), &Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339_override() {
        let t = parse_override("2026-01-31T00:00:00Z").unwrap();
        assert_eq!(t.unix_timestamp(), 1_769_817_600);
        assert!(parse_override("not a time").is_none());
    }
}
