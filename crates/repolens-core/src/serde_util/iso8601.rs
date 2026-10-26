/// Serde helpers for `time::OffsetDateTime` fields serialized as ISO-8601
/// strings with microsecond precision and a `Z` UTC suffix.
///
/// # Wire format
/// ```json
/// "updatedAt": "2026-03-29T03:00:41.411230Z"
/// ```
///
/// Two serde sub-modules are provided:
///
/// * `required` — for `OffsetDateTime` fields (`#[serde(with = "iso8601::required")]`)
/// * `optional` — for `Option<OffsetDateTime>` fields (`#[serde(with = "iso8601::optional")]`)
use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use std::fmt;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

// ---------------------------------------------------------------------------
// Shared format: RFC-3339 / ISO-8601 with up-to-microsecond precision.
// The `time` crate's Rfc3339 well-known format handles both the parse and
// format directions.  On output we always emit UTC (`Z`).
// ---------------------------------------------------------------------------

fn format_dt(dt: &OffsetDateTime) -> String {
    // Convert to UTC before formatting so we always emit `Z`.
    let utc = dt.to_offset(time::UtcOffset::UTC);
    utc.format(&Rfc3339).unwrap_or_else(|_| dt.to_string())
}

fn parse_dt<E: de::Error>(s: &str) -> Result<OffsetDateTime, E> {
    OffsetDateTime::parse(s, &Rfc3339)
        .map_err(|e| E::custom(format!("invalid ISO-8601 datetime `{s}`: {e}")))
}

// ---------------------------------------------------------------------------
// `required` sub-module — `#[serde(with = "iso8601::required")]`
// ---------------------------------------------------------------------------

pub mod required {
    use super::*;

    pub fn serialize<S: Serializer>(dt: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format_dt(dt))
    }

    struct DtVisitor;

    impl<'de> Visitor<'de> for DtVisitor {
        type Value = OffsetDateTime;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "an ISO-8601 datetime string")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            parse_dt(v)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<OffsetDateTime, D::Error> {
        deserializer.deserialize_str(DtVisitor)
    }
}

// ---------------------------------------------------------------------------
// `optional` sub-module — `#[serde(with = "iso8601::optional")]`
// Serializes `None` as JSON `null`, `Some(dt)` as an ISO-8601 string.
// ---------------------------------------------------------------------------

pub mod optional {
    use super::*;

    pub fn serialize<S: Serializer>(
        opt: &Option<OffsetDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match opt {
            Some(dt) => serializer.serialize_str(&format_dt(dt)),
            None => serializer.serialize_none(),
        }
    }

    struct OptDtVisitor;

    impl<'de> Visitor<'de> for OptDtVisitor {
        type Value = Option<OffsetDateTime>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "an ISO-8601 datetime string or null")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            required::deserialize(deserializer).map(Some)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            parse_dt(v).map(Some)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        deserializer.deserialize_option(OptDtVisitor)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    use time::OffsetDateTime;

    /// Fixture string from an actual `.repolens/session.json`.
    const FIXTURE_STR: &str = "\"2026-03-29T03:00:41.411230Z\"";

    #[derive(serde::Deserialize, serde::Serialize)]
    struct WithRequired {
        #[serde(with = "required")]
        ts: OffsetDateTime,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct WithOptional {
        #[serde(with = "optional")]
        ts: Option<OffsetDateTime>,
    }

    #[test]
    fn parses_fixture_session_timestamp() {
        let json = format!(r#"{{"ts": {FIXTURE_STR}}}"#);
        let w: WithRequired = serde_json::from_str(&json).expect("parse session timestamp");
        assert_eq!(w.ts.year(), 2026);
        assert_eq!(w.ts.month(), time::Month::March);
        assert_eq!(w.ts.day(), 29);
        assert_eq!(w.ts.hour(), 3);
        assert_eq!(w.ts.minute(), 0);
        assert_eq!(w.ts.second(), 41);
    }

    #[test]
    fn roundtrips_required_timestamp() {
        let json = format!(r#"{{"ts": {FIXTURE_STR}}}"#);
        let w: WithRequired = serde_json::from_str(&json).expect("parse");
        let out = serde_json::to_string(&w).expect("serialize");
        // The round-tripped string should contain the same date/time components.
        assert!(out.contains("2026-03-29"), "expected date in output: {out}");
        assert!(out.contains("03:00:41"), "expected time in output: {out}");
    }

    #[test]
    fn optional_some_roundtrips() {
        let json = format!(r#"{{"ts": {FIXTURE_STR}}}"#);
        let w: WithOptional = serde_json::from_str(&json).expect("parse Some");
        assert!(w.ts.is_some());
        let out = serde_json::to_string(&w).expect("serialize Some");
        assert!(out.contains("2026-03-29"), "Some roundtrip: {out}");
    }

    #[test]
    fn optional_none_roundtrips() {
        let json = r#"{"ts": null}"#;
        let w: WithOptional = serde_json::from_str(json).expect("parse None");
        assert!(w.ts.is_none());
        let out = serde_json::to_string(&w).expect("serialize None");
        assert_eq!(out, r#"{"ts":null}"#, "None should serialize as null");
    }

    #[test]
    fn output_uses_z_suffix() {
        let json = format!(r#"{{"ts": {FIXTURE_STR}}}"#);
        let w: WithRequired = serde_json::from_str(&json).expect("parse");
        let out = serde_json::to_string(&w).expect("serialize");
        assert!(out.contains('Z'), "should use Z suffix, got: {out}");
    }
}
