//! Optional `tfdiff.toml` configuration.
//!
//! ```toml
//! [list_keys]
//! "azurerm_network_security_group.security_rule" = "name"
//! "terraform_data.input.rules" = "port"
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::error::Error;

/// File looked up in the working directory when `--config` is not given.
pub const DEFAULT_FILE: &str = "tfdiff.toml";

/// Attribute used to match list elements when nothing more specific is configured.
pub const DEFAULT_LIST_KEY: &str = "name";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `"<resource_type>.<attribute.path>"` → attribute that identifies a list element.
    #[serde(default)]
    list_keys: BTreeMap<String, String>,
}

impl Config {
    /// Loads `explicit` if given; otherwise `./tfdiff.toml` when it exists, else defaults.
    ///
    /// # Errors
    ///
    /// Fails when an explicitly named file is missing, or any config file is unreadable or invalid.
    pub fn discover(explicit: Option<&Path>) -> Result<Self, Error> {
        let (path, required) = match explicit {
            Some(path) => (path, true),
            None => (Path::new(DEFAULT_FILE), false),
        };
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|source| Error::ParseConfig {
                path: path.to_path_buf(),
                source: Box::new(source),
            }),
            Err(err) if !required && err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(Error::ReadConfig {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Parses config text.
    ///
    /// # Errors
    ///
    /// Fails on invalid TOML or unknown settings.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Key attribute for the list at `attr_path` (dot-joined attribute names,
    /// no indexes) of `resource_type`.
    pub fn list_key(&self, resource_type: &str, attr_path: &str) -> &str {
        self.list_keys
            .get(&format!("{resource_type}.{attr_path}"))
            .map_or(DEFAULT_LIST_KEY, String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_over_default() {
        let config =
            Config::parse("[list_keys]\n\"terraform_data.input.rules\" = \"port\"\n").unwrap();
        assert_eq!(config.list_key("terraform_data", "input.rules"), "port");
        assert_eq!(config.list_key("terraform_data", "input.other"), "name");
    }

    #[test]
    fn unknown_settings_are_rejected() {
        assert!(Config::parse("lsit_keys = {}\n").is_err());
    }
}
