use std::collections::HashMap;
use std::path::PathBuf;

use eyre::Result;
use itertools::Itertools;
use toml::Table;

use crate::config::Settings;
use crate::registry::REGISTRY;
use crate::{dirs, file};

pub type Shorthands = HashMap<String, Vec<String>>;

pub fn get_shorthands(settings: &Settings) -> Shorthands {
    let mut shorthands = HashMap::new();
    if !settings.disable_default_registry {
        shorthands.extend(
            REGISTRY
                .iter()
                .map(|(id, rt)| {
                    (
                        id.to_string(),
                        rt.backends()
                            .iter()
                            .filter(|f| f.starts_with("asdf:") || f.starts_with("vfox:"))
                            .map(|f| f.to_string())
                            .collect_vec(),
                    )
                })
                .filter(|(_, fulls)| !fulls.is_empty()),
        );
    };
    if let Some(f) = &settings.shorthands_file {
        match parse_shorthands_file(f.clone()) {
            Ok(custom) => {
                shorthands.extend(custom);
            }
            Err(err) => {
                warn!("Failed to read shorthands file: {} {:#}", &f.display(), err);
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
            shorthands.insert(k, vec![v.to_string()]);
        }
    }
    Ok(shorthands)
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use pretty_assertions::assert_str_eq;
    use test_log::test;

    use crate::test::reset;

    use super::*;

    #[test]
    fn test_get_shorthands() {
        reset();
        Settings::reset(None);
        let mut settings = Settings::get().deref().clone();
        settings.shorthands_file = Some("../fixtures/shorthands.toml".into());
        let shorthands = get_shorthands(&settings);
        assert_str_eq!(shorthands["elixir"][0], "asdf:mise-plugins/mise-elixir");
        assert_str_eq!(shorthands["node"][0], "https://node");
        assert_str_eq!(shorthands["xxxxxx"][0], "https://xxxxxx");
    }

    #[test]
    fn test_get_shorthands_missing_file() {
        reset();
        Settings::reset(None);
        let mut settings = Settings::get().deref().clone();
        settings.shorthands_file = Some("test/fixtures/missing.toml".into());
        let shorthands = get_shorthands(&settings);
        assert!(!shorthands.is_empty());
    }
}
