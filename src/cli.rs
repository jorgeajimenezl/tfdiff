//! Command-line interface definition.

use std::path::PathBuf;

use clap::Parser;
use clap::builder::PossibleValuesParser;

use crate::gate::Expect;
use crate::render;

/// Turn `terraform show -json <plan>` into a short, reviewable summary of what really changes.
#[derive(Debug, Parser)]
#[command(
    version,
    about,
    after_help = "Exit status: 0 expectation upheld (or none given), \
    1 error, 2 expectation breached, 3 undetermined (only unknown-after-apply differences remain)."
)]
pub struct Cli {
    /// Plan JSON file; reads standard input when omitted or `-`.
    pub plan: Option<PathBuf>,

    /// Output format.
    #[arg(short, long, default_value = render::DEFAULT_FORMAT, value_parser = format_parser())]
    pub format: String,

    /// Config file [default: ./tfdiff.toml when present].
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Fail unless the plan satisfies this expectation.
    #[arg(short, long, value_enum, value_name = "EXPECTATION")]
    pub expect: Option<Expect>,
}

/// Accepts exactly the formats in the renderer registry, so a new renderer
/// shows up in `--help` and validation without touching this file.
fn format_parser() -> PossibleValuesParser {
    PossibleValuesParser::new(render::registry().iter().map(|renderer| renderer.name()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
