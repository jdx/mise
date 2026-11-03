use super::*;

fn default_str(value: &str) -> Option<String> {
    Some(value.to_string())
}

fn first_registry_package(yml: &str) -> AquaPackage {
    RegistryYaml::from_yaml(yml)
        .unwrap()
        .packages
        .into_iter()
        .next()
        .unwrap()
        .package
}
mod asset;
mod file;
mod overrides;
mod registry;
mod signature;
mod vars;
mod verification;
mod version;
