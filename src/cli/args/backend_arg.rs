use heck::ToKebabCase;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::backend::{unalias_backend, BackendType};
use crate::dirs;
use crate::registry::REGISTRY;

#[derive(Clone, PartialOrd, Ord)]
pub struct BackendArg {
    /// user-specified identifier, "node", "npm:prettier", "cargo:eza", "vfox:version-fox/vfox-nodejs"
    /// multiple ids may point to a single tool, e.g.: "node", "core:node" or "vfox:version-fox/vfox-nodejs"
    /// and "vfox:https://github.com/version-fox/vfox-nodejs"
    pub id: String,
    /// the name of the tool within the backend, e.g.: "node", "prettier", "eza", "vfox-nodejs"
    pub name: String,
    /// type of backend, "asdf", "cargo", "core", "npm", "vfox"
    pub backend_type: BackendType,
    /// ~/.local/share/mise/cache/<THIS>
    pub cache_path: PathBuf,
    /// ~/.local/share/mise/installs/<THIS>
    pub installs_path: PathBuf,
    /// ~/.local/share/mise/downloads/<THIS>
    pub downloads_path: PathBuf,
}

impl<A: AsRef<str>> From<A> for BackendArg {
    fn from(s: A) -> Self {
        let s = s.as_ref();
        if let Some(fa) = FORGE_MAP.get(s) {
            return fa.clone();
        }
        if let Some((backend_type, name)) = s.split_once(':') {
            if let Ok(backend_type) = backend_type.parse() {
                return Self::new(backend_type, name);
            }
        }
        Self::new(BackendType::Asdf, s)
    }
}

impl BackendArg {
    pub fn new(backend_type: BackendType, name: &str) -> Self {
        let name = unalias_backend(name).to_string();
        let id = match backend_type {
            BackendType::Asdf | BackendType::Core => name.clone(),
            backend_type => format!("{backend_type}:{name}"),
        };
        let pathname = id.to_kebab_case();
        Self {
            name,
            backend_type,
            id,
            cache_path: dirs::CACHE.join(&pathname),
            installs_path: dirs::INSTALLS.join(&pathname),
            downloads_path: dirs::DOWNLOADS.join(&pathname),
        }
    }
}

impl Display for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl Debug for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"BackendArg("{}")"#, self.id)
    }
}

impl PartialEq for BackendArg {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for BackendArg {}

impl Hash for BackendArg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

static FORGE_MAP: Lazy<HashMap<&'static str, BackendArg>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .map(|(short, full)| {
            let (backend_type, name) = full.split_once(':').unwrap();
            let backend_type = backend_type.parse().unwrap();
            let fa = BackendArg::new(backend_type, name);
            (*short, fa)
        })
        .collect()
});

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_str_eq};

    use super::*;

    #[test]
    fn test_backend_arg() {
        let t = |s: &str, id, name, t| {
            let fa: BackendArg = s.into();
            assert_str_eq!(fa.id, id);
            assert_str_eq!(fa.name, name);
            assert_eq!(fa.backend_type, t);
        };
        let asdf = |s, id, name| t(s, id, name, BackendType::Asdf);
        let cargo = |s, id, name| t(s, id, name, BackendType::Cargo);
        // let core = |s, id, name| t(s, id, name, BackendType::Core);
        let npm = |s, id, name| t(s, id, name, BackendType::Npm);

        asdf("asdf:poetry", "poetry", "poetry");
        asdf("poetry", "poetry", "poetry");
        asdf("", "", "");
        cargo("cargo:eza", "cargo:eza", "eza");
        // core("node", "node", "node");
        npm("npm:@antfu/ni", "npm:@antfu/ni", "@antfu/ni");
        npm("npm:prettier", "npm:prettier", "prettier");
    }

    #[test]
    fn test_backend_arg_pathname() {
        let t = |s: &str, expected| {
            let fa: BackendArg = s.into();
            let actual = fa.installs_path.to_string_lossy();
            let expected = dirs::INSTALLS.join(expected);
            assert_str_eq!(actual, expected.to_string_lossy());
        };
        t("asdf:node", "node");
        t("node", "node");
        t("", "");
        t("cargo:eza", "cargo-eza");
        t("npm:@antfu/ni", "npm-antfu-ni");
        t("npm:prettier", "npm-prettier");
    }
}
