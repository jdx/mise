use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::{Result, WrapErr, bail};
use serde::Deserialize;
use serde_json::Value;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};

pub(crate) struct NixManager;

/// Bootstrap accepts attribute paths and explicit flake references, not Nix
/// expressions, store paths, or output selectors. Nix owns source resolution.
#[derive(Debug)]
pub(crate) struct Installable<'a> {
    pub source: &'a str,
    pub attribute: &'a str,
    pub explicit: bool,
}

impl<'a> Installable<'a> {
    pub(crate) fn parse(name: &'a str) -> Result<Self> {
        if name.chars().any(char::is_control) {
            bail!("Nix package references must not contain control characters");
        }
        let (source, attribute, explicit) = match name.split_once('#') {
            Some((source, attribute)) => {
                if source.is_empty()
                    || source.starts_with('-')
                    || source.starts_with('.')
                    || source.starts_with('/')
                    || source
                        .strip_prefix("path:")
                        .is_some_and(|p| !p.starts_with('/'))
                {
                    bail!(
                        "invalid Nix flake reference '{source}'; use a registry name, URL, or path:/absolute/path"
                    );
                }
                (source, attribute, true)
            }
            None => ("nixpkgs", name, false),
        };
        if !valid_attribute(attribute) {
            bail!(
                "invalid Nix package attribute '{attribute}'; use a dotted attribute path (e.g. ripgrep or python3Packages.pip), or '<flake>#<attribute>'; pin versions in the flake reference, not with @version"
            );
        }
        Ok(Self {
            source,
            attribute,
            explicit,
        })
    }

    fn argument(&self) -> String {
        format!("{}#{}", self.source, self.attribute)
    }
}

fn valid_attribute(attribute: &str) -> bool {
    attribute.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\''))
    })
}

pub(crate) fn nix_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace("${", "\\${")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

#[derive(Debug, Deserialize)]
struct Profile {
    version: u32,
    elements: BTreeMap<String, Element>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Element {
    active: bool,
    original_url: Option<String>,
    attr_path: Option<String>,
    outputs: Option<Value>,
    store_paths: Vec<String>,
}

impl Profile {
    fn parse(json: &str) -> Result<Self> {
        let profile: Self = serde_json::from_str(json).wrap_err(
            "Nix bootstrap requires profile JSON with named entries (Nix 2.24 or newer); upgrade Nix and use a modern `nix profile` profile",
        )?;
        if profile.version != 3 {
            bail!(
                "unsupported Nix profile manifest version {}; expected version 3 (Nix 2.24 or newer)",
                profile.version
            );
        }
        Ok(profile)
    }
}

#[derive(Deserialize)]
struct ParsedReferences {
    system: String,
    refs: Vec<Value>,
}

#[derive(Clone)]
struct MatchedEntry {
    name: String,
    paths: Vec<String>,
}

async fn query(args: &[&str]) -> Result<String> {
    debug!("$ nix {}", shell_words::join(args));
    let output = tokio::process::Command::new("nix")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .wrap_err("failed to run Nix; install Nix and make its CLI available on PATH")?;
    if !output.status.success() {
        bail!(
            "nix {} failed: {}\nNix bootstrap requires modern `nix profile` support and the nix-command and flakes experimental features. Enable these in your Nix configuration; mise does not change it.",
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Passing an explicit profile avoids Nix's default-profile initialization:
/// even `nix profile list` otherwise creates ~/.nix-profile on a fresh home.
async fn profile_path() -> Result<PathBuf> {
    let config: Value = serde_json::from_str(&query(&["config", "show", "--json"]).await?)?;
    let xdg = config["use-xdg-base-directories"]["value"]
        .as_bool()
        .unwrap_or(false);
    Ok(if xdg {
        std::env::var_os("XDG_STATE_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::env::HOME.join(".local/state"))
            .join("nix/profile")
    } else {
        crate::env::HOME.join(".nix-profile")
    })
}

async fn matching_entries(pkgs: &[PackageRequest]) -> Result<Vec<Vec<MatchedEntry>>> {
    let requests = pkgs
        .iter()
        .map(|p| Installable::parse(&p.name))
        .collect::<Result<Vec<_>>>()?;
    let path = profile_path().await?;
    if !path.try_exists()? {
        return Ok(vec![vec![]; pkgs.len()]);
    }
    let profile = Profile::parse(
        &query(&[
            "profile",
            "list",
            "--profile",
            &path.to_string_lossy(),
            "--json",
            "--offline",
        ])
        .await?,
    )?;
    let elements = profile
        .elements
        .iter()
        .filter(|(_, e)| {
            e.active && e.original_url.is_some() && e.attr_path.is_some() && e.outputs.is_none()
        })
        .collect::<Vec<_>>();
    if elements.is_empty() {
        return Ok(vec![vec![]; pkgs.len()]);
    }
    // parseFlakeRef normalizes syntax (e.g. nixpkgs == flake:nixpkgs)
    // without resolving a registry entry, evaluating a flake, or fetching it.
    let sources = requests
        .iter()
        .map(|r| r.source)
        .chain(
            elements
                .iter()
                .filter_map(|(_, e)| e.original_url.as_deref()),
        )
        .map(nix_string)
        .collect::<Vec<_>>()
        .join(" ");
    let expr = format!(
        "{{ system = builtins.currentSystem; refs = map builtins.parseFlakeRef [ {sources} ]; }}"
    );
    let parsed: ParsedReferences = serde_json::from_str(
        &query(&["eval", "--json", "--impure", "--offline", "--expr", &expr]).await?,
    )?;
    if parsed.refs.len() != requests.len() + elements.len() {
        bail!("Nix returned an unexpected number of parsed flake references");
    }
    Ok(requests
        .iter()
        .enumerate()
        .map(|(i, request)| {
            elements
                .iter()
                .enumerate()
                .filter(|(j, (_, e))| {
                    parsed.refs[i] == parsed.refs[requests.len() + j]
                        && attribute_matches(
                            request.attribute,
                            e.attr_path.as_deref().unwrap(),
                            &parsed.system,
                        )
                })
                .map(|(_, (name, e))| MatchedEntry {
                    name: (*name).clone(),
                    paths: e.store_paths.clone(),
                })
                .collect()
        })
        .collect())
}

fn attribute_matches(request: &str, installed: &str, system: &str) -> bool {
    installed == request
        || installed == format!("packages.{system}.{request}")
        || installed == format!("legacyPackages.{system}.{request}")
}

async fn action(args: Vec<String>, opts: &InstallOpts) -> Result<()> {
    if opts.dry_run {
        miseprintln!("nix {}", shell_words::join(&args));
        return Ok(());
    }
    debug!("$ nix {}", shell_words::join(&args));
    let status = tokio::process::Command::new("nix")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    if !status.success() {
        bail!(
            "nix {} failed; see Nix's diagnostic above. Ensure nix-command and flakes are enabled and the profile uses modern `nix profile` format",
            shell_words::join(&args)
        );
    }
    Ok(())
}

fn reject_pins(pkgs: &[PackageRequest]) -> Result<()> {
    if let Some(pkg) = pkgs.iter().find(|p| p.version.is_some()) {
        bail!(
            "Nix cannot install package version pin '{pkg}'; pin the flake source revision instead"
        );
    }
    Ok(())
}

#[async_trait(?Send)]
impl SystemPackageManager for NixManager {
    fn name(&self) -> &str {
        "nix"
    }

    fn is_available(&self) -> bool {
        cfg!(unix) && crate::file::which("nix").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(unix) {
            "nix not found on PATH"
        } else {
            "only available on Unix"
        }
        .into()
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let entries = matching_entries(pkgs).await?;
        Ok(pkgs
            .iter()
            .zip(entries)
            .map(|(request, entries)| {
                let state = if entries.is_empty() {
                    PackageState::Missing
                } else {
                    let version = entries
                        .into_iter()
                        .flat_map(|entry| entry.paths)
                        .collect::<Vec<_>>()
                        .join(", ");
                    if request.version.is_some() {
                        PackageState::VersionMismatch { installed: version }
                    } else {
                        PackageState::Installed { version }
                    }
                };
                PackageStatus {
                    request: request.clone(),
                    state,
                }
            })
            .collect())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        reject_pins(pkgs)?;
        if pkgs.is_empty() {
            return Ok(());
        }
        let mut args = vec!["profile".into(), "install".into()];
        if opts.update {
            args.push("--refresh".into());
        }
        args.push("--".into());
        for pkg in pkgs {
            args.push(Installable::parse(&pkg.name)?.argument());
        }
        action(args, opts).await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        reject_pins(pkgs)?;
        if pkgs.is_empty() {
            return Ok(());
        }
        let names = matching_entries(pkgs)
            .await?
            .into_iter()
            .flatten()
            .map(|entry| entry.name)
            .collect::<std::collections::BTreeSet<_>>();
        if names.is_empty() {
            return Ok(());
        }
        let mut args = vec!["profile".into(), "upgrade".into(), "--".into()];
        args.extend(names);
        action(args, opts).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installables_preserve_sources() {
        assert_eq!(
            Installable::parse("ripgrep").unwrap().argument(),
            "nixpkgs#ripgrep"
        );
        for input in [
            "github:owner/repo/revision#hello",
            "git+ssh://git@host/repo?ref=topic#hello",
            "path:/tmp/a b#packages.x86_64-linux.hello",
            "private#nested.package",
        ] {
            assert_eq!(Installable::parse(input).unwrap().argument(), input);
        }
        for input in [
            "",
            "#hello",
            "./local#hello",
            "path:relative#hello",
            "hello@1",
            "hello^out",
            "hello..world",
            "hello\nworld",
            "${throw}",
            "--expr#hello",
        ] {
            assert!(Installable::parse(input).is_err(), "{input}");
        }
    }

    #[test]
    fn attributes_match_only_the_selected_path_and_system() {
        assert!(attribute_matches(
            "python3Packages.pip",
            "legacyPackages.x86_64-linux.python3Packages.pip",
            "x86_64-linux"
        ));
        assert!(!attribute_matches(
            "pip",
            "legacyPackages.x86_64-linux.python3Packages.pip",
            "x86_64-linux"
        ));
        assert!(!attribute_matches(
            "hello",
            "packages.aarch64-linux.hello",
            "x86_64-linux"
        ));
    }

    #[test]
    fn profile_format_is_checked() {
        assert!(Profile::parse(r#"{"version":3,"elements":{}}"#).is_ok());
        assert!(Profile::parse(r#"{"version":2,"elements":[]}"#).is_err());
        assert!(Profile::parse(r#"{"version":4,"elements":{}}"#).is_err());
        assert!(Profile::parse("not json").is_err());
    }

    #[test]
    fn strings_escape_interpolation_and_quotes() {
        assert_eq!(nix_string("a\"\\${x}\n"), "\"a\\\"\\\\\\${x}\\n\"");
    }
}
