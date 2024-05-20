use std::collections::BTreeMap;

use once_cell::sync::Lazy;

const _REGISTRY: &[(&str, &str)] = &[
    ("ubi", "cargo:ubi"),
    ("cargo-binstall", "cargo:cargo-binstall"),
    // ("elixir", "asdf:mise-plugins/mise-elixir"),
];

pub static REGISTRY: Lazy<BTreeMap<&str, String>> = Lazy::new(|| {
    // TODO: make sure core plugins can be overridden with this enabled
    // let core = CORE_PLUGINS
    //     .iter()
    //     .map(|p| (p.name(), format!("core:{}", p.name())));
    let registry = _REGISTRY.iter().map(|(k, v)| (*k, v.to_string()));
    registry.collect()
    // core.chain(registry).collect()
});
