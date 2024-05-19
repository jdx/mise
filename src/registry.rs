use std::collections::BTreeMap;

use once_cell::sync::Lazy;

use crate::plugins::core::CORE_PLUGINS;

const _REGISTRY: &[(&str, &str)] = &[
    ("ubi", "cargo:ubi"),
    // ("elixir", "asdf:mise-plugins/mise-elixir"),
];

pub static REGISTRY: Lazy<BTreeMap<&str, String>> = Lazy::new(|| {
    let core = CORE_PLUGINS
        .iter()
        .map(|p| (p.name(), format!("core:{}", p.name())));
    let registry = _REGISTRY.iter().map(|(k, v)| (*k, v.to_string()));
    core.chain(registry).collect()
});
