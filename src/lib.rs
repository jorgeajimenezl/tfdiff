//! tfdiff turns `terraform show -json <plan>` into a short, reviewable summary
//! of what really changes.
//!
//! Pipeline: [`plan`] (parse) → [`diff`] (compare, redact) → [`report`]
//! (redaction-safe model) → [`gate`] (optional expectation) → [`render`]
//! (pluggable output formats).

pub mod cli;
pub mod config;
pub mod diff;
pub mod error;
pub mod gate;
pub mod labels;
pub mod plan;
pub mod redact;
pub mod render;
pub mod report;
pub mod schema;

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub use crate::error::Error;
use crate::gate::Verdict;

/// What a successful run concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Result of `--expect`, if one was requested.
    pub verdict: Option<Verdict>,
}

impl Outcome {
    /// Process exit status: 0 upheld or no expectation, 2 breached, 3 undetermined.
    pub fn exit_code(self) -> u8 {
        match self.verdict {
            None | Some(Verdict::Upheld) => 0,
            Some(Verdict::Breached) => 2,
            Some(Verdict::Undetermined) => 3,
        }
    }
}

/// Reads a plan, diffs it and writes the report to `stdout`.
///
/// The plan is only ever held in memory; nothing derived from it is written
/// anywhere except the redacted report.
///
/// # Errors
///
/// Returns an [`Error`] when the plan, config or schema cannot be read or parsed, or
/// the report cannot be written.
pub fn run(cli: &cli::Cli, stdin: &mut dyn Read, stdout: &mut dyn Write) -> Result<Outcome, Error> {
    let renderer =
        render::find(&cli.format).ok_or_else(|| Error::UnknownFormat(cli.format.clone()))?;
    let config = config::Config::discover(cli.config.as_deref())?;
    let schemas = cli
        .schema
        .as_deref()
        .map(schema::Schemas::from_file)
        .transpose()?;

    let mut report = {
        let bytes = read_plan(cli.plan.as_deref(), stdin)?;
        let plan = plan::Plan::from_slice(&bytes)?;
        diff::diff_plan(&plan, &config, schemas.as_ref())
    };
    report.gate = cli.expect.map(|expect| gate::evaluate(expect, &report));

    renderer
        .render(&report, stdout)
        .and_then(|()| stdout.flush())
        .map_err(Error::Write)?;

    Ok(Outcome {
        verdict: report.gate.as_ref().map(|gate| gate.verdict),
    })
}

fn read_plan(path: Option<&Path>, stdin: &mut dyn Read) -> Result<Vec<u8>, Error> {
    match path {
        Some(path) if path != Path::new("-") => fs::read(path).map_err(|source| Error::ReadPlan {
            origin: path.display().to_string(),
            source,
        }),
        _ => {
            let mut bytes = Vec::new();
            stdin
                .read_to_end(&mut bytes)
                .map_err(|source| Error::ReadPlan {
                    origin: "standard input".to_owned(),
                    source,
                })?;
            Ok(bytes)
        }
    }
}
