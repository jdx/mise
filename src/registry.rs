use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::plugins::core::CORE_PLUGINS;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::Iterator;

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
