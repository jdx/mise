use std::env::temp_dir;
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::DAILY;
use crate::env::PATH_KEY;
use crate::git::{CloneOptions, Git};
use crate::github::GithubRelease;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, file, plugins, timeout};
use async_trait::async_trait;
use eyre::{Result, WrapErr};
use itertools::Itertools;
use xx::regex;

#[derive(Debug)]
pub struct RubyPlugin {
    ba: Arc<BackendArg>,
}

impl RubyPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("ruby")),
        }
    }

    fn ruby_build_path(&self) -> PathBuf {
        self.ba.cache_path.join("ruby-build")
    }
    fn ruby_install_path(&self) -> PathBuf {
        self.ba.cache_path.join("ruby-install")
    }

    fn ruby_build_bin(&self) -> PathBuf {
        self.ruby_build_path().join("bin/ruby-build")
    }

    fn ruby_install_bin(&self) -> PathBuf {
        self.ruby_install_path().join("bin/ruby-install")
    }

    fn lock_build_tool(&self) -> Result<fslock::LockFile> {
        let settings = Settings::get();
        let build_tool_path = if settings.ruby.ruby_install {
            self.ruby_install_bin()
        } else {
            self.ruby_build_bin()
        };
        LockFile::new(&build_tool_path)
            .with_callback(|l| {
                trace!("install_or_update_ruby_build_tool {}", l.display());
            })
            .lock()
    }

    async fn update_build_tool(&self, ctx: Option<&InstallContext>) -> Result<()> {
        let pr = ctx.map(|ctx| &ctx.pr);
        if Settings::get().ruby.ruby_install {
            self.update_ruby_install(pr)
                .await
                .wrap_err("failed to update ruby-install")
        } else {
            self.update_ruby_build(pr)
                .await
                .wrap_err("failed to update ruby-build")
        }
    }

    fn install_ruby_build(&self, pr: Option<&Box<dyn SingleReport>>) -> Result<()> {
        debug!(
            "Installing ruby-build to {}",
            self.ruby_build_path().display()
        );
        let tmp = temp_dir().join("mise-ruby-build");
        file::remove_all(&tmp)?;
        file::create_dir_all(tmp.parent().unwrap())?;
        let git = Git::new(tmp.clone());
        let mut clone_options = CloneOptions::default();
        if let Some(pr) = pr {
            clone_options = clone_options.pr(pr);
        }
        git.clone(&Settings::get().ruby.ruby_build_repo, clone_options)?;

        cmd!("sh", "install.sh")
            .env("PREFIX", self.ruby_build_path())
            .dir(&tmp)
            .run()?;
        file::remove_all(&tmp)?;
        Ok(())
    }
    async fn update_ruby_build(&self, pr: Option<&Box<dyn SingleReport>>) -> Result<()> {
        let _lock = self.lock_build_tool();
        if self.ruby_build_bin().exists() {
            let cur = self.ruby_build_version()?;
            let latest = self.latest_ruby_build_version().await;
            match (cur, latest) {
                // ruby-build is up-to-date
                (cur, Ok(latest)) if cur == latest => return Ok(()),
                // ruby-build is not up-to-date
                (_cur, Ok(_latest)) => {}
                // error getting latest ruby-build version (usually github rate limit)
                (_cur, Err(err)) => warn!("failed to get latest ruby-build version: {}", err),
            }
        }
        debug!(
            "Updating ruby-build in {}",
            self.ruby_build_path().display()
        );
        file::remove_all(self.ruby_build_path())?;
        self.install_ruby_build(pr)?;
        Ok(())
    }

    fn install_ruby_install(&self, pr: Option<&Box<dyn SingleReport>>) -> Result<()> {
        let settings = Settings::get();
        debug!(
            "Installing ruby-install to {}",
            self.ruby_install_path().display()
        );
        let tmp = temp_dir().join("mise-ruby-install");
        file::remove_all(&tmp)?;
        file::create_dir_all(tmp.parent().unwrap())?;
        let git = Git::new(tmp.clone());
        let mut clone_options = CloneOptions::default();
        if let Some(pr) = pr {
            clone_options = clone_options.pr(pr);
        }
        git.clone(&settings.ruby.ruby_install_repo, clone_options)?;

        cmd!("make", "install")
            .env("PREFIX", self.ruby_install_path())
            .dir(&tmp)
            .stdout_to_stderr()
            .run()?;
        file::remove_all(&tmp)?;
        Ok(())
    }
    async fn update_ruby_install(&self, pr: Option<&Box<dyn SingleReport>>) -> Result<()> {
        let _lock = self.lock_build_tool();
        let ruby_install_path = self.ruby_install_path();
        if !ruby_install_path.exists() {
            self.install_ruby_install(pr)?;
        }
        if self.ruby_install_recently_updated()? {
            return Ok(());
        }
        debug!("Updating ruby-install in {}", ruby_install_path.display());

        plugins::core::run_fetch_task_with_timeout(move || {
            cmd!(&ruby_install_path, "--update")
                .stdout_to_stderr()
                .run()?;
            file::touch_dir(&ruby_install_path)?;
            Ok(())
        })
    }

    fn ruby_install_recently_updated(&self) -> Result<bool> {
        let updated_at = file::modified_duration(&self.ruby_install_path())?;
        Ok(updated_at < DAILY)
    }

    fn gem_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/gem")
    }

    async fn install_default_gems(
        &self,
        config: &Arc<Config>,
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
            pr.set_message(format!("install default gem: {package}"));
            let gem = self.gem_path(tv);
            let mut cmd = CmdLineRunner::new(gem)
                .with_pr(pr)
                .arg("install")
                .envs(config.env().await?);
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

    fn ruby_build_version(&self) -> Result<String> {
        let output = cmd!(self.ruby_build_bin(), "--version").read()?;
        let re = regex!(r"^ruby-build ([0-9.]+)");
        let caps = re.captures(&output).expect("ruby-build version regex");
        Ok(caps.get(1).unwrap().as_str().to_string())
    }

    async fn latest_ruby_build_version(&self) -> Result<String> {
        let release: GithubRelease = HTTP_FETCH
            .json("https://api.github.com/repos/rbenv/ruby-build/releases/latest")
            .await?;
        Ok(release.tag_name.trim_start_matches('v').to_string())
    }

    fn install_rubygems_hook(&self, tv: &ToolVersion) -> Result<()> {
        let site_ruby_path = tv.install_path().join("lib/ruby/site_ruby");
        let f = site_ruby_path.join("rubygems_plugin.rb");
        file::create_dir_all(site_ruby_path)?;
        file::write(f, include_str!("assets/rubygems_plugin.rb"))?;
        Ok(())
    }

    async fn install_cmd<'a>(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &'a Box<dyn SingleReport>,
    ) -> Result<CmdLineRunner<'a>> {
        let settings = Settings::get();
        let cmd = if settings.ruby.ruby_install {
            CmdLineRunner::new(self.ruby_install_bin()).args(self.install_args_ruby_install(tv)?)
        } else {
            CmdLineRunner::new(self.ruby_build_bin())
                .args(self.install_args_ruby_build(tv)?)
                .stdin_string(self.fetch_patches().await?)
        };
        Ok(cmd.with_pr(pr).envs(config.env().await?))
    }
    fn install_args_ruby_build(&self, tv: &ToolVersion) -> Result<Vec<String>> {
        let settings = Settings::get();
        let mut args = vec![];
        if self.verbose_install() {
            args.push("--verbose".into());
        }
        if settings.ruby.apply_patches.is_some() {
            args.push("--patch".into());
        }
        args.push(tv.version.clone());
        args.push(tv.install_path().to_string_lossy().to_string());
        if let Some(opts) = &settings.ruby.ruby_build_opts {
            args.push("--".into());
            args.extend(shell_words::split(opts)?);
        }
        Ok(args)
    }
    fn install_args_ruby_install(&self, tv: &ToolVersion) -> Result<Vec<String>> {
        let settings = Settings::get();
        let mut args = vec![];
        for patch in self.fetch_patch_sources() {
            args.push("--patch".into());
            args.push(patch);
        }
        let (engine, version) = match tv.version.split_once('-') {
            Some((engine, version)) => (engine, version),
            None => ("ruby", tv.version.as_str()),
        };
        args.push(engine.into());
        args.push(version.into());
        args.push("--install-dir".into());
        args.push(tv.install_path().to_string_lossy().to_string());
        if let Some(opts) = &settings.ruby.ruby_install_opts {
            args.push("--".into());
            args.extend(shell_words::split(opts)?);
        }
        Ok(args)
    }

    fn verbose_install(&self) -> bool {
        let settings = Settings::get();
        let verbose_env = settings.ruby.verbose_install;
        verbose_env == Some(true) || (settings.verbose && verbose_env != Some(false))
    }

    fn fetch_patch_sources(&self) -> Vec<String> {
        let settings = Settings::get();
        let patch_sources = settings.ruby.apply_patches.clone().unwrap_or_default();
        patch_sources
            .split('\n')
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    async fn fetch_patches(&self) -> Result<String> {
        let mut patches = vec![];
        let re = regex!(r#"^[Hh][Tt][Tt][Pp][Ss]?://"#);
        for f in &self.fetch_patch_sources() {
            if re.is_match(f) {
                patches.push(HTTP.get_text(f).await?);
            } else {
                patches.push(file::read_to_string(f)?);
            }
        }
        Ok(patches.join("\n"))
    }
}

#[async_trait]
impl Backend for RubyPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }
    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        timeout::run_with_timeout_async(
            async || {
                if let Err(err) = self.update_build_tool(None).await {
                    warn!("{err}");
                }
                let ruby_build_bin = self.ruby_build_bin();
                let versions = plugins::core::run_fetch_task_with_timeout(move || {
                    let output = cmd!(ruby_build_bin, "--definitions").read()?;
                    let versions = output
                        .split('\n')
                        .sorted_by_cached_key(|s| regex!(r#"^\d"#).is_match(s)) // show matz ruby first
                        .map(|s| s.to_string())
                        .collect();
                    Ok(versions)
                })?;
                Ok(versions)
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
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

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if let Err(err) = self.update_build_tool(Some(ctx)).await {
            warn!("ruby build tool update error: {err:#}");
        }
        ctx.pr.set_message("ruby-build".into());
        self.install_cmd(&ctx.config, &tv, &ctx.pr)
            .await?
            .execute()?;

        self.install_rubygems_hook(&tv)?;
        if let Err(err) = self.install_default_gems(&ctx.config, &tv, &ctx.pr).await {
            warn!("failed to install default ruby gems {err:#}");
        }
        Ok(tv)
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

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
