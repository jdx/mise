use crate::config::Settings;
use crate::plugins::core::CORE_PLUGINS;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::collections::BTreeMap;

const _REGISTRY: &[(&str, &str)] = &[
    ("ubi", "cargo:ubi-cli"),
    ("cargo-binstall", "cargo:cargo-binstall"),
    // ("elixir", "asdf:mise-plugins/mise-elixir"),
];

const _REGISTRY_VFOX: &[(&str, &str)] = &[
    ("bun", "vfox:ahai-code/vfox-bun"),
    ("cargo-binstall", "cargo:cargo-binstall"),
    ("clang", "vfox:version-fox/vfox-clang"),
    ("cmake", "vfox:version-fox/vfox-cmake"),
    ("crystal", "vfox:yanecc/vfox-crystal"),
    ("dart", "vfox:version-fox/vfox-dart"),
    ("deno", "vfox:version-fox/vfox-deno"),
    ("dotnet", "vfox:version-fox/vfox-dotnet"),
    ("elixir", "vfox:version-fox/vfox-elixir"),
    ("erlang", "vfox:version-fox/vfox-erlang"),
    ("etcd", "vfox:version-fox/vfox-etcd"),
    ("flutter", "vfox:version-fox/vfox-flutter"),
    ("golang", "vfox:version-fox/vfox-golang"),
    ("gradle", "vfox:version-fox/vfox-gradle"),
    ("groovy", "vfox:version-fox/vfox-groovy"),
    ("java", "vfox:version-fox/vfox-java"),
    ("julia", "vfox:ahai-code/vfox-julia"),
    ("kotlin", "vfox:version-fox/vfox-kotlin"),
    ("kubectl", "vfox:ahai-code/vfox-kubectl"),
    ("maven", "vfox:version-fox/vfox-maven"),
    ("mongo", "vfox:yeshan333/vfox-mongo"),
    ("php", "vfox:version-fox/vfox-php"),
    ("protobuf", "vfox:ahai-code/vfox-protobuf"),
    // ("ruby", "vfox:yanecc/vfox-ruby"),
    ("scala", "vfox:version-fox/vfox-scala"),
    ("terraform", "vfox:enochchau/vfox-terraform"),
    ("ubi", "cargo:ubi-cli"),
    ("vlang", "vfox:ahai-code/vfox-vlang"),
    ("zig", "vfox:version-fox/vfox-zig"),
];

pub static REGISTRY: Lazy<BTreeMap<&str, String>> = Lazy::new(|| {
    let settings = Settings::get();

    let registry = if cfg!(windows) || settings.vfox {
        _REGISTRY.iter().chain(_REGISTRY_VFOX.iter()).collect_vec()
    } else {
        _REGISTRY.iter().collect_vec()
    };

    registry
        .into_iter()
        .filter(|(id, _)| !CORE_PLUGINS.contains_key(*id))
        .map(|(k, v)| (*k, v.to_string()))
        .collect()
});

pub static REGISTRY_VFOX: Lazy<BTreeMap<&str, &str>> =
    Lazy::new(|| _REGISTRY_VFOX.iter().map(|(k, v)| (*k, *v)).collect());
