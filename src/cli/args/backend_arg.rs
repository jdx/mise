use crate::backend::backend_meta::BackendMeta;
use crate::backend::{unalias_backend, BackendType};
use crate::dirs;
use crate::plugins::{PluginType, PLUGIN_NAMES_TO_TYPE};
use crate::registry::REGISTRY_BACKEND_MAP;
use crate::toolset::{parse_tool_options, ToolVersionOptions};
use heck::ToKebabCase;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::PathBuf;
use xx::regex;

#[derive(Clone, PartialOrd, Ord)]
pub struct BackendArg {
    /// short or full identifier (what the user specified), "node", "prettier", "npm:prettier", "cargo:eza"
    pub short: String,
    /// full identifier, "core:node", "npm:prettier", "cargo:eza", "vfox:version-fox/vfox-nodejs"
    pub full: String,
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
    pub opts: Option<ToolVersionOptions>,
}

impl<A: AsRef<str>> From<A> for BackendArg {
    fn from(s: A) -> Self {
        let s = s.as_ref();
        if let Some(fa) = REGISTRY_BACKEND_MAP.get(s).and_then(|rbm| rbm.first()) {
            fa.clone()
        } else {
            Self::new(s, s)
        }
    }
}

impl From<BackendMeta> for BackendArg {
    fn from(meta: BackendMeta) -> Self {
        meta.short.into()
    }
}

impl BackendArg {
    pub fn new(short: &str, full: &str) -> Self {
        let short = unalias_backend(short).to_string();
        let short = regex!(r#"\[.+\]$"#).replace_all(&short, "").to_string();

        let (backend, mut name) = full.split_once(':').unwrap_or(("", full));

        let mut opts = None;
        if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(name) {
            name = c.get(1).unwrap().as_str();
            opts = Some(parse_tool_options(c.get(2).unwrap().as_str()));
        }

        let backend = unalias_backend(backend);

        let backend_type = if let Some(pt) = PLUGIN_NAMES_TO_TYPE.get(&short) {
            match pt {
                PluginType::Asdf => BackendType::Asdf,
                PluginType::Vfox => BackendType::Vfox,
                PluginType::Core => BackendType::Core,
            }
        } else {
            backend.parse().unwrap_or(BackendType::Asdf)
        };
        let full = match backend_type {
            BackendType::Asdf | BackendType::Core => short.clone(),
            backend_type => format!("{backend_type}:{name}"),
        };
        let pathname = short.to_kebab_case();
        Self {
            name: name.to_string(),
            backend_type,
            short,
            full,
            cache_path: dirs::CACHE.join(&pathname),
            installs_path: dirs::INSTALLS.join(&pathname),
            downloads_path: dirs::DOWNLOADS.join(&pathname),
            opts,
        }
    }
}

impl Display for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short)
    }
}

impl Debug for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.short != self.full {
            write!(f, r#"BackendArg("{}" -> "{}")"#, self.short, self.full)
        } else {
            write!(f, r#"BackendArg("{}")"#, self.short)
        }
    }
}

impl PartialEq for BackendArg {
    fn eq(&self, other: &Self) -> bool {
        self.short == other.short
    }
}

impl Eq for BackendArg {}

impl Hash for BackendArg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.short.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::reset;
    use pretty_assertions::{assert_eq, assert_str_eq};

    #[test]
    fn test_backend_arg() {
        reset();
        let t = |s: &str, id, name, t| {
            let fa: BackendArg = s.into();
            assert_str_eq!(fa.full, id);
            assert_str_eq!(fa.name, name);
            assert_eq!(fa.backend_type, t);
        };
        let asdf = |s, id, name| t(s, id, name, BackendType::Asdf);
        let cargo = |s, id, name| t(s, id, name, BackendType::Cargo);
        // let core = |s, id, name| t(s, id, name, BackendType::Core);
        let npm = |s, id, name| t(s, id, name, BackendType::Npm);
        let vfox = |s, id, name| t(s, id, name, BackendType::Vfox);

        asdf("asdf:poetry", "asdf:poetry", "poetry");
        asdf("poetry", "poetry", "mise-plugins/mise-poetry");
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
    fn test_backend_arg_pathname() {
        reset();
        let t = |s: &str, expected| {
            let fa: BackendArg = s.into();
            let actual = fa.installs_path.to_string_lossy();
            let expected = dirs::INSTALLS.join(expected);
            assert_str_eq!(actual, expected.to_string_lossy());
        };
        t("asdf:node", "asdf-node");
        t("node", "node");
        t("", "");
        t("cargo:eza", "cargo-eza");
        t("npm:@antfu/ni", "npm-antfu-ni");
        t("npm:prettier", "npm-prettier");
        t(
            "vfox:version-fox/vfox-nodejs",
            "vfox-version-fox-vfox-nodejs",
        );
        t("vfox:version-fox/nodejs", "vfox-version-fox-nodejs");
    }
}
