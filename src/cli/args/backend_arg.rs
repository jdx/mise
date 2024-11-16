use crate::backend::backend_type::BackendType;
use crate::backend::{unalias_backend, ABackend};
use crate::config::CONFIG;
use crate::registry::REGISTRY;
use crate::toolset::{install_state, parse_tool_options, ToolVersionOptions};
use crate::{backend, config, dirs};
use contracts::requires;
use eyre::{bail, Result};
use heck::ToKebabCase;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::PathBuf;
use xx::regex;

#[derive(Clone)]
pub struct BackendArg {
    /// short or full identifier (what the user specified), "node", "prettier", "npm:prettier", "cargo:eza"
    pub short: String,
    /// full identifier, "core:node", "npm:prettier", "cargo:eza", "vfox:version-fox/vfox-nodejs"
    full: Option<String>,
    /// the name of the tool within the backend, e.g.: "node", "prettier", "eza", "vfox-nodejs"
    pub tool_name: String,
    /// ~/.local/share/mise/cache/<THIS>
    pub cache_path: PathBuf,
    /// ~/.local/share/mise/installs/<THIS>
    pub installs_path: PathBuf,
    /// ~/.local/share/mise/downloads/<THIS>
    pub downloads_path: PathBuf,
    /// ~/.local/share/mise/plugins/<THIS>
    pub plugin_path: PathBuf,
    pub opts: Option<ToolVersionOptions>,
    // TODO: make this not a hash key anymore to use this
    // backend: OnceCell<ABackend>,
}

impl<A: AsRef<str>> From<A> for BackendArg {
    fn from(s: A) -> Self {
        let s = s.as_ref();
        let short = unalias_backend(s).to_string();
        if let Some(backend) = REGISTRY
            .get(&s)
            .and_then(|rbm| rbm.backends().first().copied())
        {
            BackendArg::new(short, Some(backend.to_string()))
        } else {
            Self::new(short, None)
        }
    }
}

impl BackendArg {
    #[requires(!short.is_empty())]
    pub fn new(short: String, full: Option<String>) -> Self {
        let short = unalias_backend(&short).to_string();
        let (_backend, mut tool_name) = full
            .as_ref()
            .unwrap_or(&short)
            .split_once(':')
            .unwrap_or(("", full.as_ref().unwrap_or(&short)));
        let short = regex!(r#"\[.+\]$"#).replace_all(&short, "").to_string();

        let mut opts = None;
        if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(tool_name) {
            tool_name = c.get(1).unwrap().as_str();
            opts = Some(parse_tool_options(c.get(2).unwrap().as_str()));
        }

        Self::new_raw(short.clone(), full.clone(), tool_name.to_string(), opts)
    }

    pub fn new_raw(
        short: String,
        full: Option<String>,
        tool_name: String,
        opts: Option<ToolVersionOptions>,
    ) -> Self {
        let pathname = short.to_kebab_case();
        Self {
            tool_name,
            short,
            full,
            plugin_path: dirs::PLUGINS.join(&pathname),
            cache_path: dirs::CACHE.join(&pathname),
            installs_path: dirs::INSTALLS.join(&pathname),
            downloads_path: dirs::DOWNLOADS.join(&pathname),
            opts,
            // backend: Default::default(),
        }
    }

    pub fn backend(&self) -> Result<ABackend> {
        // TODO: see above about hash key
        // let backend = self.backend.get_or_try_init(|| {
        //     if let Some(backend) = backend::get(self) {
        //         Ok(backend)
        //     } else {
        //         bail!("{self} not found in mise tool registry");
        //     }
        // })?;
        // Ok(backend.clone())
        if let Some(backend) = backend::get(self) {
            Ok(backend)
        } else {
            bail!("{self} not found in mise tool registry");
        }
    }

    pub fn backend_type(&self) -> BackendType {
        if let Ok(Some(backend_type)) = install_state::backend_type(&self.short) {
            return backend_type;
        }
        let full = self.full();
        let backend = full.split(':').next().unwrap();
        if let Ok(backend_type) = backend.parse() {
            return backend_type;
        }
        if config::is_loaded() {
            if let Some(repo_url) = CONFIG.get_repo_url(&self.short) {
                return if repo_url.contains("vfox-") {
                    BackendType::Vfox
                } else {
                    // TODO: maybe something more intelligent?
                    BackendType::Asdf
                };
            }
        }
        BackendType::Unknown
    }

    pub fn full(&self) -> String {
        if config::is_loaded() {
            if let Some(full) = CONFIG
                .all_aliases
                .get(&self.short)
                .and_then(|a| a.backend.clone())
            {
                return full;
            }
        }
        if let Some(full) = &self.full {
            return full.clone();
        }
        unalias_backend(&self.short).to_string()
    }

    pub fn is_os_supported(&self) -> bool {
        if self.uses_plugin() {
            return true;
        }
        if let Some(rt) = REGISTRY.get(self.short.as_str()) {
            return rt.is_supported_os();
        }
        true
    }

    pub fn uses_plugin(&self) -> bool {
        install_state::get_plugin_type(&self.short).is_ok_and(|pt| pt.is_some())
    }
}

impl Display for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short)
    }
}

impl Debug for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(full) = &self.full {
            write!(f, r#"BackendArg("{}" -> "{}")"#, self.short, full)
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

impl PartialOrd for BackendArg {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.short.cmp(&other.short))
    }
}

impl Ord for BackendArg {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.short.cmp(&other.short)
    }
}

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
        let t = |s: &str, full, tool_name, t| {
            let fa: BackendArg = s.into();
            assert_str_eq!(full, fa.full());
            assert_str_eq!(tool_name, fa.tool_name);
            assert_eq!(t, fa.backend_type());
        };
        let asdf = |s, full, name| t(s, full, name, BackendType::Asdf);
        let cargo = |s, full, name| t(s, full, name, BackendType::Cargo);
        // let core = |s, full, name| t(s, full, name, BackendType::Core);
        let npm = |s, full, name| t(s, full, name, BackendType::Npm);
        let vfox = |s, full, name| t(s, full, name, BackendType::Vfox);

        asdf("asdf:poetry", "asdf:poetry", "poetry");
        asdf(
            "poetry",
            "asdf:mise-plugins/mise-poetry",
            "mise-plugins/mise-poetry",
        );
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
