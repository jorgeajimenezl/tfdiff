//! Machine-readable output.

use std::io;

use serde::Serialize;

use super::Renderer;
use crate::report::Report;

/// Bumped on any incompatible change to the JSON shape.
const SCHEMA_VERSION: u32 = 1;

pub struct Json;

#[derive(Serialize)]
struct Document<'a> {
    schema_version: u32,
    #[serde(flatten)]
    report: &'a Report,
}

impl Renderer for Json {
    fn name(&self) -> &'static str {
        "json"
    }

    fn render(&self, report: &Report, out: &mut dyn io::Write) -> io::Result<()> {
        let document = Document {
            schema_version: SCHEMA_VERSION,
            report,
        };
        serde_json::to_writer_pretty(&mut *out, &document).map_err(io::Error::from)?;
        writeln!(out)
    }
}
