use std::path::Path;
use std::sync::Arc;

use crate::args::BackendArg;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::cmd::CmdLineRunner;
use crate::config::{Settings, SettingsExt};
use crate::toolset::ToolVersionOptions;
use crate::{backend::Backend, config::Config, file, timeout};
use async_trait::async_trait;
use eyre::{Result, bail, eyre};
use itertools::Itertools;

pub(crate) const EXPERIMENTAL: bool = true;

/// Compiles Ruby command-line tools from a GitHub repository with
/// [Spinel](https://github.com/matz/spinel), matz's Ruby AOT compiler.
#[derive(Debug)]
pub(crate) struct SpinelBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct SpinelOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> SpinelOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn tag_prefix(&self) -> &'a str {
        self.values.str("tag_prefix").unwrap_or("")
    }

    fn entrypoint(&self) -> &'a str {
        self.values.str("entrypoint").unwrap_or("main.rb")
    }

    fn bin(&self) -> Option<&'a str> {
        self.values.str("bin")
    }

    fn source_ref(&self) -> Option<&'a str> {
        self.values.str("source_ref")
    }

    fn compiler(&self) -> &'a str {
        self.values.str("spinel").unwrap_or("spinel")
    }
}

#[async_trait]
impl Backend for SpinelBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Spinel
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["tag_prefix"]
    }

    fn get_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec!["spinel"])
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let repo = validate_repo(&self.tool_name())?;
        let opts = config.get_tool_opts_with_overrides(self.ba()).await?;
        let prefix = SpinelOptions::new(&opts).tag_prefix().to_string();
        let remote = repo_url(&repo);
        timeout::run_with_timeout_async(
            async || {
                // git ls-remote rather than the GitHub API: no token, no rate limit.
                let output = crate::cmd::cmd_read_async_inherited_env(
                    "git",
                    &["ls-remote", "--tags", "--refs", &remote],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                Ok(versions_from_ls_remote(&output, &prefix))
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn install_version_(
        &self,
        ctx: &crate::install_context::InstallContext,
        tv: crate::toolset::ToolVersion,
    ) -> Result<crate::toolset::ToolVersion> {
        if cfg!(windows) {
            bail!("the spinel backend does not support Windows");
        }
        let request_opts = tv.request.options();
        if SpinelOptions::new(&request_opts)
            .values
            .str("spinel")
            .is_none()
        {
            self.warn_if_dependency_missing(
                &ctx.config,
                "spinel",
                &["spinel"],
                "The spinel backend needs the Spinel compiler on PATH.\n\
Build it from https://github.com/matz/spinel (`make deps && make`),\n\
or point the `spinel` tool option at a compiler binary.",
            )
            .await;
        }

        let repo = validate_repo(&self.tool_name())?;
        let opts = SpinelOptions::new(&request_opts);
        let entrypoint = validate_entrypoint(opts.entrypoint())?;
        let bin = match opts.bin() {
            Some(bin) => bin.to_string(),
            None => repo
                .rsplit('/')
                .next()
                .expect("repo has an owner/name form")
                .to_string(),
        };
        validate_bin(&bin)?;
        let git_ref = match opts.source_ref() {
            Some(sha) => validate_sha(sha)?.to_string(),
            None => {
                let tag = format!("{}{}", opts.tag_prefix(), tv.version);
                validate_tag(&tag)?;
                format!("refs/tags/{tag}")
            }
        };

        let source = tv.download_path().join("source");
        // A kept download directory must not leak files from an earlier build.
        file::remove_all(&source)?;
        let bin_dir = tv.install_path().join("bin");
        file::create_dir_all(&source)?;
        file::create_dir_all(&bin_dir)?;

        let git = |args: &[&str]| -> Result<()> {
            CmdLineRunner::new("git")
                .args(args)
                .current_dir(&source)
                .with_pr(ctx.pr.as_ref())
                .execute()
        };
        git(&["init", "-q"])?;
        git(&["fetch", "-q", "--depth", "1", &repo_url(&repo), &git_ref])?;
        git(&["checkout", "-q", "--detach", "FETCH_HEAD"])?;

        let compiler = self
            .spawn_program(&ctx.config, Some(&ctx.ts), opts.compiler())
            .await;
        CmdLineRunner::new(compiler)
            .arg(entrypoint)
            .arg("-o")
            .arg(bin_dir.join(&bin))
            .current_dir(&source)
            .with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env(&ctx.config).await?)
            .env_values(tv.install_env())
            .execute()?;
        if !is_executable(&bin_dir.join(&bin)) {
            bail!("spinel did not produce an executable at bin/{bin}");
        }
        Ok(tv)
    }
}

impl SpinelBackend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }
}

/// Options that only matter while building, so a cached lockfile entry never
/// pins them.
pub(crate) fn install_time_option_keys() -> Vec<String> {
    vec!["spinel".into()]
}

fn repo_url(repo: &str) -> String {
    format!("https://github.com/{repo}.git")
}

fn validate_repo(tool: &str) -> Result<String> {
    let valid = regex::Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
        .expect("static regex")
        .is_match(tool)
        && !tool.contains("..");
    if !valid {
        bail!("expected spinel:owner/repo, got spinel:{tool}");
    }
    Ok(tool.to_string())
}

fn validate_bin(bin: &str) -> Result<()> {
    let valid = !bin.is_empty()
        && bin != "."
        && bin != ".."
        && bin
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !valid {
        bail!("`bin` must be a plain file name, got {bin:?}");
    }
    Ok(())
}

/// The entrypoint is handed to the compiler inside the checkout, so keep it a
/// relative path that stays there.
fn validate_entrypoint(path: &str) -> Result<&str> {
    let escapes = path.is_empty()
        || path.starts_with('/')
        || path.starts_with('-')
        || path.contains('\\')
        || path.split('/').any(|part| part == ".." || part.is_empty());
    if escapes {
        bail!("`entrypoint` must be a relative path inside the repository, got {path:?}");
    }
    Ok(path)
}

fn validate_sha(sha: &str) -> Result<&str> {
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("`source_ref` must be a full 40-character commit SHA, got {sha:?}");
    }
    Ok(sha)
}

fn validate_tag(tag: &str) -> Result<()> {
    if tag.starts_with('-') || tag.contains("..") || tag.chars().any(|c| c.is_whitespace()) {
        return Err(eyre!("invalid git tag {tag:?}"));
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Parses `git ls-remote --tags --refs` output into opaque versions, keeping
/// only tags that start with `prefix` and removing it.
fn versions_from_ls_remote(output: &str, prefix: &str) -> Vec<VersionInfo> {
    output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(_, git_ref)| git_ref.strip_prefix("refs/tags/"))
        .filter_map(|tag| tag.strip_prefix(prefix))
        .filter(|version| !version.is_empty())
        .unique()
        .map(|version| VersionInfo {
            version: version.to_string(),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_prefixed_tags_without_the_prefix() {
        let output = "aaa\trefs/tags/v1.0.0\nbbb\trefs/tags/v1.1.0\nccc\trefs/tags/nightly\n";
        let versions: Vec<_> = versions_from_ls_remote(output, "v")
            .into_iter()
            .map(|v| v.version)
            .collect();
        assert_eq!(versions, ["1.0.0", "1.1.0"]);
        let all: Vec<_> = versions_from_ls_remote(output, "")
            .into_iter()
            .map(|v| v.version)
            .collect();
        assert_eq!(all, ["v1.0.0", "v1.1.0", "nightly"]);
    }

    #[test]
    fn validates_repo_entrypoint_bin_and_sha() {
        assert!(validate_repo("tobi/try").is_ok());
        assert!(validate_repo("tobi").is_err());
        assert!(validate_repo("a/../b").is_err());
        assert!(validate_entrypoint("try.rb").is_ok());
        assert!(validate_entrypoint("bin/try.rb").is_ok());
        for bad in ["", "/etc/x.rb", "../x.rb", "a//b.rb", "-x.rb", "a\\b.rb"] {
            assert!(validate_entrypoint(bad).is_err(), "{bad}");
        }
        assert!(validate_bin("try").is_ok());
        assert!(validate_bin("a/b").is_err());
        assert!(validate_bin("..").is_err());
        assert!(validate_sha(&"a".repeat(40)).is_ok());
        assert!(validate_sha("abc").is_err());
        assert!(validate_tag("v1.0.0").is_ok());
        assert!(validate_tag("-x").is_err());
    }
}
