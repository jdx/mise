use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::Iterator;
use strum::IntoEnumIterator;
use url::Url;

// the registry is generated from registry.toml in the project root
include!(concat!(env!("OUT_DIR"), "/registry.rs"));

#[derive(Debug, Clone)]
pub struct RegistryTool {
    pub backends: Vec<&'static str>,
    pub aliases: &'static [&'static str],
}

// a rust representation of registry.toml
pub static REGISTRY: Lazy<BTreeMap<&str, RegistryTool>> = Lazy::new(|| {
    let mut backend_types = BackendType::iter()
        .map(|b| b.to_string())
        .collect::<HashSet<_>>();
    for backend in &SETTINGS.disable_backends {
        backend_types.remove(backend);
    }
    if cfg!(windows) {
        backend_types.remove("asdf");
    }
    if cfg!(unix) && !SETTINGS.experimental {
        backend_types.remove("aqua");
    }

    let mut registry: BTreeMap<&str, RegistryTool> = _REGISTRY
        .iter()
        .map(|(short, backends, aliases)| {
            let backends = backends
                .iter()
                .filter(|full| {
                    full.split(':')
                        .next()
                        .map_or(false, |b| backend_types.contains(b))
                })
                .copied()
                .collect();
            let tool = RegistryTool { backends, aliases };
            (*short, tool)
        })
        .filter(|(_, tool)| !tool.backends.is_empty())
        .collect();

    let aliased = registry
        .values()
        .flat_map(|tool| tool.aliases.iter().map(move |alias| (*alias, tool.clone())))
        .collect_vec();

    registry.extend(aliased);

    registry
});

pub static REGISTRY_BACKEND_MAP: Lazy<HashMap<&'static str, Vec<BackendArg>>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .map(|(short, tool)| {
            (
                *short,
                tool.backends
                    .iter()
                    .map(|f| BackendArg::new(short.to_string(), Some(f.to_string())))
                    .collect(),
            )
        })
        .collect()
});

pub fn is_trusted_plugin(name: &str, remote: &str) -> bool {
    let normalized_url = normalize_remote(remote).unwrap_or("INVALID_URL".into());
    let is_shorthand = REGISTRY
        .get(name)
        .and_then(|tool| tool.backends.first())
        .map(|full| full_to_url(full))
        .is_some_and(|s| normalize_remote(&s).unwrap_or_default() == normalized_url);
    let is_mise_url = normalized_url.starts_with("github.com/mise-plugins/");

    !is_shorthand || is_mise_url
}

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}

pub fn full_to_url(full: &str) -> String {
    let (_backend, url) = full.split_once(':').unwrap_or(("", full));
    if url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://github.com/{url}.git")
    }
}
