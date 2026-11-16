//! Typed view of the parts of `terraform show -json <plan>` that tfdiff reads.
//!
//! Unknown fields are ignored so newer Terraform versions keep working.

use serde::Deserialize;
use serde_json::Value;
use serde_json::error::Category;

use crate::error::Error;
use crate::redact::Raw;

#[derive(Debug, Deserialize)]
pub struct Plan {
    pub format_version: String,
    #[serde(default)]
    pub resource_changes: Vec<ResourceChange>,
}

#[derive(Debug, Deserialize)]
pub struct ResourceChange {
    pub address: String,
    #[serde(default)]
    pub previous_address: Option<String>,
    #[serde(default)]
    pub mode: String,
    #[serde(rename = "type", default)]
    pub resource_type: String,
    /// Fully qualified provider, e.g. `registry.terraform.io/hashicorp/azurerm`.
    #[serde(default)]
    pub provider_name: Option<String>,
    pub change: Change,
}

#[derive(Debug, Deserialize)]
pub struct Change {
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub before: Raw,
    #[serde(default)]
    pub after: Raw,
    #[serde(default)]
    pub after_unknown: Raw,
    #[serde(default)]
    pub before_sensitive: Raw,
    #[serde(default)]
    pub after_sensitive: Raw,
    /// Attribute paths that force a replacement; elements are attribute names or indexes.
    #[serde(default)]
    pub replace_paths: Vec<Vec<Value>>,
}

impl Plan {
    /// Parses plan JSON held in memory.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PlanJson`] for malformed input (without echoing any of
    /// it) and [`Error::UnsupportedFormatVersion`] for a non-1.x plan format.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        let plan: Self = serde_json::from_slice(bytes).map_err(|err| Error::PlanJson {
            category: match err.classify() {
                Category::Io => "read failure",
                Category::Syntax => "syntax error",
                Category::Data => "unexpected structure",
                Category::Eof => "unexpected end of input",
            },
            line: err.line(),
            column: err.column(),
        })?;
        if plan.format_version.split('.').next() != Some("1") {
            return Err(Error::UnsupportedFormatVersion(plan.format_version));
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errors_never_echo_plan_content() {
        // serde would normally say: invalid type: string "hunter2-CANARY", expected a sequence
        let bad = br#"{"format_version":"1.2","resource_changes":"hunter2-CANARY"}"#;
        let err = Plan::from_slice(bad).unwrap_err();
        let shown = format!("{err} / {err:?}");
        assert!(!shown.contains("CANARY"), "{shown}");
        assert!(shown.contains("unexpected structure"), "{shown}");
    }

    #[test]
    fn rejects_other_major_versions() {
        let err = Plan::from_slice(br#"{"format_version":"2.0"}"#).unwrap_err();
        assert!(matches!(err, Error::UnsupportedFormatVersion(v) if v == "2.0"));
    }

    #[test]
    fn debug_output_hides_raw_values() {
        let plan = Plan::from_slice(
            br#"{"format_version":"1.2","resource_changes":[{"address":"a.b","change":{"before":{"p":"CANARY"}}}]}"#,
        )
        .unwrap();
        assert!(!format!("{plan:?}").contains("CANARY"));
    }
}
