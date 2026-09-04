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
use crate::sandbox::SandboxConfig;

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
    crate::file::write(&output_path, "")?;
    let ruby = ruby_for_metadata(name, provision_ruby).await?;
    let mut runner = CmdLineRunner::new(&ruby)
        .arg("--disable-gems")
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
            ("MISE_BREW_MACOS_VERSION", macos_version()),
            ("MISE_BREW_OS", std::env::consts::OS.to_string()),
            ("MISE_BREW_ARCH", std::env::consts::ARCH.to_string()),
        ])
        .with_sandbox(metadata_sandbox(&formula_path, &shim_path, &output_path)?);
    runner.apply_sandbox().await?;
    runner
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
    crate::file::write(&output_path, "")?;
    let ruby = ruby_for_metadata(token, provision_ruby).await?;
    let mut runner = CmdLineRunner::new(&ruby)
        .arg("--disable-gems")
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
            ("MISE_BREW_MACOS_VERSION", macos_version()),
            ("MISE_BREW_OS", std::env::consts::OS.to_string()),
            ("MISE_BREW_ARCH", std::env::consts::ARCH.to_string()),
        ])
        .with_sandbox(metadata_sandbox(&cask_path, &shim_path, &output_path)?);
    runner.apply_sandbox().await?;
    runner
        .execute_async()
        .await
        .wrap_err_with(|| format!("failed to evaluate Casks/{token}.rb"))?;
    let cask: Cask = serde_json::from_str(&crate::file::read_to_string(&output_path)?)
        .wrap_err_with(|| format!("invalid metadata extracted from Casks/{token}.rb"))?;
    Ok(cask)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn metadata_sandbox(source: &Path, shim: &Path, output: &Path) -> Result<SandboxConfig> {
    let mut sandbox = SandboxConfig {
        deny_read: true,
        deny_write: true,
        deny_net: true,
        deny_env: true,
        deny_process: true,
        deny_temp_write: true,
        allow_read: vec![source.to_path_buf(), shim.to_path_buf()],
        allow_write: vec![output.to_path_buf()],
        allow_exec: vec![shim.to_path_buf()],
        ..Default::default()
    };
    sandbox.resolve_paths();
    Ok(sandbox)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn metadata_sandbox(_source: &Path, _shim: &Path, _output: &Path) -> Result<SandboxConfig> {
    bail!(
        "evaluating third-party tap definitions is only supported inside the Linux or macOS process sandbox"
    )
}

fn macos_version() -> String {
    if cfg!(target_os = "macos") {
        crate::cmd::cmd("sw_vers", ["-productVersion"])
            .read()
            .map(|version| version.trim().to_string())
            .unwrap_or_default()
    } else {
        "0".to_string()
    }
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
    ruby_is_compatible(&ruby).await.then_some(ruby)
}

async fn ruby_is_compatible(ruby: &Path) -> bool {
    tokio::process::Command::new(&ruby)
        .args(["-e", "exit RUBY_VERSION.split('.').first.to_i >= 3 ? 0 : 1"])
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .is_some()
}

async fn ruby_for_metadata(name: &str, provision_ruby: bool) -> Result<PathBuf> {
    if let Some(ruby) = usable_system_ruby().await {
        return Ok(ruby);
    }
    if let Some(ruby) = super::source::installed_ruby_bin().await?
        && ruby_is_compatible(&ruby).await
    {
        return Ok(ruby);
    }
    if provision_ruby {
        let ruby = super::source::ruby_bin().await?;
        if ruby_is_compatible(&ruby).await {
            return Ok(ruby);
        }
    }
    bail!(
        "evaluating the tap definition for {name} requires Ruby 3 or newer; install a compatible Ruby or run the apply command"
    )
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

    async fn test_ruby() -> Result<Option<PathBuf>> {
        if let Some(ruby) = usable_system_ruby().await {
            return Ok(Some(ruby));
        }
        super::super::source::installed_ruby_bin().await
    }

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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn extracts_formula_metadata_without_homebrew() -> Result<()> {
        let Some(ruby) = test_ruby().await? else {
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
  version "1.2.3"
  url "https://example.com/widget-#{version}.tar.gz"
  sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  depends_on "libfoo"
  depends_on "cmake" => :build
  on_sequoia :or_older do
    depends_on "release-boundary"
  end
  on_system macos: :sequoia_or_older do
    depends_on "system-release-boundary"
  end
  keg_only :versioned_formula
end
"#,
        )?;
        crate::file::write(&shim, METADATA_SHIM_RB)?;
        let mut runner = CmdLineRunner::new(ruby)
            .with_on_stderr(|line| eprintln!("{line}"))
            .arg("--disable-gems")
            .arg(&shim)
            .env("MISE_BREW_FORMULA_FILE", &formula)
            .env("MISE_BREW_METADATA_OUTPUT", &output)
            .env("MISE_BREW_NAME", "widget")
            .env("MISE_BREW_TAP", "acme/tools")
            .env("MISE_BREW_SOURCE_PATH", "Formula/widget.rb")
            .env("MISE_BREW_SOURCE_CHECKSUM", "bbbb")
            .env("MISE_BREW_TAP_COMMIT", "deadbeef")
            .env("MISE_BREW_MACOS_VERSION", "15.3")
            .env("MISE_BREW_OS", "macos")
            .env("MISE_BREW_ARCH", std::env::consts::ARCH)
            .with_sandbox(metadata_sandbox(&formula, &shim, &output)?);
        runner.apply_sandbox().await?;
        runner.execute_async().await?;
        let formula: Formula = serde_json::from_str(&crate::file::read_to_string(output)?)?;
        assert_eq!(formula.name, "widget");
        assert_eq!(formula.versions.stable.as_deref(), Some("1.2.3"));
        assert_eq!(
            formula.urls["stable"].url,
            "https://example.com/widget-1.2.3.tar.gz"
        );
        assert_eq!(
            formula.dependencies,
            ["libfoo", "release-boundary", "system-release-boundary"]
        );
        assert_eq!(formula.build_dependencies, ["cmake"]);
        assert!(formula.keg_only);
        assert!(formula.bottle.is_empty());
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn extracts_cask_metadata_without_homebrew() -> Result<()> {
        let Some(ruby) = test_ruby().await? else {
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
  sha256 :no_check
  url "https://example.com/widget-#{version}.zip"
  depends_on formula: "libfoo"
  on_sonoma do
    url "https://example.com/wrong-platform.zip"
  end
  on_system macos: :ventura_or_newer do
    url "https://example.com/also-wrong-platform.zip"
  end
  app "Widget.app"
  binary "Widget.app/Contents/MacOS/widget", target: "widget"
end
"#,
        )?;
        crate::file::write(&shim, CASK_METADATA_SHIM_RB)?;
        let mut runner = CmdLineRunner::new(ruby)
            .with_on_stderr(|line| eprintln!("{line}"))
            .arg("--disable-gems")
            .arg(&shim)
            .env("MISE_BREW_CASK_FILE", &cask_file)
            .env("MISE_BREW_METADATA_OUTPUT", &output)
            .env("MISE_BREW_TOKEN", "widget")
            .env("MISE_BREW_SOURCE_PATH", "Casks/widget.rb")
            .env("MISE_BREW_SOURCE_CHECKSUM", "bbbb")
            .env("MISE_BREW_TAP_COMMIT", "deadbeef")
            .env("MISE_BREW_MACOS_VERSION", "0")
            .env("MISE_BREW_OS", std::env::consts::OS)
            .env("MISE_BREW_ARCH", std::env::consts::ARCH)
            .with_sandbox(metadata_sandbox(&cask_file, &shim, &output)?);
        runner.apply_sandbox().await?;
        runner.execute_async().await?;
        let json = crate::file::read_to_string(output)?;
        let _: Cask = serde_json::from_str(&json)?;
        let metadata: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(metadata["token"], "widget");
        assert_eq!(metadata["version"], "1.2.3");
        assert_eq!(metadata["sha256"], "no_check");
        assert_eq!(metadata["url"], "https://example.com/widget-1.2.3.zip");
        assert_eq!(metadata["depends_on"]["formula"][0], "libfoo");
        assert_eq!(metadata["artifacts"].as_array().unwrap().len(), 2);
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn metadata_evaluation_is_fully_sandboxed() {
        let config = metadata_sandbox(
            Path::new("/tmp/formula.rb"),
            Path::new("/tmp/shim.rb"),
            Path::new("/tmp/metadata.json"),
        )
        .unwrap();
        assert!(config.deny_read);
        assert!(config.deny_write);
        assert!(config.deny_net);
        assert!(config.deny_env);
        assert_eq!(config.allow_read.len(), 2);
        assert_eq!(config.allow_write.len(), 1);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn metadata_sandbox_blocks_tap_processes_and_temp_writes() -> Result<()> {
        let Some(ruby) = test_ruby().await? else {
            return Ok(());
        };
        crate::file::create_dir_all(&*crate::env::HOME)?;
        let dir = tempfile::Builder::new()
            .prefix(".mise-tap-sandbox-")
            .tempdir_in(&*crate::env::HOME)?;
        let source = dir.path().join("malicious.rb");
        let output = dir.path().join("metadata.json");
        let temp_dir = tempfile::tempdir()?;
        let denied = temp_dir.path().join("denied");
        crate::file::write(
            &source,
            r#"
begin
  File.write(ARGV.fetch(0), "escaped")
rescue SystemCallError
end
ran = begin
  system("true")
rescue SystemCallError
  false
end
raise "child process escaped sandbox" if ran
begin
  exec("/usr/bin/false")
rescue SystemCallError
end
"#,
        )?;
        crate::file::write(&output, "")?;
        let mut runner = CmdLineRunner::new(ruby)
            .with_on_stderr(|line| eprintln!("{line}"))
            .arg("--disable-gems")
            .arg(&source)
            .arg(&denied)
            .with_sandbox(metadata_sandbox(&source, &source, &output)?);
        runner.apply_sandbox().await?;
        runner.execute_async().await?;
        assert!(!denied.exists());
        Ok(())
    }
}
