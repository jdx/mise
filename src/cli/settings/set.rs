use eyre::{Result, bail, eyre};
use toml_edit::DocumentMut;

use crate::config::settings::{SETTINGS_META, SettingsFile, SettingsType, parse_url_replacements};
use crate::toml::dedup_toml_array;
use crate::{config, duration, file};

/// Add/update a setting
///
/// This modifies the contents of ~/.config/mise/config.toml by default.
/// With `--local`, modifies the local config file instead.
/// See https://mise.jdx.dev/configuration.html#target-file-for-write-operations
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsSet {
    /// The setting to set
    #[clap()]
    pub setting: String,
    /// The value to set (optional if provided as KEY=VALUE)
    pub value: Option<String>,
    /// Use the local config file instead of the global one
    #[clap(long, short)]
    pub local: bool,
}

impl SettingsSet {
    pub fn run(self) -> Result<()> {
        match self.value {
            Some(value) => set(&self.setting, &value, false, self.local),
            None => {
                let (key, value) = self.setting.split_once('=').ok_or_else(|| {
                    eyre!(
                        "Usage: mise settings set <KEY>=<VALUE> or mise settings set <KEY> <VALUE>"
                    )
                })?;
                set(key, value, false, self.local)
            }
        }
    }
}

pub fn set(mut key: &str, value: &str, add: bool, local: bool) -> Result<()> {
    let meta = match SETTINGS_META.get(key) {
        Some(meta) => meta,
        None => {
            bail!("Unknown setting: {}", key);
        }
    };

    // Writing it would succeed and then be ignored, and `mise settings get` would report the
    // stored value as if it were live. See https://github.com/jdx/mise/discussions/5791.
    // `mise settings unset` is deliberately still allowed, so anyone who already has one of
    // these in a config can remove it.
    if meta.env_only {
        bail!(
            "{key} cannot be set in a config file: mise reads it before config files load. Use the {} environment variable instead.",
            meta.env.unwrap_or("matching MISE_*")
        );
    }

    let value = match meta.type_ {
        SettingsType::Bool => parse_bool(value)?,
        SettingsType::Integer => parse_i64(value)?,
        SettingsType::Duration => parse_duration(value)?,
        SettingsType::Url | SettingsType::Path | SettingsType::String => value.into(),
        SettingsType::ListString => parse_list_by_comma(value)?,
        SettingsType::ListPath => parse_list_by_os_path_separator(value)?,
        SettingsType::SetString => parse_set_by_comma(value)?,
        SettingsType::IndexMap => parse_indexmap_by_json(value)?,
        SettingsType::BoolOrString => parse_bool(value).unwrap_or_else(|_| value.into()),
    };

    let path = if local {
        config::local_toml_config_path()
    } else {
        config::global_config_path()
    };
    file::create_dir_all(path.parent().unwrap())?;
    let raw = file::read_to_string(&path).unwrap_or_default();
    let mut config: DocumentMut = raw.parse()?;
    if !config.contains_key("settings") {
        let mut settings = toml_edit::Table::new();
        settings.set_implicit(true);
        config["settings"] = toml_edit::Item::Table(settings);
    }
    if let Some(settings) = config["settings"].as_table_like_mut() {
        let settings: &mut dyn toml_edit::TableLike =
            if let Some((parent_key, child_key)) = key.split_once('.') {
                key = child_key;
                settings
                    .entry(parent_key)
                    .or_insert({
                        let mut t = toml_edit::Table::new();
                        t.set_implicit(true);
                        toml_edit::Item::Table(t)
                    })
                    .as_table_like_mut()
                    .ok_or_else(|| eyre!("Setting [{parent_key}] is not a table"))?
            } else {
                settings
            };

        let value = match settings.get(key) {
            Some(current) if add && current.as_array().is_some() => {
                let array = current.as_array().unwrap();
                let mut new_array = array.clone();
                new_array.extend(value.as_array().unwrap().iter().cloned());
                match meta.type_ {
                    SettingsType::SetString => dedup_toml_array(&new_array).into(),
                    _ => new_array.into(),
                }
            }
            _ => value,
        };
        settings.insert(key, value.into());

        // validate
        let _: SettingsFile = toml::from_str(&config.to_string())?;

        file::write(path, config.to_string())?;
    }
    Ok(())
}

fn parse_list_by_comma(value: &str) -> Result<toml_edit::Value> {
    if value.is_empty() || value == "[]" {
        return Ok(toml_edit::Array::new().into());
    }
    Ok(value.split(',').map(|s| s.trim().to_string()).collect())
}

/// Split a path list the way the host does: `;` on Windows, `:` on Unix.
///
/// Splitting on `:` unconditionally made `mise settings set trusted_config_paths 'C:\proj'`
/// store `["C", "\proj"]`. A Windows drive letter's colon is not a separator, so even a lone
/// path with no list separator in it came apart, leaving no way to write one from the CLI.
/// #9058 gave the environment-variable route the OS separator; this is the CLI route it missed.
///
/// Deliberately the same `std::env::split_paths` that `list_by_os_path_separator` in
/// `config::settings` uses for `parse_env = "list_by_os_path_separator"`, so the two entry
/// points to the same settings cannot drift apart again. On Windows it also strips surrounding
/// double quotes, matching how the OS itself reads a path list.
fn parse_list_by_os_path_separator(value: &str) -> Result<toml_edit::Value> {
    if value.is_empty() || value == "[]" {
        return Ok(toml_edit::Array::new().into());
    }
    // `std::env` rather than `crate::env`, which is what `env` resolves to elsewhere in mise.
    Ok(std::env::split_paths(value)
        .map(|p| p.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

fn parse_set_by_comma(value: &str) -> Result<toml_edit::Value> {
    let value = value.trim();
    if value.is_empty() || value == "[]" {
        return Ok(toml_edit::Array::new().into());
    }
    let array: toml_edit::Array = value
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    Ok(dedup_toml_array(&array).into())
}

fn parse_bool(value: &str) -> Result<toml_edit::Value> {
    match value.to_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Ok(true.into()),
        "0" | "false" | "no" | "n" => Ok(false.into()),
        _ => Err(eyre!("{} must be true or false", value)),
    }
}

fn parse_i64(value: &str) -> Result<toml_edit::Value> {
    match value.parse::<i64>() {
        Ok(value) => Ok(value.into()),
        Err(_) => Err(eyre!("{} must be a number", value)),
    }
}

fn parse_duration(value: &str) -> Result<toml_edit::Value> {
    duration::parse_duration(value)?;
    Ok(value.into())
}

fn parse_indexmap_by_json(value: &str) -> Result<toml_edit::Value> {
    let index_map = parse_url_replacements(value)
        .map_err(|e| eyre!("Failed to parse JSON for IndexMap: {}", e))?;
    Ok(toml_edit::Value::InlineTable({
        let mut table = toml_edit::InlineTable::new();
        for (k, v) in index_map {
            table.insert(&k, toml_edit::Value::String(toml_edit::Formatted::new(v)));
        }
        table
    }))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings idiomatic_version_file=true</bold>
"#
);

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn entries(value: toml_edit::Value) -> Vec<String> {
        value
            .as_array()
            .expect("a path list parses to an array")
            .iter()
            .map(|v| v.as_str().expect("array of strings").to_string())
            .collect()
    }

    fn parse(value: &str) -> Vec<String> {
        entries(parse_list_by_os_path_separator(value).unwrap())
    }

    fn expect(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// The defect this function exists for: a drive letter's colon is not a separator, so a
    /// path list splits on `;` here. Asserted only on Windows -- `\` and `;` are ordinary
    /// filename characters on unix, where `C:\a` really is one relative path named `C:\a`.
    #[cfg(windows)]
    #[test]
    fn windows_drive_letter_is_not_a_separator() {
        // The sharpest form: no list separator anywhere, and it still came apart before.
        assert_eq!(parse(r"C:\a"), expect(&[r"C:\a"]));
        assert_eq!(
            parse(r"C:\a\one;C:\b\two"),
            expect(&[r"C:\a\one", r"C:\b\two"])
        );
    }

    /// `split_paths` unquotes on Windows, the way the OS reads a path list -- which is the only
    /// way to spell a path containing the separator. Asserted so that swapping the split for a
    /// hand-rolled one would fail here rather than quietly drop the quotes into the config.
    #[cfg(windows)]
    #[test]
    fn windows_quoted_paths_are_unquoted() {
        assert_eq!(
            parse(r#""C:\Program Files\one";"C:\Program Files\two""#),
            expect(&[r"C:\Program Files\one", r"C:\Program Files\two"])
        );
        // Quoting is per entry, not all-or-nothing.
        assert_eq!(parse(r#""C:\a b";C:\c"#), expect(&[r"C:\a b", r"C:\c"]));
    }

    /// Unix keeps the colon it always had. This is the control for the change above: the
    /// separator moved with the platform rather than everywhere.
    #[cfg(not(windows))]
    #[test]
    fn unix_still_splits_on_colon() {
        assert_eq!(parse("/a/one:/b/two"), expect(&["/a/one", "/b/two"]));
        assert_eq!(parse("/a"), expect(&["/a"]));
    }

    #[test]
    fn empty_input_parses_to_an_empty_array() {
        assert!(parse("").is_empty());
        assert!(parse("[]").is_empty());
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        #[cfg(windows)]
        let (input, expected) = (r"  C:\a ; C:\b  ", expect(&[r"C:\a", r"C:\b"]));
        #[cfg(not(windows))]
        let (input, expected) = ("  /a : /b  ", expect(&["/a", "/b"]));
        assert_eq!(parse(input), expected);
    }

    /// An empty segment is not a path. `list_by_os_path_separator` and `env::split_colon_list`
    /// both drop them; writing one into the config would only be dropped again on read.
    #[test]
    fn empty_segments_are_dropped() {
        #[cfg(windows)]
        let (input, expected) = (r";C:\a;;C:\b;", expect(&[r"C:\a", r"C:\b"]));
        #[cfg(not(windows))]
        let (input, expected) = (":/a::/b:", expect(&["/a", "/b"]));
        assert_eq!(parse(input), expected);
    }
}
