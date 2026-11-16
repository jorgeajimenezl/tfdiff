use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use tfdiff::cli::Cli;

/// Exit status for anything that prevents a report (bad input, bad usage, I/O).
const EXIT_ERROR: u8 = 1;

fn main() -> ExitCode {
    // clap exits with 2 on usage errors by default, which would collide with
    // "expectation breached"; map usage errors to the generic error status.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return ExitCode::from(if err.use_stderr() { EXIT_ERROR } else { 0 });
        }
    };

    match try_main(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "tfdiff: {err:#}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn try_main(cli: &Cli) -> anyhow::Result<u8> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    let outcome =
        tfdiff::run(cli, &mut io::stdin().lock(), &mut stdout).context("no report was produced")?;
    Ok(outcome.exit_code())
}
