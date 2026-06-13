//! Client for the formulae.brew.sh JSON API (static JSON, no auth).

use std::collections::HashMap;

use eyre::{WrapErr, eyre};
use serde::Deserialize;

use crate::http::HTTP_FETCH;
use crate::result::Result;

const API_BASE: &str = "https://formulae.brew.sh/api";

#[derive(Debug, Clone, Deserialize)]
pub struct Formula {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub versions: Versions,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub keg_only: bool,
    /// runtime dependencies (formula names)
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// build-time-only dependencies — needed for source builds, not pours
    #[serde(default)]
    pub build_dependencies: Vec<String>,
    #[serde(default)]
    pub bottle: HashMap<String, BottleSpec>,
    /// per-bottle-tag overrides (e.g. different dependencies on some platforms)
    #[serde(default)]
    pub variations: HashMap<String, Variation>,
    /// source download specs keyed by spec name ("stable")
    #[serde(default)]
    pub urls: HashMap<String, SourceUrl>,
    /// formula .rb location in homebrew/core (e.g. "Formula/h/hello.rb")
    #[serde(default)]
    pub ruby_source_path: Option<String>,
    #[serde(default)]
    pub ruby_source_checksum: Option<RubySourceChecksum>,
    /// homebrew/core commit this API snapshot was generated from
    #[serde(default)]
    pub tap_git_head: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceUrl {
    pub url: String,
    /// sha256 of the source archive; absent for VCS sources
    #[serde(default)]
    pub checksum: Option<String>,
    /// non-default download strategy (":git", ":svn", ...) — unsupported
    #[serde(default)]
    pub using: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RubySourceChecksum {
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Versions {
    pub stable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BottleSpec {
    #[serde(default)]
    pub files: HashMap<String, BottleFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BottleFile {
    /// ":any", ":any_skip_relocation", or a pinned cellar path
    pub cellar: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Variation {
    #[serde(default)]
    pub dependencies: Option<Vec<String>>,
    #[serde(default)]
    pub build_dependencies: Option<Vec<String>>,
}

impl Formula {
    /// keg directory name: version plus brew's bottle revision suffix
    pub fn pkg_version(&self) -> Result<String> {
        let stable = self
            .versions
            .stable
            .as_ref()
            .ok_or_else(|| eyre!("formula {} has no stable version", self.name))?;
        Ok(if self.revision > 0 {
            format!("{stable}_{}", self.revision)
        } else {
            stable.clone()
        })
    }

    /// runtime dependencies for the given bottle tag, applying `variations`
    pub fn dependencies_for(&self, tag: &str) -> &[String] {
        if let Some(v) = self.variations.get(tag)
            && let Some(deps) = &v.dependencies
        {
            return deps;
        }
        &self.dependencies
    }

    /// build-time dependencies for the given bottle tag, applying `variations`
    pub fn build_dependencies_for(&self, tag: &str) -> &[String] {
        if let Some(v) = self.variations.get(tag)
            && let Some(deps) = &v.build_dependencies
        {
            return deps;
        }
        &self.build_dependencies
    }

    pub fn bottle_files(&self) -> Option<&HashMap<String, BottleFile>> {
        self.bottle.get("stable").map(|b| &b.files)
    }

    /// the stable source archive spec, when present
    pub fn stable_url(&self) -> Option<&SourceUrl> {
        self.urls.get("stable")
    }
}

/// Fetch formula metadata by name (or alias — brew's API redirects aliases
/// to the canonical formula).
pub async fn formula(name: &str) -> Result<Formula> {
    let url = format!("{API_BASE}/formula/{name}.json");
    HTTP_FETCH
        .json_cached::<Formula, _>(url)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to fetch Homebrew formula '{name}'. mise only supports homebrew/core formulae \
                 (third-party taps require Homebrew itself)"
            )
        })
}
