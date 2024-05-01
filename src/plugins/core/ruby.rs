use std::collections::BTreeMap;
use std::env::temp_dir;
use std::path::{Path, PathBuf};

use eyre::Result;
use eyre::WrapErr;

use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::DAILY;
use crate::forge::Forge;
use crate::git::Git;
use crate::github::GithubRelease;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::plugins::core::CorePlugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, env, file};

#[derive(Debug)]
pub struct RubyPlugin {
    core: CorePlugin,
}

impl RubyPlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("ruby"),
        }
    }

    fn ruby_build_path(&self) -> PathBuf {
        self.core.fa.cache_path.join("ruby-build")
    }
    fn ruby_install_path(&self) -> PathBuf {
        self.core.fa.cache_path.join("ruby-install")
    }

    fn ruby_build_bin(&self) -> PathBuf {
        self.ruby_build_path().join("bin/ruby-build")
    }

    fn ruby_install_bin(&self) -> PathBuf {
        self.ruby_install_path().join("bin/ruby-install")
    }

    fn lock_build_tool(&self) -> Result<fslock::LockFile> {
        let build_tool_path = if *env::MISE_RUBY_INSTALL {
            self.ruby_build_bin()
        } else {
            self.ruby_install_bin()
        };
        LockFile::new(&build_tool_path)
            .with_callback(|l| {
                trace!("install_or_update_ruby_build_tool {}", l.display());
            })
            .lock()
    }

    fn update_build_tool(&self) -> Result<()> {
        if *env::MISE_RUBY_INSTALL {
            self.update_ruby_install()
                .wrap_err("failed to update ruby-install")?;
        }
        self.update_ruby_build()
            .wrap_err("failed to update ruby-build")
    }

    fn install_ruby_build(&self) -> Result<()> {
        debug!(
            "Installing ruby-build to {}",
            self.ruby_build_path().display()
        );
        let tmp = temp_dir().join("mise-ruby-build");
        file::remove_all(&tmp)?;
        file::create_dir_all(tmp.parent().unwrap())?;
        let git = Git::new(tmp.clone());
        git.clone(&env::MISE_RUBY_BUILD_REPO)?;

        cmd!("sh", "install.sh")
            .env("PREFIX", self.ruby_build_path())
            .dir(&tmp)
            .run()?;
        file::remove_all(&tmp)?;
        Ok(())
    }
    fn update_ruby_build(&self) -> Result<()> {
        let _lock = self.lock_build_tool();
        if self.ruby_build_path().exists() {
            let cur = self.ruby_build_version()?;
            let latest = self.latest_ruby_build_version();
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
        self.install_ruby_build()?;
        Ok(())
    }

    fn install_ruby_install(&self) -> Result<()> {
        debug!(
            "Installing ruby-install to {}",
            self.ruby_install_path().display()
        );
        let tmp = temp_dir().join("mise-ruby-install");
        file::remove_all(&tmp)?;
        file::create_dir_all(tmp.parent().unwrap())?;
        let git = Git::new(tmp.clone());
        git.clone(&env::MISE_RUBY_INSTALL_REPO)?;

        cmd!("make", "install")
            .env("PREFIX", self.ruby_install_path())
            .dir(&tmp)
            .run()?;
        file::remove_all(&tmp)?;
        Ok(())
    }
    fn update_ruby_install(&self) -> Result<()> {
        let _lock = self.lock_build_tool();
        let ruby_install_path = self.ruby_install_path();
        if !ruby_install_path.exists() {
            self.install_ruby_install()?;
        }
        if self.ruby_install_recently_updated()? {
            return Ok(());
        }
        debug!("Updating ruby-install in {}", ruby_install_path.display());

        CorePlugin::run_fetch_task_with_timeout(move || {
            cmd!(&ruby_install_path, "--update").run()?;
            file::touch_dir(&ruby_install_path)?;
            Ok(())
        })
    }

    fn ruby_install_recently_updated(&self) -> Result<bool> {
        let updated_at = file::modified_duration(&self.ruby_install_path())?;
        Ok(updated_at < DAILY)
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        if let Err(err) = self.update_build_tool() {
            warn!("{err}");
        }
        let ruby_build_bin = self.ruby_build_bin();
        let versions = CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(ruby_build_bin, "--definitions").read()?;
            let versions = output
                .split('\n')
                .filter(|s| regex!(r"^[0-9].+$").is_match(s))
                .map(|s| s.to_string())
                .collect();
            Ok(versions)
        })?;
        Ok(versions)
    }

    fn ruby_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/ruby")
    }

    fn gem_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/gem")
    }

    fn install_default_gems(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> Result<()> {
        let body = file::read_to_string(&*env::MISE_RUBY_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default gem: {}", package));
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
            cmd.env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
                .execute()?;
        }
        Ok(())
    }

    fn test_ruby(&self, config: &Config, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("ruby -v".into());
        CmdLineRunner::new(self.ruby_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(config.env()?)
            .execute()
    }

    fn test_gem(&self, config: &Config, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("gem -v".into());
        CmdLineRunner::new(self.gem_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(config.env()?)
            .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
            .execute()
    }

    fn ruby_build_version(&self) -> Result<String> {
        let output = cmd!(self.ruby_build_bin(), "--version").read()?;
        let re = regex!(r"^ruby-build ([0-9.]+)");
        let caps = re.captures(&output).expect("ruby-build version regex");
        Ok(caps.get(1).unwrap().as_str().to_string())
    }

    fn latest_ruby_build_version(&self) -> Result<String> {
        let release: GithubRelease =
            HTTP_FETCH.json("https://api.github.com/repos/rbenv/ruby-build/releases/latest")?;
        Ok(release.tag_name.trim_start_matches('v').to_string())
    }

    fn install_rubygems_hook(&self, tv: &ToolVersion) -> Result<()> {
        let d = self.rubygems_plugins_path(tv);
        let f = d.join("rubygems_plugin.rb");
        file::create_dir_all(d)?;
        file::write(f, include_str!("assets/rubygems_plugin.rb"))?;
        Ok(())
    }

    fn rubygems_plugins_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("lib/rubygems_plugin")
    }

    fn install_cmd<'a>(
        &'a self,
        config: &'a Config,
        tv: &ToolVersion,
        pr: &'a dyn SingleReport,
    ) -> Result<CmdLineRunner> {
        let cmd = if *env::MISE_RUBY_INSTALL {
            CmdLineRunner::new(self.ruby_install_bin()).args(self.install_args_ruby_install(tv)?)
        } else {
            CmdLineRunner::new(self.ruby_build_bin())
                .args(self.install_args_ruby_build(tv)?)
                .stdin_string(self.fetch_patches()?)
        };
        Ok(cmd.with_pr(pr).envs(config.env()?))
    }
    fn install_args_ruby_build(&self, tv: &ToolVersion) -> Result<Vec<String>> {
        let mut args = env::MISE_RUBY_BUILD_OPTS.clone()?;
        if self.verbose_install() {
            args.push("--verbose".into());
        }
        if env::MISE_RUBY_APPLY_PATCHES.is_some() {
            args.push("--patch".into());
        }
        args.push(tv.version.clone());
        args.push(tv.install_path().to_string_lossy().to_string());
        Ok(args)
    }
    fn install_args_ruby_install(&self, tv: &ToolVersion) -> Result<Vec<String>> {
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
        args.extend(env::MISE_RUBY_INSTALL_OPTS.clone()?);
        Ok(args)
    }

    fn verbose_install(&self) -> bool {
        let settings = Settings::get();
        let verbose_env = *env::MISE_RUBY_VERBOSE_INSTALL;
        verbose_env == Some(true) || (settings.verbose && verbose_env != Some(false))
    }

    fn fetch_patch_sources(&self) -> Vec<String> {
        let patch_sources = env::MISE_RUBY_APPLY_PATCHES.clone().unwrap_or_default();
        patch_sources
            .split('\n')
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn fetch_patches(&self) -> Result<String> {
        let mut patches = vec![];
        for f in &self.fetch_patch_sources() {
            if regex!(r#"^[Hh][Tt][Tt][Pp][Ss]?://"#).is_match(f) {
                patches.push(HTTP.get_text(f)?);
            } else {
                patches.push(file::read_to_string(f)?);
            }
        }
        Ok(patches.join("\n"))
    }
}

impl Forge for RubyPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }
    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".ruby-version".into(), "Gemfile".into()])
    }

    fn parse_legacy_file(&self, path: &Path) -> Result<String> {
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

    #[requires(matches!(ctx.tv.request, ToolVersionRequest::Version { .. } | ToolVersionRequest::Prefix { .. }), "unsupported tool version request type")]
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        if let Err(err) = self.update_build_tool() {
            warn!("{err}");
        }
        ctx.pr.set_message("running ruby-build".into());
        let config = Config::get();
        self.install_cmd(&config, &ctx.tv, ctx.pr.as_ref())?
            .execute()?;

        self.test_ruby(&config, &ctx.tv, ctx.pr.as_ref())?;
        self.install_rubygems_hook(&ctx.tv)?;
        self.test_gem(&config, &ctx.tv, ctx.pr.as_ref())?;
        self.install_default_gems(&config, &ctx.tv, ctx.pr.as_ref())?;
        Ok(())
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        // TODO: is there a way to avoid needing to set RUBYLIB?
        // is there a directory I can put rubygems_plugin.rb in that will be automatically loaded?
        let rubygems_plugin_path = self.rubygems_plugins_path(tv);
        let mut map = BTreeMap::new();
        if rubygems_plugin_path.exists() {
            let rubygems_plugin_path = rubygems_plugin_path.to_string_lossy().to_string();
            let rubylib = match env::PRISTINE_ENV.get("RUBYLIB") {
                Some(rubylib) => format!("{}:{}", rubylib, rubygems_plugin_path),
                None => rubygems_plugin_path,
            };
            map.insert("RUBYLIB".to_string(), rubylib);
        }
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
