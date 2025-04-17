use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::PATH_KEY;
use crate::github::GithubRelease;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, github, plugins};
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct RubyPlugin {
    ba: BackendArg,
}

impl RubyPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("ruby"),
        }
    }

    fn ruby_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join("ruby.exe")
    }

    fn gem_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join("gem.cmd")
    }

    fn install_default_gems(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        let settings = Settings::get();
        let default_gems_file = file::replace_path(&settings.ruby.default_packages_file);
        let body = file::read_to_string(&default_gems_file).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("install default gem: {}", package));
            let gem = self.gem_path(tv);
            let mut cmd = CmdLineRunner::new(gem)
                .with_pr(pr)
                .arg("install")
                .envs(config.env()?);
            match package.split_once(' ') {
                Some((name, "--pre")) => cmd = cmd.arg(name).arg("--pre"),
                Some((name, version)) => cmd = cmd.arg(name).arg("--version").arg(version),
                None => cmd = cmd.arg(package),
            };
            cmd.env(&*PATH_KEY, plugins::core::path_env_with_tv_path(tv)?)
                .execute()?;
        }
        Ok(())
    }

    fn test_ruby(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<()> {
        pr.set_message("ruby -v".into());
        CmdLineRunner::new(self.ruby_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(Config::get().env()?)
            .execute()
    }

    fn test_gem(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        pr.set_message("gem -v".into());
        CmdLineRunner::new(self.gem_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(config.env()?)
            .env(&*PATH_KEY, plugins::core::path_env_with_tv_path(tv)?)
            .execute()
    }

    fn install_rubygems_hook(&self, tv: &ToolVersion) -> Result<()> {
        let site_ruby_path = tv.install_path().join("lib/ruby/site_ruby");
        let f = site_ruby_path.join("rubygems_plugin.rb");
        file::create_dir_all(site_ruby_path)?;
        file::write(f, include_str!("assets/rubygems_plugin.rb"))?;
        Ok(())
    }

    fn download(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<PathBuf> {
        let arch = arch();
        let url = format!(
            "https://github.com/oneclick/rubyinstaller2/releases/download/RubyInstaller-{version}-1/rubyinstaller-{version}-1-{arch}.7z",
            version = tv.version,
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let arch = arch();
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::un7z(tarball_path, &tv.download_path())?;
        file::rename(
            tv.download_path()
                .join(format!("rubyinstaller-{}-1-{arch}", tv.version)),
            tv.install_path(),
        )?;
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_ruby(tv, &ctx.pr)
    }
}

impl Backend for RubyPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }
    fn _list_remote_versions(&self, _tr: Option<ToolRequest>) -> Result<Vec<String>> {
        // TODO: use windows set of versions
        //  match self.core.fetch_remote_versions_from_mise() {
        //      Ok(Some(versions)) => return Ok(versions),
        //      Ok(None) => {}
        //      Err(e) => warn!("failed to fetch remote versions: {}", e),
        //  }
        let releases: Vec<GithubRelease> = github::list_releases("oneclick/rubyinstaller2")?;
        let versions = releases
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| {
                regex!(r"RubyInstaller-([0-9.]+)-.*")
                    .replace(&v, "$1")
                    .parse()
                    .ok()
            })
            .unique()
            .sorted_by_cached_key(|s: &String| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".ruby-version".into(), "Gemfile".into()])
    }

    fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        let v = match path.file_name() {
            Some(name) if name == "Gemfile" => parse_gemfile(&file::read_to_string(path)?),
            _ => {
                // .ruby-version
                let body = file::read_to_string(path)?;
                body.trim()
                    .trim_start_matches("ruby-")
                    .trim_start_matches('v')
                    .to_string()
            }
        };
        Ok(v)
    }

    fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let config = Config::get();
        let tarball = self.download(&tv, &ctx.pr)?;
        self.verify_checksum(ctx, &mut tv, &tarball)?;
        self.install(ctx, &tv, &tarball)?;
        self.verify(ctx, &tv)?;
        self.install_rubygems_hook(&tv)?;
        self.test_gem(&config, &tv, &ctx.pr)?;
        if let Err(err) = self.install_default_gems(&config, &tv, &ctx.pr) {
            warn!("failed to install default ruby gems {err:#}");
        }
        Ok(tv)
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        _tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let map = BTreeMap::new();
        // No modification to RUBYLIB
        Ok(map)
    }
}

fn parse_gemfile(body: &str) -> String {
    let v = body
        .lines()
        .find(|line| line.trim().starts_with("ruby "))
        .unwrap_or_default()
        .trim()
        .split('#')
        .next()
        .unwrap_or_default()
        .replace("engine:", ":engine =>")
        .replace("engine_version:", ":engine_version =>");
    let v = regex!(r#".*:engine *=> *['"](?<engine>[^'"]*).*:engine_version *=> *['"](?<engine_version>[^'"]*).*"#).replace_all(&v, "${engine_version}__ENGINE__${engine}").to_string();
    let v = regex!(r#".*:engine_version *=> *['"](?<engine_version>[^'"]*).*:engine *=> *['"](?<engine>[^'"]*).*"#).replace_all(&v, "${engine_version}__ENGINE__${engine}").to_string();
    let v = regex!(r#" *ruby *['"]([^'"]*).*"#)
        .replace_all(&v, "$1")
        .to_string();
    let v = regex!(r#"^[^0-9]"#).replace_all(&v, "").to_string();
    let v = regex!(r#"(.*)__ENGINE__(.*)"#)
        .replace_all(&v, "$2-$1")
        .to_string();
    // make sure it's like "ruby-3.0.0" or "3.0.0"
    if !regex!(r"^(\w+-)?([0-9])(\.[0-9])*$").is_match(&v) {
        return "".to_string();
    }
    v
}

#[allow(clippy::if_same_then_else)]
fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "x64"
    } else {
        "x64"
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_list_versions_matching() {
        let plugin = RubyPlugin::new();
        assert!(
            !plugin.list_versions_matching(None, "3").unwrap().is_empty(),
            "versions for 3 should not be empty"
        );
        assert!(
            !plugin
                .list_versions_matching(None, "truffleruby-24")
                .unwrap()
                .is_empty(),
            "versions for truffleruby-24 should not be empty"
        );
        assert!(
            !plugin
                .list_versions_matching(None, "truffleruby+graalvm-24")
                .unwrap()
                .is_empty(),
            "versions for truffleruby+graalvm-24 should not be empty"
        );
    }

    #[test]
    fn test_parse_gemfile() {
        assert_eq!(
            parse_gemfile(indoc! {r#"
            ruby '2.7.2'
        "#}),
            "2.7.2"
        );
        assert_eq!(
            parse_gemfile(indoc! {r#"
            ruby '1.9.3', engine: 'jruby', engine_version: "1.6.7"
        "#}),
            "jruby-1.6.7"
        );
        assert_eq!(
            parse_gemfile(indoc! {r#"
            ruby '1.9.3', :engine => 'jruby', :engine_version => '1.6.7'
        "#}),
            "jruby-1.6.7"
        );
        assert_eq!(
            parse_gemfile(indoc! {r#"
            ruby '1.9.3', :engine_version => '1.6.7', :engine => 'jruby'
        "#}),
            "jruby-1.6.7"
        );
        assert_eq!(
            parse_gemfile(indoc! {r#"
            source "https://rubygems.org"
            ruby File.read(File.expand_path(".ruby-version", __dir__)).strip
        "#}),
            ""
        );
    }
}
