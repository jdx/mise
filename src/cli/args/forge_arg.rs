use heck::ToKebabCase;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::dirs;
use crate::forge::{unalias_forge, ForgeType};
use crate::registry::REGISTRY;

#[derive(Clone, PartialOrd, Ord)]
pub struct ForgeArg {
    /// user-specified identifier, "node", "npm:prettier", "cargo:eza", "vfox:version-fox/vfox-nodejs"
    /// multiple ids may point to a single tool, e.g.: "node", "core:node" or "vfox:version-fox/vfox-nodejs"
    /// and "vfox:https://github.com/version-fox/vfox-nodejs"
    pub id: String,
    /// the name of the tool within the forge, e.g.: "node", "prettier", "eza", "vfox-nodejs"
    pub name: String,
    /// type of forge, "asdf", "cargo", "core", "npm", "vfox"
    pub forge_type: ForgeType,
    /// ~/.local/share/mise/cache/<THIS>
    pub cache_path: PathBuf,
    /// ~/.local/share/mise/installs/<THIS>
    pub installs_path: PathBuf,
    /// ~/.local/share/mise/downloads/<THIS>
    pub downloads_path: PathBuf,
}

impl<A: AsRef<str>> From<A> for ForgeArg {
    fn from(s: A) -> Self {
        let s = s.as_ref();
        if let Some(fa) = FORGE_MAP.get(s) {
            return fa.clone();
        }
        if let Some((forge_type, name)) = s.split_once(':') {
            if let Ok(forge_type) = forge_type.parse() {
                return Self::new(forge_type, name);
            }
        }
        Self::new(ForgeType::Asdf, s)
    }
}

impl ForgeArg {
    pub fn new(forge_type: ForgeType, name: &str) -> Self {
        let name = unalias_forge(name).to_string();
        let id = match forge_type {
            ForgeType::Asdf | ForgeType::Core => name.clone(),
            forge_type => format!("{forge_type}:{name}"),
        };
        let pathname = id.to_kebab_case();
        Self {
            name,
            forge_type,
            id,
            cache_path: dirs::CACHE.join(&pathname),
            installs_path: dirs::INSTALLS.join(&pathname),
            downloads_path: dirs::DOWNLOADS.join(&pathname),
        }
    }
}

impl Display for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl Debug for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"ForgeArg("{}")"#, self.id)
    }
}

impl PartialEq for ForgeArg {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ForgeArg {}

impl Hash for ForgeArg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

static FORGE_MAP: Lazy<HashMap<&'static str, ForgeArg>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .map(|(short, full)| {
            let (forge_type, name) = full.split_once(':').unwrap();
            let forge_type = forge_type.parse().unwrap();
            let fa = ForgeArg::new(forge_type, name);
            (*short, fa)
        })
        .collect()
});

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_str_eq};

    use super::*;

    #[test]
    fn test_forge_arg() {
        let t = |s: &str, id, name, t| {
            let fa: ForgeArg = s.into();
            assert_str_eq!(fa.id, id);
            assert_str_eq!(fa.name, name);
            assert_eq!(fa.forge_type, t);
        };
        let asdf = |s, id, name| t(s, id, name, ForgeType::Asdf);
        let cargo = |s, id, name| t(s, id, name, ForgeType::Cargo);
        // let core = |s, id, name| t(s, id, name, ForgeType::Core);
        let npm = |s, id, name| t(s, id, name, ForgeType::Npm);
        let vfox = |s, id, name| t(s, id, name, ForgeType::Vfox);

        asdf("asdf:poetry", "poetry", "poetry");
        asdf("poetry", "poetry", "poetry");
        asdf("", "", "");
        cargo("cargo:eza", "cargo:eza", "eza");
        // core("node", "node", "node");
        npm("npm:@antfu/ni", "npm:@antfu/ni", "@antfu/ni");
        npm("npm:prettier", "npm:prettier", "prettier");
        vfox(
            "vfox:version-fox/vfox-nodejs",
            "vfox:version-fox/vfox-nodejs",
            "version-fox/vfox-nodejs",
        );
    }

    #[test]
    fn test_forge_arg_pathname() {
        let t = |s: &str, expected| {
            let fa: ForgeArg = s.into();
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
        t("vfox:version-fox/vfox-nodejs", "vfox-nodejs");
        t("vfox:version-fox/nodejs", "vfox-nodejs");
    }
}
