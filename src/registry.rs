use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::plugins::core::CORE_PLUGINS;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::iter::Iterator;
use url::Url;

// the registry is generated from registry.toml in the project root
include!(concat!(env!("OUT_DIR"), "/registry.rs"));

// a rust representation of registry.toml
pub static REGISTRY: Lazy<BTreeMap<&str, String>> = Lazy::new(|| {
    let backend_types = vec!["ubi", "vfox", "asdf", "cargo", "go", "npm", "pipx", "spm"]
        .into_iter()
        .filter(|b| cfg!(windows) || SETTINGS.vfox == Some(true) || *b != "ubi")
        .filter(|b| !SETTINGS.disable_backends.contains(&b.to_string()))
        .collect::<HashSet<_>>();

    _REGISTRY
        .iter()
        .filter(|(id, _)| !CORE_PLUGINS.contains_key(*id))
        .filter_map(|(id, fulls)| {
            fulls
                .iter()
                .find(|full| {
                    full.split(':')
                        .next()
                        .map_or(false, |b| backend_types.contains(b))
                })
                .map(|full| (*id, full.to_string()))
        })
        .collect()
});

pub static REGISTRY_BACKEND_MAP: Lazy<HashMap<&'static str, BackendArg>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .map(|(short, full)| (*short, BackendArg::new(short, full)))
        .collect()
});

pub static REGISTRY_VFOX: Lazy<BTreeMap<&str, &str>> = Lazy::new(|| {
    _REGISTRY
        .iter()
        .filter_map(|(id, fulls)| {
            let vfox_fulls = fulls
                .iter()
                .filter(|full| full.starts_with("vfox:"))
                .collect_vec();
            if vfox_fulls.is_empty() {
                None
            } else {
                Some((id, vfox_fulls[0]))
            }
        })
        .map(|(k, v)| (*k, *v))
        .collect()
});

pub static TRUSTED_SHORTHANDS: Lazy<BTreeSet<&'static str>> =
    Lazy::new(|| _TRUSTED_IDS.iter().copied().collect());

pub fn is_trusted_plugin(name: &str, remote: &str) -> bool {
    let normalized_url = normalize_remote(remote).unwrap_or("INVALID_URL".into());
    let is_shorthand = REGISTRY
        .get(name)
        .map(|full| full_to_url(full))
        .is_some_and(|s| normalize_remote(&s).unwrap_or_default() == normalized_url);
    let is_mise_url = normalized_url.starts_with("github.com/mise-plugins/");

    !is_shorthand || is_mise_url || TRUSTED_SHORTHANDS.contains(name)
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
