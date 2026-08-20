//! Client for the formulae.brew.sh JSON API (static JSON, no auth).

use std::collections::HashMap;

use eyre::{WrapErr, bail, eyre};
use serde::Deserialize;
use serde_json::Value;

use crate::http::HTTP_FETCH;
use crate::result::Result;

const API_BASE: &str = "https://formulae.brew.sh/api";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FetchMode {
    Cached,
    Fresh,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Formula {
    pub name: String,
    #[serde(default)]
    pub tap: Option<String>,
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
    #[serde(default)]
    pub post_install_steps: Vec<Value>,
    #[serde(default)]
    pub post_install_defined: bool,
    /// Homebrew install policy that must be checked before any keg mutation.
    /// Kept grouped so callers cannot accidentally deserialize a policy field
    /// without including it in the common validation boundary.
    #[serde(flatten)]
    pub(super) install_policy: FormulaInstallPolicy,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct FormulaInstallPolicy {
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    disable_date: Option<String>,
    #[serde(default)]
    disable_reason: Option<String>,
    #[serde(default)]
    disable_replacement_formula: Option<String>,
    #[serde(default)]
    disable_replacement_cask: Option<String>,
    #[serde(default)]
    deprecated: bool,
    #[serde(default)]
    deprecation_date: Option<String>,
    #[serde(default)]
    deprecation_reason: Option<String>,
    #[serde(default)]
    deprecation_replacement_formula: Option<String>,
    #[serde(default)]
    deprecation_replacement_cask: Option<String>,
    #[serde(default)]
    requirements: Vec<FormulaRequirement>,
    #[serde(default)]
    pour_bottle_only_if: Option<String>,
    #[serde(default)]
    conflicts_with: Vec<String>,
    #[serde(default)]
    conflicts_with_reasons: Vec<Option<String>>,
    #[serde(default)]
    link_overwrite: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FormulaRequirement {
    name: String,
    #[serde(default)]
    cask: Option<String>,
    #[serde(default)]
    download: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    contexts: Vec<String>,
    #[serde(default)]
    specs: Vec<String>,
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
    pub rebuild: u32,
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
    /// Validate Homebrew policy that mise must understand before installing.
    ///
    /// `link_overwrite` is intentionally represented but does not grant
    /// overwrite authority: mise's topology preflight remains conservative
    /// and rejects an occupied destination. This preserves user/Homebrew state
    /// until exact typed overwrite ownership exists.
    pub fn validate_install_policy(&self) -> Result<()> {
        let policy = &self.install_policy;
        if policy.disabled {
            let detail = policy_detail(
                policy.disable_date.as_deref(),
                policy.disable_reason.as_deref(),
                policy.disable_replacement_formula.as_deref(),
                policy.disable_replacement_cask.as_deref(),
            );
            bail!(
                "brew:{} is disabled by Homebrew{detail}; mise will not install it",
                self.name
            );
        }

        let unsupported_requirements = policy
            .requirements
            .iter()
            .filter(|requirement| !requirement.is_satisfied())
            .collect::<Vec<_>>();
        if !unsupported_requirements.is_empty() {
            let requirements = unsupported_requirements
                .into_iter()
                .map(FormulaRequirement::describe)
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "brew:{} declares unsupported Homebrew requirements ({requirements}); refusing to install without exact requirement enforcement",
                self.name
            );
        }

        if let Some(condition) = policy.pour_bottle_only_if.as_deref() {
            bail!(
                "brew:{} declares unsupported pour_bottle_only_if policy {condition:?}; refusing to install without exact predicate enforcement",
                self.name
            );
        }

        if self
            .link_overwrite()
            .iter()
            .any(|pattern| pattern.trim().is_empty())
        {
            bail!(
                "brew:{} declares an empty link_overwrite pattern; refusing ambiguous overwrite policy",
                self.name
            );
        }

        if policy.deprecated {
            let detail = policy_detail(
                policy.deprecation_date.as_deref(),
                policy.deprecation_reason.as_deref(),
                policy.deprecation_replacement_formula.as_deref(),
                policy.deprecation_replacement_cask.as_deref(),
            );
            warn!("brew:{} is deprecated by Homebrew{detail}", self.name);
        }
        Ok(())
    }

    pub fn conflicts_with(&self) -> &[String] {
        &self.install_policy.conflicts_with
    }

    pub fn conflict_reason(&self, name: &str) -> Option<&str> {
        let index = self
            .install_policy
            .conflicts_with
            .iter()
            .position(|conflict| conflict == name)?;
        self.install_policy
            .conflicts_with_reasons
            .get(index)
            .and_then(|reason| reason.as_deref())
    }

    /// Patterns are informational until an exact ownership-aware overwrite
    /// implementation exists. Callers must not treat them as mutation authority.
    pub fn link_overwrite(&self) -> &[String] {
        &self.install_policy.link_overwrite
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

impl FormulaRequirement {
    fn is_satisfied(&self) -> bool {
        if !cfg!(target_os = "macos")
            || self.name != "xcode"
            || self.version.is_some()
            || self.cask.is_some()
            || self.download.is_some()
            || self.contexts.as_slice() != ["build"]
            || self.specs.as_slice() != ["stable"]
        {
            return false;
        }
        let selected = std::process::Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .output()
            .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
        let compiler = std::process::Command::new("/usr/bin/xcrun")
            .args(["--find", "clang"])
            .output()
            .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
        selected && compiler
    }

    fn describe(&self) -> String {
        let mut attributes = Vec::new();
        if let Some(version) = &self.version {
            attributes.push(format!("version={version}"));
        }
        if let Some(cask) = &self.cask {
            attributes.push(format!("cask={cask}"));
        }
        if let Some(download) = &self.download {
            attributes.push(format!("download={download}"));
        }
        if !self.contexts.is_empty() {
            attributes.push(format!("contexts={}", self.contexts.join("|")));
        }
        if !self.specs.is_empty() {
            attributes.push(format!("specs={}", self.specs.join("|")));
        }
        if attributes.is_empty() {
            self.name.clone()
        } else {
            format!("{}[{}]", self.name, attributes.join(","))
        }
    }
}

fn policy_detail(
    date: Option<&str>,
    reason: Option<&str>,
    replacement_formula: Option<&str>,
    replacement_cask: Option<&str>,
) -> String {
    let mut details = Vec::new();
    if let Some(date) = date {
        details.push(format!("date {date}"));
    }
    if let Some(reason) = reason {
        details.push(format!("reason {reason}"));
    }
    if let Some(replacement) = replacement_formula {
        details.push(format!("replacement formula {replacement}"));
    }
    if let Some(replacement) = replacement_cask {
        details.push(format!("replacement cask {replacement}"));
    }
    if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    }
}

/// Fetch formula metadata by name (or alias — brew's API redirects aliases
/// to the canonical formula).
pub(super) async fn formula_with_mode(name: &str, mode: FetchMode) -> Result<Formula> {
    let url = format!("{API_BASE}/formula/{name}.json");
    let result = match mode {
        FetchMode::Cached => HTTP_FETCH.json_cached::<Formula, _>(url).await,
        FetchMode::Fresh => HTTP_FETCH.json::<Formula, _>(url).await,
    };
    result.wrap_err_with(|| format!("failed to fetch Homebrew formula '{name}'"))
}

pub(super) async fn formula_with_tap_name_mode(
    name: &str,
    tap_name: Option<&str>,
    tap_url: Option<&str>,
    mode: FetchMode,
) -> Result<Formula> {
    let Some((owner, tap, formula_name)) = split_tap_name(name).or_else(|| {
        let (owner, tap) = split_tap(tap_name?)?;
        Some((owner, tap, name))
    }) else {
        return formula_with_mode(name, mode).await;
    };
    if owner == "homebrew" && tap == "core" {
        return formula_with_mode(formula_name, mode).await;
    }
    let Some(url) = tap_formula_api_url(owner, tap, formula_name, tap_url) else {
        bail!(
            "brew: tapped formula '{name}' needs a GitHub tap URL in [bootstrap.brew.taps] \
             so mise can fetch metadata directly without the brew CLI"
        );
    };
    let result = match mode {
        FetchMode::Cached => HTTP_FETCH.json_cached::<Formula, _>(url).await,
        FetchMode::Fresh => HTTP_FETCH.json::<Formula, _>(url).await,
    };
    result.wrap_err_with(|| {
        format!(
            "failed to fetch Homebrew tap formula '{name}' directly. \
                 The tap must publish API metadata at api/formula/{formula_name}.json; \
                 mise will not proxy to the brew CLI"
        )
    })
}

pub(super) fn tap_name(name: &str) -> Option<String> {
    let (owner, tap, _) = split_tap_name(name)?;
    if owner == "homebrew" && tap == "core" {
        None
    } else {
        Some(format!("{owner}/{tap}"))
    }
}

fn split_tap(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() {
        None
    } else {
        Some((owner, tap))
    }
}

pub(super) fn split_tap_name(name: &str) -> Option<(&str, &str, &str)> {
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

fn tap_formula_api_url(
    owner: &str,
    tap: &str,
    formula: &str,
    tap_url: Option<&str>,
) -> Option<String> {
    let repo = tap_raw_base(owner, tap, tap_url)?;
    Some(format!("{repo}/api/formula/{formula}.json"))
}

pub(super) fn tap_raw_base(owner: &str, tap: &str, tap_url: Option<&str>) -> Option<String> {
    match tap_url {
        Some(url) => github_raw_base(url),
        None => Some(format!(
            "https://raw.githubusercontent.com/{owner}/homebrew-{tap}/HEAD"
        )),
    }
}

pub(super) fn github_raw_base(url: &str) -> Option<String> {
    let url = url.trim_end_matches(".git").trim_end_matches('/');
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
    use serde_json::{Value, json};

    use super::*;

    fn formula_with_policy(policy: Value) -> Formula {
        let mut value = json!({
            "name": "policy-test",
            "versions": { "stable": "1.0" }
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(policy.as_object().unwrap().clone());
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn install_policy_defaults_are_safe() {
        let formula = formula_with_policy(json!({}));
        formula.validate_install_policy().unwrap();
        assert!(formula.conflicts_with().is_empty());
        assert!(formula.link_overwrite().is_empty());
    }

    #[test]
    fn disabled_formula_is_rejected_with_typed_context() {
        let formula = formula_with_policy(json!({
            "disabled": true,
            "disable_date": "2026-08-01",
            "disable_reason": "does_not_build",
            "disable_replacement_formula": "replacement"
        }));
        let error = formula.validate_install_policy().unwrap_err().to_string();
        assert!(error.contains("disabled by Homebrew"));
        assert!(error.contains("does_not_build"));
        assert!(error.contains("replacement formula replacement"));
    }

    #[test]
    fn unsupported_requirements_and_pour_predicates_are_rejected() {
        let requirement = formula_with_policy(json!({
            "requirements": [{
                "name": "xcode",
                "version": "26.0",
                "contexts": ["build"],
                "specs": ["stable"]
            }]
        }));
        let error = requirement
            .validate_install_policy()
            .unwrap_err()
            .to_string();
        assert!(error.contains("xcode[version=26.0,contexts=build,specs=stable]"));

        let predicate = formula_with_policy(json!({
            "pour_bottle_only_if": "default_prefix"
        }));
        assert!(
            predicate
                .validate_install_policy()
                .unwrap_err()
                .to_string()
                .contains("default_prefix")
        );
    }

    #[test]
    fn conflict_and_link_policy_is_typed_but_link_overwrite_grants_no_authority() {
        let formula = formula_with_policy(json!({
            "conflicts_with": ["other", "third"],
            "conflicts_with_reasons": ["same binary", null],
            "link_overwrite": ["bin/tool", "share/tool/*"],
            "deprecated": true,
            "deprecation_reason": "versioned_formula"
        }));
        formula.validate_install_policy().unwrap();
        assert_eq!(formula.conflicts_with(), ["other", "third"]);
        assert_eq!(formula.conflict_reason("other"), Some("same binary"));
        assert_eq!(formula.conflict_reason("third"), None);
        assert_eq!(formula.link_overwrite(), ["bin/tool", "share/tool/*"]);
    }
}
