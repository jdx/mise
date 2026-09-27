//! Homebrew formula and cask metadata parsing and tap URL parsing.

pub mod cask;

use std::collections::HashMap;

use eyre::{Result, eyre};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Formula {
    pub name: String,
    #[serde(default)]
    pub tap: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// names this formula had before a rename
    #[serde(default)]
    pub oldnames: Vec<String>,
    pub versions: Versions,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub keg_only: bool,
    #[serde(default)]
    pub keg_only_reason: Option<KegOnlyReason>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct KegOnlyReason {
    /// ":provided_by_macos", another reason symbol, or free text
    #[serde(default)]
    pub reason: String,
}

impl Formula {
    /// every name that refers to this formula: canonical, aliases, old names
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str())
            .chain(self.aliases.iter().map(String::as_str))
            .chain(self.oldnames.iter().map(String::as_str))
    }

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

    /// Mirrors brew's KegOnlyReason#applicable?: keg-only reasons tied to
    /// macOS (:provided_by_macos, :shadowed_by_macos) do not apply on other
    /// OSes, where brew links these formulae normally.
    pub fn keg_only_for_target(&self) -> bool {
        if !self.keg_only {
            return false;
        }
        if cfg!(target_os = "macos") {
            return true;
        }
        !matches!(
            self.keg_only_reason.as_ref().map(|r| r.reason.as_str()),
            Some(":provided_by_macos") | Some(":shadowed_by_macos")
        )
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

pub fn tap_name(name: &str) -> Option<String> {
    let (owner, tap, _) = split_tap_name(name)?;
    if owner == "homebrew" && tap == "core" {
        None
    } else {
        Some(format!("{owner}/{tap}"))
    }
}

pub fn tap_name_from_url(url: &str) -> Option<String> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!(
        "{owner}/{}",
        repo.strip_prefix("homebrew-").unwrap_or(repo)
    ))
}

pub fn split_tap(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() {
        None
    } else {
        Some((owner, tap))
    }
}

pub fn split_tap_name(name: &str) -> Option<(&str, &str, &str)> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    let formula = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() || formula.is_empty() {
        None
    } else {
        Some((owner, tap, formula))
    }
}

pub fn tap_formula_api_url(
    owner: &str,
    tap: &str,
    formula: &str,
    tap_url: Option<&str>,
) -> Option<String> {
    let repo = tap_raw_base(owner, tap, tap_url)?;
    Some(format!("{repo}/api/formula/{formula}.json"))
}

pub fn tap_raw_base(owner: &str, tap: &str, tap_url: Option<&str>) -> Option<String> {
    match tap_url {
        Some(url) => github_raw_base(url),
        None => Some(format!(
            "https://raw.githubusercontent.com/{owner}/homebrew-{tap}/HEAD"
        )),
    }
}

pub fn github_raw_base(url: &str) -> Option<String> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        None
    } else {
        Some(format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/HEAD"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keg_only_formula(reason: Option<&str>) -> Formula {
        let mut json = serde_json::json!({
            "name": "zip",
            "versions": {"stable": "3.0"},
            "keg_only": true,
        });
        if let Some(reason) = reason {
            json["keg_only_reason"] = serde_json::json!({"reason": reason});
        }
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn macos_keg_only_reasons_only_apply_on_macos() {
        for reason in [":provided_by_macos", ":shadowed_by_macos"] {
            let formula = keg_only_formula(Some(reason));
            assert_eq!(formula.keg_only_for_target(), cfg!(target_os = "macos"));
        }
    }

    #[test]
    fn other_keg_only_reasons_apply_everywhere() {
        assert!(keg_only_formula(Some(":versioned_formula")).keg_only_for_target());
        assert!(keg_only_formula(Some("free-text reason")).keg_only_for_target());
        assert!(keg_only_formula(None).keg_only_for_target());
    }

    #[test]
    fn formula_names_include_aliases_and_old_names() {
        let formula: Formula = serde_json::from_value(serde_json::json!({
            "name": "gitea-runner",
            "aliases": ["runner"],
            "oldnames": ["act_runner"],
            "versions": {"stable": "1.0"},
        }))
        .unwrap();
        assert_eq!(
            formula.names().collect::<Vec<_>>(),
            ["gitea-runner", "runner", "act_runner"]
        );
    }

    #[test]
    fn github_tap_urls_allow_trailing_slashes() {
        let url = "https://github.com/acme/homebrew-tools.git/";
        assert_eq!(tap_name_from_url(url).as_deref(), Some("acme/tools"));
        assert_eq!(
            github_raw_base(url).as_deref(),
            Some("https://raw.githubusercontent.com/acme/homebrew-tools/HEAD")
        );
    }
}
