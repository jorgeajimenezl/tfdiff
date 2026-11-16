//! Library error type. The binary wraps these with `anyhow` for context.

use std::io;
use std::path::PathBuf;

/// Everything that can go wrong while producing a report.
///
/// No variant ever carries a value taken from the plan: plan contents may be
/// secret, and error messages end up in CI logs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read the plan from {origin}")]
    ReadPlan {
        origin: String,
        #[source]
        source: io::Error,
    },

    /// `serde_json` messages can quote the offending value, so only the error
    /// category and position are kept.
    #[error(
        "the plan is not valid `terraform show -json` output ({category} at line {line}, column {column})"
    )]
    PlanJson {
        category: &'static str,
        line: usize,
        column: usize,
    },

    #[error("unsupported plan format_version {0:?} (tfdiff understands 1.x)")]
    UnsupportedFormatVersion(String),

    #[error("cannot read the config file {}", path.display())]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("invalid config file {}", path.display())]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("unknown output format {0:?}")]
    UnknownFormat(String),

    #[error("cannot write the report")]
    Write(#[source] io::Error),
}
