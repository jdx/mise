use aqua_registry::{AquaPackage, Result, decode_package_rkyv};

use crate::platform::Platform;

/// Metadata for the baked aqua registry snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AquaRegistryMetadata {
    pub repository: &'static str,
    pub tag: &'static str,
}

/// Baked canonical registry packages (compiled into the mise binary).
pub(crate) static AQUA_STANDARD_REGISTRY_FILES: phf::Map<&'static str, &'static [u8]> =
    include!(concat!(env!("OUT_DIR"), "/aqua_standard_registry_files.rs"));

/// Baked aqua registry snapshot metadata (compiled into the mise binary).
pub(crate) static AQUA_STANDARD_REGISTRY_METADATA: AquaRegistryMetadata = include!(concat!(
    env!("OUT_DIR"),
    "/aqua_standard_registry_metadata.rs"
));

/// Baked alias-to-canonical package ID map (compiled into the mise binary).
static AQUA_STANDARD_REGISTRY_ALIASES: phf::Map<&'static str, &'static str> = include!(concat!(
    env!("OUT_DIR"),
    "/aqua_standard_registry_aliases.rs"
));

#[derive(Debug)]
struct AquaSearchBackends {
    default: Option<&'static str>,
    overrides: &'static [AquaSearchBackendOverride],
}

#[derive(Debug)]
struct AquaSearchBackendOverride {
    goos: Option<&'static str>,
    goarch: Option<&'static str>,
    envs: &'static [&'static str],
    libc: Option<&'static str>,
    backend: Option<&'static str>,
}

#[derive(Debug)]
struct AquaSearchPlatform {
    os: String,
    arch: String,
    libc: Option<String>,
}

impl AquaSearchBackends {
    fn backend(&self, platform: &AquaSearchPlatform) -> Option<&'static str> {
        self.overrides
            .iter()
            .find(|package_override| package_override.matches(platform))
            .map_or(self.default, |package_override| package_override.backend)
    }
}

impl AquaSearchBackendOverride {
    fn matches(&self, platform: &AquaSearchPlatform) -> bool {
        self.goos.is_none_or(|goos| goos == platform.os.as_str())
            && self
                .goarch
                .is_none_or(|goarch| goarch == platform.arch.as_str())
            && (self.envs.is_empty()
                || self.envs.iter().any(|env| {
                    *env == "all"
                        || *env == platform.os.as_str()
                        || *env == platform.arch.as_str()
                        || env.split_once('/')
                            == Some((platform.os.as_str(), platform.arch.as_str()))
                }))
            && self
                .libc
                .is_none_or(|libc| Some(libc) == platform.libc.as_deref())
    }
}

/// Baked exceptions to the default `aqua:<id>` search backend.
/// An empty backend marks a package that cannot be represented by a runnable mise backend.
static AQUA_STANDARD_REGISTRY_SEARCH: phf::Map<&'static str, AquaSearchBackends> = include!(
    concat!(env!("OUT_DIR"), "/aqua_standard_registry_search.rs")
);

/// Returns searchable Aqua package IDs and any precomputed backend override.
/// Packages without a runnable mise backend on the current platform are omitted.
pub(crate) fn search_entries() -> impl Iterator<Item = (&'static str, Option<&'static str>)> {
    let current_platform = Platform::current();
    let os = match current_platform.os.as_str() {
        "macos" => "darwin",
        other => other,
    }
    .to_string();
    let arch = match current_platform.arch.as_str() {
        "x64" => "amd64",
        other => other,
    }
    .to_string();
    let libc = (os == "linux").then(|| current_platform.libc().unwrap_or("gnu").to_string());
    let platform = AquaSearchPlatform { os, arch, libc };

    AQUA_STANDARD_REGISTRY_FILES.keys().filter_map(move |id| {
        let backend = AQUA_STANDARD_REGISTRY_SEARCH
            .get(id)
            .and_then(|backends| backends.backend(&platform));
        (backend != Some("")).then_some((*id, backend))
    })
}

pub(crate) fn package(package_id: &str) -> Option<Result<AquaPackage>> {
    baked_registry_file(package_id).map(|content| decode_package_rkyv(package_id, content))
}

fn baked_registry_file(package_id: &str) -> Option<&'static [u8]> {
    if let Some(content) = AQUA_STANDARD_REGISTRY_FILES.get(package_id) {
        return Some(*content);
    }

    AQUA_STANDARD_REGISTRY_ALIASES
        .get(package_id)
        .and_then(|canonical| AQUA_STANDARD_REGISTRY_FILES.get(*canonical))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baked_registry_package_lookup() {
        let package = package("01mf02/jaq").unwrap().unwrap();

        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
    }

    #[test]
    fn test_baked_registry_path_only_package_lookup() {
        let package = package("golang.org/x/perf/cmd/benchstat").unwrap().unwrap();

        assert_eq!(
            package.path.as_deref(),
            Some("golang.org/x/perf/cmd/benchstat")
        );
    }

    #[test]
    fn test_baked_registry_search_entries() {
        let entries = search_entries().collect::<std::collections::HashMap<_, _>>();

        assert_eq!(entries.get("crates.io/broot"), Some(&Some("cargo:broot")));
        assert_eq!(
            entries.get("golang.org/x/perf/cmd/benchstat"),
            Some(&Some("go:golang.org/x/perf/cmd/benchstat"))
        );
        assert_eq!(entries.get("goccy/go-yaml/ycat"), None);
        assert_eq!(
            entries.get("Azure/mapotf"),
            Some(&Some("go:github.com/Azure/mapotf"))
        );
        assert_eq!(
            entries.get("vburenin/ifacemaker"),
            Some(&Some("go:github.com/vburenin/ifacemaker"))
        );
        assert_eq!(entries.get("golang/tools/gorename"), None);
        assert_eq!(entries.get("cli/cli"), Some(&None));
    }

    #[test]
    fn test_baked_registry_search_platform_overrides() {
        let darwin = AquaSearchPlatform {
            os: "darwin".to_string(),
            arch: "arm64".to_string(),
            libc: None,
        };
        let linux = AquaSearchPlatform {
            os: "linux".to_string(),
            arch: "amd64".to_string(),
            libc: Some("gnu".to_string()),
        };

        let rhit = AQUA_STANDARD_REGISTRY_SEARCH.get("Canop/rhit").unwrap();
        assert_eq!(rhit.backend(&darwin), Some("cargo:rhit"));
        assert_eq!(rhit.backend(&linux), None);

        let dockfmt = AQUA_STANDARD_REGISTRY_SEARCH
            .get("jessfraz/dockfmt")
            .unwrap();
        assert_eq!(
            dockfmt.backend(&darwin),
            Some("go:github.com/jessfraz/dockfmt")
        );
        assert_eq!(dockfmt.backend(&linux), None);
    }

    #[test]
    fn test_search_backend_libc_override_selection() {
        let backends = AquaSearchBackends {
            default: None,
            overrides: &[AquaSearchBackendOverride {
                goos: Some("linux"),
                goarch: None,
                envs: &[],
                libc: Some("musl"),
                backend: Some("cargo:example"),
            }],
        };
        let gnu = AquaSearchPlatform {
            os: "linux".to_string(),
            arch: "amd64".to_string(),
            libc: Some("gnu".to_string()),
        };
        let musl = AquaSearchPlatform {
            os: "linux".to_string(),
            arch: "amd64".to_string(),
            libc: Some("musl".to_string()),
        };

        assert_eq!(backends.backend(&gnu), None);
        assert_eq!(backends.backend(&musl), Some("cargo:example"));
    }

    #[test]
    fn test_baked_registry_metadata() {
        assert_eq!(
            AQUA_STANDARD_REGISTRY_METADATA.repository,
            "aquaproj/aqua-registry"
        );
        assert!(!AQUA_STANDARD_REGISTRY_METADATA.tag.is_empty());
        assert!(
            AQUA_STANDARD_REGISTRY_METADATA.tag.starts_with('v')
                || AQUA_STANDARD_REGISTRY_METADATA.tag.len() == 40
                    && AQUA_STANDARD_REGISTRY_METADATA
                        .tag
                        .chars()
                        .all(|c| c.is_ascii_hexdigit())
        );
    }

    #[test]
    fn test_baked_registry_alias_lookup() {
        let alias = "elijah-potter/harper/harper-ls";

        assert!(!AQUA_STANDARD_REGISTRY_FILES.contains_key(alias));
        assert_eq!(
            AQUA_STANDARD_REGISTRY_ALIASES.get(alias).copied(),
            Some("Automattic/harper/harper-ls")
        );

        let alias_package = package(alias).unwrap().unwrap();
        let canonical_package = package("Automattic/harper/harper-ls").unwrap().unwrap();

        assert_eq!(
            alias_package.name.as_deref(),
            Some("Automattic/harper/harper-ls")
        );
        assert_eq!(
            alias_package.name.as_deref(),
            canonical_package.name.as_deref()
        );
        assert_eq!(alias_package.repo_owner, canonical_package.repo_owner);
        assert_eq!(alias_package.repo_name, canonical_package.repo_name);
    }

    #[test]
    fn test_baked_registry_numeric_replacement_keys() {
        let package = package("sharkdp/hyperfine").unwrap().unwrap();

        assert_eq!(package.replacements.get("386"), Some(&"i686".to_string()));
    }
}
