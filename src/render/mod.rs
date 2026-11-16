//! Pluggable output formats.
//!
//! To add a format: create a module with a type implementing [`Renderer`] and
//! add it to [`registry`]. `--format` validation, `--help` and the secret-leak
//! test all iterate the registry, so nothing else needs to change.
//!
//! A renderer only receives a [`Report`], which holds redacted values
//! exclusively, so a format cannot leak secrets.

mod json;
mod text;

use std::io;

use crate::report::Report;

/// An output format.
pub trait Renderer: Sync {
    /// The value accepted by `--format`.
    fn name(&self) -> &'static str;

    /// Writes the whole report.
    ///
    /// # Errors
    ///
    /// Propagates failures of `out`.
    fn render(&self, report: &Report, out: &mut dyn io::Write) -> io::Result<()>;
}

/// Format used when `--format` is not given.
pub const DEFAULT_FORMAT: &str = "text";

/// Every available format.
pub fn registry() -> &'static [&'static dyn Renderer] {
    &[&text::Text, &json::Json]
}

/// Looks a format up by name.
pub fn find(name: &str) -> Option<&'static dyn Renderer> {
    registry()
        .iter()
        .copied()
        .find(|renderer| renderer.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_format_is_registered() {
        assert!(find(DEFAULT_FORMAT).is_some());
    }

    #[test]
    fn format_names_are_unique() {
        let mut names: Vec<_> = registry().iter().map(|r| r.name()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total);
    }
}
