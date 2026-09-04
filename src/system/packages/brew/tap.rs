//! Metadata extraction for ordinary third-party taps.
//!
//! Homebrew does not publish JSON API metadata for most taps. For those taps,
//! fetch the formula definition and evaluate only its metadata DSL with mise's
//! own Ruby shim. The resulting formula deliberately has no bottles: source
//! installation is the portable fallback and avoids duplicating Homebrew's
//! bottle URL construction rules.

use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde::Deserialize;

use super::api::{self, Formula};
use super::cask::Cask;
use crate::cmd::CmdLineRunner;
use crate::http::HTTP_FETCH;
use crate::result::Result;

const METADATA_SHIM_RB: &str = include_str!("tap_formula_metadata.rb");
const CASK_METADATA_SHIM_RB: &str = include_str!("tap_cask_metadata.rb");

#[derive(Deserialize)]
struct GithubCommit {
    sha: String,
}

#[derive(Deserialize)]
struct GithubRepository {
    default_branch: String,
}

struct TapSource {
    raw_base: String,
    commit: String,
}

pub(super) async fn formula_from_ruby(
    owner: &str,
    tap: &str,
    name: &str,
    tap_url: Option<&str>,
    provision_ruby: bool,
) -> Result<Formula> {
    validate_name(name)?;
    let tap_source = resolve_tap_source(owner, tap, tap_url).await?;
    let (source, source_path) = fetch_ruby_source(&tap_source.raw_base, "Formula", name).await?;
    let checksum = crate::hash::hash_sha256_to_str(&source);
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("formula");
    crate::file::create_dir_all(&cache_dir)?;
    let formula_path = cache_dir.join(format!("{name}-{}.rb", &checksum[..12]));
    crate::file::write(&formula_path, source)?;

    let shim_path = cache_dir.join("mise-brew-tap-metadata.rb");
    ensure_shim(&shim_path, METADATA_SHIM_RB)?;
    let output_path = cache_dir.join(format!("{name}-{}.json", &checksum[..12]));
    let ruby = ruby_for_metadata(name, provision_ruby).await?;
    CmdLineRunner::new(&ruby)
        .arg(&shim_path)
        .envs([
            ("MISE_BREW_FORMULA_FILE", formula_path.display().to_string()),
            (
                "MISE_BREW_METADATA_OUTPUT",
                output_path.display().to_string(),
            ),
            ("MISE_BREW_NAME", name.to_string()),
            ("MISE_BREW_TAP", format!("{owner}/{tap}")),
            ("MISE_BREW_SOURCE_PATH", source_path),
            ("MISE_BREW_SOURCE_CHECKSUM", checksum),
            ("MISE_BREW_TAP_COMMIT", tap_source.commit),
        ])
        .execute_async()
        .await
        .wrap_err_with(|| format!("failed to evaluate Formula/{name}.rb"))?;

    let formula: Formula = serde_json::from_str(&crate::file::read_to_string(&output_path)?)
        .wrap_err_with(|| format!("invalid metadata extracted from Formula/{name}.rb"))?;
    if formula.name != name {
        bail!(
            "tap formula name mismatch: requested '{name}', extracted '{}'",
            formula.name
        );
    }
    Ok(formula)
}

pub(super) async fn cask_from_ruby(
    owner: &str,
    tap: &str,
    token: &str,
    tap_url: Option<&str>,
    provision_ruby: bool,
) -> Result<Cask> {
    validate_name(token)?;
    let tap_source = resolve_tap_source(owner, tap, tap_url).await?;
    let (source, source_path) = fetch_ruby_source(&tap_source.raw_base, "Casks", token).await?;
    let checksum = crate::hash::hash_sha256_to_str(&source);
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("cask-source");
    crate::file::create_dir_all(&cache_dir)?;
    let cask_path = cache_dir.join(format!("{token}-{}.rb", &checksum[..12]));
    crate::file::write(&cask_path, source)?;
    let shim_path = cache_dir.join("mise-brew-tap-cask-metadata.rb");
    ensure_shim(&shim_path, CASK_METADATA_SHIM_RB)?;
    let output_path = cache_dir.join(format!("{token}-{}.json", &checksum[..12]));
    let ruby = ruby_for_metadata(token, provision_ruby).await?;
    CmdLineRunner::new(&ruby)
        .arg(&shim_path)
        .envs([
            ("MISE_BREW_CASK_FILE", cask_path.display().to_string()),
            (
                "MISE_BREW_METADATA_OUTPUT",
                output_path.display().to_string(),
            ),
            ("MISE_BREW_TOKEN", token.to_string()),
            ("MISE_BREW_SOURCE_PATH", source_path),
            ("MISE_BREW_SOURCE_CHECKSUM", checksum),
            ("MISE_BREW_TAP_COMMIT", tap_source.commit),
        ])
        .execute_async()
        .await
        .wrap_err_with(|| format!("failed to evaluate Casks/{token}.rb"))?;
    let cask: Cask = serde_json::from_str(&crate::file::read_to_string(&output_path)?)
        .wrap_err_with(|| format!("invalid metadata extracted from Casks/{token}.rb"))?;
    Ok(cask)
}

async fn resolve_tap_source(owner: &str, tap: &str, tap_url: Option<&str>) -> Result<TapSource> {
    let raw_base = api::tap_raw_base(owner, tap, tap_url)
        .ok_or_else(|| eyre::eyre!("only GitHub tap URLs can be fetched directly"))?;
    let (repo_owner, repo) = github_repository(owner, tap, tap_url)?;
    let repository: GithubRepository = HTTP_FETCH
        .json_cached(format!("https://api.github.com/repos/{repo_owner}/{repo}"))
        .await
        .wrap_err("failed to resolve tap repository")?;
    let commit: GithubCommit = HTTP_FETCH
        .json_cached(format!(
            "https://api.github.com/repos/{repo_owner}/{repo}/commits/{}",
            urlencoding::encode(&repository.default_branch)
        ))
        .await
        .wrap_err("failed to resolve tap default branch")?;
    Ok(TapSource {
        raw_base: raw_base.trim_end_matches("/HEAD").to_string() + "/" + &commit.sha,
        commit: commit.sha,
    })
}

async fn usable_system_ruby() -> Option<PathBuf> {
    let ruby = crate::file::which("ruby")?;
    tokio::process::Command::new(&ruby)
        .args(["-e", "exit 0"])
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|_| ruby)
}

async fn ruby_for_metadata(name: &str, provision_ruby: bool) -> Result<PathBuf> {
    match usable_system_ruby().await {
        Some(ruby) => Ok(ruby),
        None if provision_ruby => super::source::ruby_bin().await,
        None => super::source::installed_ruby_bin().await?.ok_or_else(|| {
            eyre::eyre!(
                "evaluating the tap definition for {name} requires Ruby; install Ruby or run the apply command"
            )
        }),
    }
}

fn github_repository<'a>(
    owner: &'a str,
    tap: &'a str,
    tap_url: Option<&'a str>,
) -> Result<(&'a str, String)> {
    let Some(url) = tap_url else {
        return Ok((owner, format!("homebrew-{tap}")));
    };
    let normalized = url.trim_end_matches('/').trim_end_matches(".git");
    let rest = normalized
        .strip_prefix("https://github.com/")
        .ok_or_else(|| eyre::eyre!("only GitHub tap URLs can be fetched directly"))?;
    let mut parts = rest.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(repo_owner), Some(repo), None) if !repo_owner.is_empty() && !repo.is_empty() => {
            Ok((repo_owner, repo.to_string()))
        }
        _ => bail!("invalid GitHub tap URL '{url}'"),
    }
}

async fn fetch_ruby_source(
    raw_base: &str,
    directory: &str,
    name: &str,
) -> Result<(String, String)> {
    let paths = [
        format!("{directory}/{name}.rb"),
        format!("{directory}/{}/{name}.rb", &name[..1]),
    ];
    let mut last_error = None;
    for path in paths {
        match HTTP_FETCH.get_text(format!("{raw_base}/{path}")).await {
            Ok(source) => return Ok((source, path)),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap()).wrap_err_with(|| format!("tap has no {directory}/{name}.rb"))
}

fn ensure_shim(path: &Path, contents: &str) -> Result<()> {
    if crate::file::read_to_string(path).is_ok_and(|current| current == contents) {
        return Ok(());
    }
    crate::file::write(path, contents)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains(['/', '\\', '\0'])
        || name == "."
        || name == ".."
        || PathBuf::from(name).components().count() != 1
    {
        bail!("invalid tap formula name '{name}'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn rejects_unsafe_formula_names() {
        for name in ["", ".", "..", "../oops", "a/b", "a\\b"] {
            assert!(validate_name(name).is_err(), "accepted {name:?}");
        }
        assert!(validate_name("foo@2").is_ok());
    }

    #[test]
    fn resolves_default_and_explicit_github_repositories() -> Result<()> {
        assert_eq!(
            github_repository("acme", "tools", None)?,
            ("acme", "homebrew-tools".to_string())
        );
        assert_eq!(
            github_repository(
                "acme",
                "tools",
                Some("https://github.com/example/custom.git")
            )?,
            ("example", "custom".to_string())
        );
        Ok(())
    }

    #[tokio::test]
    async fn extracts_formula_metadata_without_homebrew() -> Result<()> {
        let Some(ruby) = usable_system_ruby().await else {
            return Ok(());
        };
        let dir = tempfile::tempdir()?;
        let formula = dir.path().join("widget.rb");
        let output = dir.path().join("widget.json");
        let shim = dir.path().join("shim.rb");
        crate::file::write(
            &formula,
            r#"
class Widget < Formula
  desc "example"
  url "https://example.com/widget-1.2.3.tar.gz"
  sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  depends_on "libfoo"
  depends_on "cmake" => :build
  keg_only :versioned_formula
end
"#,
        )?;
        crate::file::write(&shim, METADATA_SHIM_RB)?;
        let status = Command::new(ruby)
            .arg(shim)
            .env("MISE_BREW_FORMULA_FILE", &formula)
            .env("MISE_BREW_METADATA_OUTPUT", &output)
            .env("MISE_BREW_NAME", "widget")
            .env("MISE_BREW_TAP", "acme/tools")
            .env("MISE_BREW_SOURCE_PATH", "Formula/widget.rb")
            .env("MISE_BREW_SOURCE_CHECKSUM", "bbbb")
            .env("MISE_BREW_TAP_COMMIT", "deadbeef")
            .status()?;
        assert!(status.success());
        let formula: Formula = serde_json::from_str(&crate::file::read_to_string(output)?)?;
        assert_eq!(formula.name, "widget");
        assert_eq!(formula.versions.stable.as_deref(), Some("1.2.3"));
        assert_eq!(formula.dependencies, ["libfoo"]);
        assert_eq!(formula.build_dependencies, ["cmake"]);
        assert!(formula.keg_only);
        assert!(formula.bottle.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn extracts_cask_metadata_without_homebrew() -> Result<()> {
        let Some(ruby) = usable_system_ruby().await else {
            return Ok(());
        };
        let dir = tempfile::tempdir()?;
        let cask_file = dir.path().join("widget.rb");
        let output = dir.path().join("widget.json");
        let shim = dir.path().join("shim.rb");
        crate::file::write(
            &cask_file,
            r#"
cask "widget" do
  version "1.2.3"
  sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  url "https://example.com/widget-#{version}.zip"
  depends_on formula: "libfoo"
  app "Widget.app"
  binary "Widget.app/Contents/MacOS/widget", target: "widget"
end
"#,
        )?;
        crate::file::write(&shim, CASK_METADATA_SHIM_RB)?;
        let status = Command::new(ruby)
            .arg(shim)
            .env("MISE_BREW_CASK_FILE", &cask_file)
            .env("MISE_BREW_METADATA_OUTPUT", &output)
            .env("MISE_BREW_TOKEN", "widget")
            .env("MISE_BREW_SOURCE_PATH", "Casks/widget.rb")
            .env("MISE_BREW_SOURCE_CHECKSUM", "bbbb")
            .env("MISE_BREW_TAP_COMMIT", "deadbeef")
            .status()?;
        assert!(status.success());
        let json = crate::file::read_to_string(output)?;
        let _: Cask = serde_json::from_str(&json)?;
        let metadata: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(metadata["token"], "widget");
        assert_eq!(metadata["version"], "1.2.3");
        assert_eq!(metadata["depends_on"]["formula"][0], "libfoo");
        assert_eq!(metadata["artifacts"].as_array().unwrap().len(), 2);
        Ok(())
    }
}
