/// Serde helpers for manifest-style epoch timestamps.
///
/// Java writes pack manifest timestamps as bare JSON numbers with exactly
/// 9 fractional digits, e.g. `1774753241.321919000`.  Representing this as
/// `f64` would silently lose precision (f64 has ~15-16 significant decimal
/// digits but the value has 19).  We use `rust_decimal::Decimal` instead,
/// which preserves the exact numeric string representation.
///
/// # Wire format
/// ```json
/// "timestamp" : 1774753241.321919000
/// ```
use rust_decimal::Decimal;
use serde::de::Deserializer;
use serde::ser::Serializer;
use std::str::FromStr;

/// Newtype wrapper around `Decimal` that round-trips Java manifest timestamps
/// without precision loss.
///
/// Requires `serde_json`'s `arbitrary_precision` feature to be enabled.
/// With that feature, JSON numbers are preserved as exact decimal strings
/// through the serde pipeline, so `1774753241.321919000` survives a full
/// deserialize → serialize round-trip without losing trailing zeros.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct JavaEpochSeconds(pub Decimal);

impl JavaEpochSeconds {
    pub fn inner(&self) -> &Decimal {
        &self.0
    }

    pub fn into_inner(self) -> Decimal {
        self.0
    }
}

impl serde::Serialize for JavaEpochSeconds {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // With arbitrary_precision enabled, serde_json::Number stores the exact
        // decimal string and serializes it as a raw JSON number token.
        let s = self.0.to_string();
        let num: serde_json::Number = serde_json::from_str(&s)
            .map_err(|e| serde::ser::Error::custom(format!("Decimal->Number: {e}")))?;
        serde::Serialize::serialize(&num, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for JavaEpochSeconds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // With arbitrary_precision, serde_json::Number preserves the exact
        // decimal string from the JSON source. We deserialize as Number first,
        // then parse the string into Decimal.
        let num = serde_json::Number::deserialize(deserializer)?;
        let s = num.to_string();
        Decimal::from_str(&s)
            .map(JavaEpochSeconds)
            .map_err(|e| serde::de::Error::custom(format!("invalid decimal `{s}`: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Serde module form — usable as `#[serde(with = "java_epoch_seconds")]`
// on a plain `Decimal` field.
// ---------------------------------------------------------------------------

pub fn serialize<S: Serializer>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&JavaEpochSeconds(*value), serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    serde::Deserialize::deserialize(deserializer).map(|j: JavaEpochSeconds| j.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    /// The exact fixture value from `manifest.json`.
    const FIXTURE_JSON: &str = "1774753241.321919000";

    #[test]
    fn deserializes_fixture_value_with_full_precision() {
        let ts: JavaEpochSeconds =
            serde_json::from_str(FIXTURE_JSON).expect("should parse fixture timestamp");
        assert_eq!(ts.inner().to_string(), "1774753241.321919000");
    }

    #[test]
    fn full_precision_survives_roundtrip() {
        let ts: JavaEpochSeconds = serde_json::from_str(FIXTURE_JSON).expect("parse");
        let serialized = serde_json::to_string(&ts).expect("serialize");
        assert_eq!(
            serialized, FIXTURE_JSON,
            "serialized output must match Java format exactly"
        );
        let rt: JavaEpochSeconds = serde_json::from_str(&serialized).expect("re-parse");
        assert_eq!(ts, rt);
    }

    #[test]
    fn serde_module_form_roundtrips() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Wrapper {
            #[serde(with = "super")]
            ts: Decimal,
        }
        let json = r#"{"ts":1774753241.321919000}"#;
        let w: Wrapper = serde_json::from_str(json).expect("module-form deserialize");
        assert_eq!(w.ts.to_string(), "1774753241.321919000");
        let reserialized = serde_json::to_string(&w).expect("module-form serialize");
        assert_eq!(reserialized, json);
    }
}
