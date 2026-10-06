//! mise secrets: spawn-time env sources (`[secrets.*]`). Not `crate::system::secrets`
//! (`[bootstrap.secrets]`).
//!
//! A project names a secrets source in its own `mise.toml`. This module only knows how to
//! find that source and list the names it can provide; it never resolves a value.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::file::display_path;

mod config;
mod fnox;
mod name;
mod source;

pub use name::SecretName;
pub use source::{Catalog, CatalogEntry, InjectMode, KeyKind};

use source::SecretSource;

/// Where the project's secrets come from.
pub struct SourceInfo {
    pub kind: &'static str,
    pub root: PathBuf,
    pub declared_in: Vec<PathBuf>,
    pub profile: Option<String>,
    pub tool_path: PathBuf,
}

pub struct Inventory {
    pub source: Option<SourceInfo>,
    pub catalog: Option<Arc<Catalog>>,
    pub ignored: Vec<PathBuf>,
}

/// Used by `mise secrets ls`. Gated (safe mode, trust). Spawns
/// `fnox ... env --json --describe` once.
pub async fn inventory(config: &Arc<Config>) -> eyre::Result<Inventory> {
    let selection = config::select_for_cwd(config)?;
    let Some(selected) = selection.source else {
        return Ok(Inventory {
            source: None,
            catalog: None,
            ignored: selection.ignored,
        });
    };
    let source = fnox::FnoxSource::new(config, &selected).await?;
    debug!("describing secrets from {}", source.label());
    let catalog = source.describe().await?;
    let id = source.id();
    Ok(Inventory {
        source: Some(SourceInfo {
            kind: id.kind,
            root: id.root.clone(),
            declared_in: selected.declared_in,
            profile: id.profile.clone(),
            tool_path: source.tool_path().to_path_buf(),
        }),
        catalog: Some(Arc::new(catalog)),
        ignored: selection.ignored,
    })
}

/// The deprecated plugin, by name (`mise-env-fnox = "./plugins/secrets"`) or by the repo name
/// ending its source (`https://github.com/x/mise-env-fnox.git`). A source that merely contains
/// the text, such as `mise-env-fnox-fork-tools`, is some other plugin.
fn is_env_fnox_plugin(name: &str, source: &str) -> bool {
    const PLUGIN: &str = "mise-env-fnox";
    let last = source
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or_default();
    name == PLUGIN || last.strip_suffix(".git").unwrap_or(last) == PLUGIN
}

/// Used by `mise doctor`. Never spawns fnox, never errors.
pub async fn doctor_warnings(config: &Arc<Config>) -> Vec<String> {
    let mut warnings = vec![];
    match config::select_for_cwd_ungated(config) {
        Ok(selection) => {
            for file in &selection.ignored {
                warnings.push(format!(
                    "[secrets.fnox] in {} is ignored: secrets sources are allowed only in project config",
                    display_path(file)
                ));
            }
            if let Some(selected) = &selection.source
                && fnox::find_binary(config).await.is_none()
            {
                warnings.push(format!(
                    "[secrets.fnox] is configured in {} but the fnox CLI was not found; add it with: mise use fnox",
                    display_path(&selected.declared_in[0])
                ));
            }
        }
        Err(err) => warnings.push(format!("{err:#}")),
    }
    for (path, cf) in config.config_files.iter() {
        if let Ok(plugins) = cf.plugins()
            && plugins
                .iter()
                .any(|(name, src)| is_env_fnox_plugin(name, src))
        {
            warnings.push(format!(
                "the mise-env-fnox plugin is deprecated (configured in {}); see https://mise.jdx.dev/environments/secrets/fnox.html#migrating",
                display_path(path)
            ));
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::is_env_fnox_plugin;

    #[test]
    fn env_fnox_plugin_matches_by_name_or_repo() {
        assert!(is_env_fnox_plugin("mise-env-fnox", "./plugins/secrets"));
        assert!(is_env_fnox_plugin(
            "fnox-env",
            "https://github.com/jdx/mise-env-fnox"
        ));
        assert!(is_env_fnox_plugin(
            "fnox-env",
            "https://github.com/jdx/mise-env-fnox.git"
        ));
        assert!(is_env_fnox_plugin("x", "git@github.com:jdx/mise-env-fnox/"));
    }

    #[test]
    fn env_fnox_plugin_ignores_lookalikes() {
        assert!(!is_env_fnox_plugin(
            "other",
            "https://github.com/a/mise-env-fnox-tools"
        ));
        assert!(!is_env_fnox_plugin("other", "./mise-env-fnox/other-plugin"));
        assert!(!is_env_fnox_plugin(
            "other",
            "https://github.com/a/not-mise-env-fnox"
        ));
    }
}
