use std::collections::HashMap;
use std::path::PathBuf;

use eyre::Result;
use toml::Table;

use crate::config::Settings;
use crate::default_shorthands::DEFAULT_SHORTHANDS;
use crate::{dirs, file};

pub type Shorthands = HashMap<String, String>;

pub fn get_shorthands() -> Shorthands {
    let mut shorthands = HashMap::new();
    let settings = Settings::get();
    if !settings.disable_default_shorthands {
        shorthands.extend(
            DEFAULT_SHORTHANDS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        );
    };
    if let Some(f) = &settings.shorthands_file {
        match parse_shorthands_file(f.clone()) {
            Ok(custom) => {
                shorthands.extend(custom);
            }
            Err(err) => {
                rtxwarn!("Failed to read shorthands file: {} {:#}", &f.display(), err);
            }
        }
    }
    shorthands
}

fn parse_shorthands_file(mut f: PathBuf) -> Result<Shorthands> {
    if f.starts_with("~") {
        f = dirs::HOME.join(f.strip_prefix("~")?);
    }
    let raw = file::read_to_string(&f)?;
    let toml = raw.parse::<Table>()?;

    let mut shorthands = HashMap::new();
    for (k, v) in toml {
        if let Some(v) = v.as_str() {
            shorthands.insert(k, v.to_string());
        }
    }
    Ok(shorthands)
}

#[cfg(test)]
mod tests {
    use crate::config::SettingsPartial;
    use confique::Partial;
    use pretty_assertions::assert_str_eq;

    use super::*;

    #[test]
    fn test_get_shorthands() {
        Settings::reset();
        let mut partial = SettingsPartial::empty();
        partial.shorthands_file = Some("../fixtures/shorthands.toml".into());
        Settings::add_partial(partial);
        let shorthands = get_shorthands();
        assert_str_eq!(
            shorthands["elixir"],
            "https://github.com/asdf-vm/asdf-elixir.git"
        );
        assert_str_eq!(shorthands["node"], "https://node");
        assert_str_eq!(shorthands["xxxxxx"], "https://xxxxxx");
    }

    #[test]
    fn test_get_shorthands_missing_file() {
        Settings::reset();
        let mut partial = SettingsPartial::empty();
        partial.shorthands_file = Some("test/fixtures/missing.toml".into());
        Settings::add_partial(partial);
        let shorthands = get_shorthands();
        assert!(!shorthands.is_empty());
    }
}
