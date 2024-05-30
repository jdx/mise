use std::collections::BTreeMap;

use once_cell::sync::Lazy;

#[cfg(unix)]
const _REGISTRY: &[(&str, &str)] = &[
    ("ubi", "cargo:ubi"),
    ("cargo-binstall", "cargo:cargo-binstall"),
    // ("elixir", "asdf:mise-plugins/mise-elixir"),
];

#[cfg(windows)]
const _REGISTRY: &[(&str, &str)] = &[
    ("bun", "vfox:ahai-code/vfox-bun"),
    ("cargo-binstall", "cargo:cargo-binstall"),
    ("cmake", "vfox:version-fox/vfox-cmake"),
    ("crystal", "vfox:yanecc/vfox-crystal"),
    ("dart", "vfox:version-fox/vfox-dart"),
    ("deno", "vfox:version-fox/vfox-deno"),
    ("dotnet", "vfox:version-fox/vfox-dotnet"),
    ("elixir", "vfox:version-fox/vfox-elixir"),
    ("erlang", "vfox:version-fox/vfox-erlang"),
    ("etcd", "vfox:version-fox/vfox-etcd"),
    ("flutter", "vfox:version-fox/vfox-flut"),
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
    ("python", "vfox:version-fox/vfox-python"),
    ("ruby", "vfox:yanecc/vfox-ruby"),
    ("scala", "vfox:version-fox/vfox-scala"),
    ("terraform", "vfox:enochchau/vfox-terraform"),
    ("ubi", "cargo:ubi"),
    ("vlang", "vfox:ahai-code/vfox-vlang"),
    ("zig", "vfox:version-fox/vfox-zig"),
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
