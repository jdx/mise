use std::collections::{BTreeMap, HashMap};
use std::env::temp_dir;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{Result, WrapErr, eyre};
use itertools::Itertools;
use xx::regex;

use crate::backend::platform_target::PlatformTarget;
use crate::backend::{Backend, VersionInfo};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::DAILY;
use crate::env::{self, PATH_KEY};
use crate::git::{CloneOptions, Git};
use crate::github::{self, GithubRelease};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::lockfile::PlatformInfo;
use crate::plugins::PluginSource;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, hash, plugins, timeout};

const RUBY_INDEX_URL: &str = "https://cache.ruby-lang.org/pub/ruby/index.txt";
const ATTESTATION_HELP: &str = "To disable attestation verification, set MISE_RUBY_GITHUB_ATTESTATIONS=false\n\
    or add `ruby.github_attestations = false` to your mise config";

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
        let pr = ctx.map(|ctx| ctx.pr.as_ref());
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

    async fn install_ruby_build(&self, pr: Option<&dyn SingleReport>) -> Result<()> {
        debug!(
            "Installing ruby-build to {}",
            self.ruby_build_path().display()
        );
        let settings = Settings::get();
        let tmp = self
            .prepare_source_in_tmp(&settings.ruby.ruby_build_repo, pr, "mise-ruby-build")
            .await?;

        cmd!("sh", "install.sh")
            .env("PREFIX", self.ruby_build_path())
            .dir(&tmp)
            .run()?;
        file::remove_all(&tmp)?;
        Ok(())
    }
    async fn update_ruby_build(&self, pr: Option<&dyn SingleReport>) -> Result<()> {
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
        self.install_ruby_build(pr).await?;
        Ok(())
    }

    async fn install_ruby_install(&self, pr: Option<&dyn SingleReport>) -> Result<()> {
        debug!(
            "Installing ruby-install to {}",
            self.ruby_install_path().display()
        );
        let settings = Settings::get();
        let tmp = self
            .prepare_source_in_tmp(&settings.ruby.ruby_install_repo, pr, "mise-ruby-install")
            .await?;
        cmd!("make", "install")
            .env("PREFIX", self.ruby_install_path())
            .dir(&tmp)
            .stdout_to_stderr()
            .run()?;
        file::remove_all(&tmp)?;
        Ok(())
    }
    async fn update_ruby_install(&self, pr: Option<&dyn SingleReport>) -> Result<()> {
        let _lock = self.lock_build_tool();
        let ruby_install_path = self.ruby_install_path();
        if !ruby_install_path.exists() {
            self.install_ruby_install(pr).await?;
        }
        if self.ruby_install_recently_updated()? {
            return Ok(());
        }
        debug!("Updating ruby-install in {}", ruby_install_path.display());

        plugins::core::run_fetch_task_with_timeout(move || {
            cmd!(self.ruby_install_bin(), "--update")
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

    async fn prepare_source_in_tmp(
        &self,
        repo: &str,
        pr: Option<&dyn SingleReport>,
        tmp_dir_name: &str,
    ) -> Result<PathBuf> {
        let tmp = temp_dir().join(tmp_dir_name);
        file::remove_all(&tmp)?;
        file::create_dir_all(tmp.parent().unwrap())?;
        let source = PluginSource::parse(repo);
        match source {
            PluginSource::Zip { url } => {
                let temp_archive = tmp.join("ruby.zip");
                HTTP.download_file(url, &temp_archive, pr).await?;

                if let Some(pr) = pr {
                    pr.set_message("extracting zip file".to_string());
                }

                let strip_components =
                    file::should_strip_components(&temp_archive, file::TarFormat::Zip)?;

                file::unzip(
                    &temp_archive,
                    &tmp,
                    &file::ZipOptions {
                        strip_components: if strip_components { 1 } else { 0 },
                    },
                )?;
            }
            PluginSource::Git {
                url: repo_url,
                git_ref: _,
            } => {
                let git = Git::new(tmp.clone());
                let mut clone_options = CloneOptions::default();
                if let Some(pr) = pr {
                    clone_options = clone_options.pr(pr);
                }
                git.clone(&repo_url, clone_options)?;
            }
        }
        Ok(tmp)
    }

    fn gem_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/gem")
    }

    async fn install_default_gems(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
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
        pr: &'a dyn SingleReport,
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

    /// Fetch Ruby source tarball info from cache.ruby-lang.org index
    /// Returns (url, sha256) for the given version
    async fn get_ruby_download_info(&self, version: &str) -> Result<Option<(String, String)>> {
        // Only standard MRI Ruby versions are in the index (e.g., "3.3.0", not "jruby-9.4.0")
        if !version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Ok(None);
        }

        let index_text: String = HTTP_FETCH.get_text(RUBY_INDEX_URL).await?;

        // Format: name\turl\tsha1\tsha256\tsha512
        // Example: ruby-3.3.0\thttps://cache.ruby-lang.org/pub/ruby/3.3/ruby-3.3.0.tar.gz\t...\t<sha256>\t...
        let target_name = format!("ruby-{version}");
        for line in index_text.lines().skip(1) {
            // skip header
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                let name = parts[0];
                // Match exact version with .tar.gz (prefer over .tar.xz for compatibility)
                if name == target_name {
                    let url = parts[1];
                    let sha256 = parts[3];
                    if url.ends_with(".tar.gz") && !sha256.is_empty() {
                        return Ok(Some((url.to_string(), format!("sha256:{sha256}"))));
                    }
                }
            }
        }

        Ok(None)
    }

    // ===== Precompiled Ruby support =====

    /// Check if precompiled binaries should be tried
    /// Precompiled if: explicit opt-in (compile=false), or experimental + not opted out
    /// TODO(2026.8.0): make precompiled the default when compile is unset, remove this debug_assert
    fn should_try_precompiled(&self) -> bool {
        debug_assert!(
            *crate::cli::version::V < versions::Versioning::new("2026.8.0").unwrap(),
            "precompiled ruby should be the default now, update should_try_precompiled()"
        );
        let settings = Settings::get();
        settings.ruby.compile == Some(false)
            || (settings.experimental && settings.ruby.compile.is_none())
    }

    /// Get platform identifier for precompiled binaries
    /// Returns platform in jdx/ruby format: "macos", "arm64_linux", or "x86_64_linux"
    fn precompiled_platform(&self) -> Option<String> {
        let settings = Settings::get();

        // Check for user overrides first
        if let (Some(arch), Some(os)) = (
            settings.ruby.precompiled_arch.as_deref(),
            settings.ruby.precompiled_os.as_deref(),
        ) {
            return Some(format!("{}_{}", arch, os));
        }

        // Auto-detect platform
        if cfg!(target_os = "macos") {
            // macOS only supports arm64 and uses "macos" without arch prefix
            match settings.arch() {
                "arm64" | "aarch64" => Some("macos".to_string()),
                _ => None,
            }
        } else if cfg!(target_os = "linux") {
            // Linux uses arch_linux format
            let arch = match settings.arch() {
                "arm64" | "aarch64" => "arm64",
                "x64" | "x86_64" => "x86_64",
                _ => return None,
            };
            Some(format!("{}_linux", arch))
        } else {
            None
        }
    }

    /// Get platform identifier for a specific target (used for lockfiles)
    /// Returns platform in jdx/ruby format: "macos", "arm64_linux", or "x86_64_linux"
    fn precompiled_platform_for_target(&self, target: &PlatformTarget) -> Option<String> {
        match target.os_name() {
            "macos" => {
                // macOS only supports arm64 and uses "macos" without arch prefix
                match target.arch_name() {
                    "arm64" | "aarch64" => Some("macos".to_string()),
                    _ => None,
                }
            }
            "linux" => {
                // Linux uses arch_linux format
                let arch = match target.arch_name() {
                    "arm64" | "aarch64" => "arm64",
                    "x64" | "x86_64" => "x86_64",
                    _ => return None,
                };
                Some(format!("{}_linux", arch))
            }
            _ => None,
        }
    }

    /// Render URL template with version and platform variables
    fn render_precompiled_url(&self, template: &str, version: &str, platform: &str) -> String {
        let (arch, os) = platform.split_once('_').unwrap_or((platform, ""));
        template
            .replace("{version}", version)
            .replace("{platform}", platform)
            .replace("{os}", os)
            .replace("{arch}", arch)
    }

    /// Check if the system needs the no-YJIT variant (glibc < 2.35 on Linux).
    /// YJIT builds from jdx/ruby require glibc 2.35+.
    fn needs_no_yjit() -> bool {
        match *crate::env::LINUX_GLIBC_VERSION {
            Some((major, minor)) => major < 2 || (major == 2 && minor < 35),
            None => false, // non-Linux or can't detect, assume modern system
        }
    }

    /// Find precompiled asset from a GitHub repo's releases.
    /// On Linux with glibc < 2.35, prefers the no-YJIT variant (.no_yjit.) which
    /// targets glibc 2.17. Falls back to the standard build if no variant is found.
    async fn find_precompiled_asset_in_repo(
        &self,
        repo: &str,
        version: &str,
        platform: &str,
        prefer_no_yjit: bool,
    ) -> Result<Option<(String, Option<String>)>> {
        let releases = github::list_releases(repo).await?;
        let standard_name = format!("ruby-{}.{}.tar.gz", version, platform);
        let no_yjit_name = format!("ruby-{}.{}.no_yjit.tar.gz", version, platform);

        if prefer_no_yjit {
            debug!("glibc < 2.35 detected, preferring no-YJIT Ruby variant");
        }

        let mut standard_asset = None;
        let mut no_yjit_asset = None;

        for release in &releases {
            for asset in &release.assets {
                if no_yjit_asset.is_none() && asset.name == no_yjit_name {
                    no_yjit_asset =
                        Some((asset.browser_download_url.clone(), asset.digest.clone()));
                } else if standard_asset.is_none() && asset.name == standard_name {
                    standard_asset =
                        Some((asset.browser_download_url.clone(), asset.digest.clone()));
                }
            }
            if no_yjit_asset.is_some() && standard_asset.is_some() {
                break;
            }
        }

        if prefer_no_yjit {
            if no_yjit_asset.is_some() {
                return Ok(no_yjit_asset);
            }
            debug!("no-YJIT variant not found, falling back to standard build");
        }
        Ok(standard_asset)
    }

    /// Resolve precompiled binary URL and checksum for a given version and platform
    async fn resolve_precompiled_url(
        &self,
        version: &str,
        platform: &str,
        prefer_no_yjit: bool,
    ) -> Result<Option<(String, Option<String>)>> {
        let settings = Settings::get();
        let source = &settings.ruby.precompiled_url;

        if source.contains("://") {
            // Full URL template - no checksum available
            Ok(Some((
                self.render_precompiled_url(source, version, platform),
                None,
            )))
        } else {
            // GitHub repo shorthand (default: "jdx/ruby")
            self.find_precompiled_asset_in_repo(source, version, platform, prefer_no_yjit)
                .await
        }
    }

    /// Convert a Ruby GitHub tag name to a version string.
    /// Ruby uses tags like "v3_3_0" for version "3.3.0"
    fn tag_to_version(tag: &str) -> Option<String> {
        // Ruby tags are in format v3_3_0, v3_3_0_preview1, etc.
        let tag = tag.strip_prefix('v')?;
        // Replace underscores with dots, but be careful with preview/rc suffixes
        let re = regex!(r"^(\d+)_(\d+)_(\d+)(.*)$");
        if let Some(caps) = re.captures(tag) {
            let major = &caps[1];
            let minor = &caps[2];
            let patch = &caps[3];
            let suffix = &caps[4];
            // Convert suffix like "_preview1" to "-preview1"
            let suffix = suffix.replace('_', "-");
            Some(format!("{major}.{minor}.{patch}{suffix}"))
        } else {
            None
        }
    }

    /// Fetch created_at timestamps for Ruby versions from GitHub releases
    async fn fetch_ruby_release_dates(&self) -> HashMap<String, String> {
        let mut dates = HashMap::new();
        match github::list_releases("ruby/ruby").await {
            Ok(releases) => {
                for release in releases {
                    if let Some(version) = Self::tag_to_version(&release.tag_name) {
                        dates.insert(version, release.created_at);
                    }
                }
            }
            Err(err) => {
                debug!("Failed to fetch Ruby release dates: {err}");
            }
        }
        dates
    }

    /// Try to install from precompiled binary
    /// Returns Ok(None) if no precompiled version is available for this version/platform
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        let Some(platform) = self.precompiled_platform() else {
            return Ok(None);
        };

        let Some((url, checksum)) = self
            .resolve_precompiled_url(&tv.version, &platform, Self::needs_no_yjit())
            .await?
        else {
            return Ok(None);
        };

        let filename = match url.rsplit('/').next() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => format!("ruby-{}.{}.tar.gz", tv.version, platform),
        };
        let tarball_path = tv.download_path().join(&filename);

        ctx.pr.set_message(format!("download {}", filename));
        HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))
            .await?;

        if let Some(hash_str) = checksum.as_ref().and_then(|c| c.strip_prefix("sha256:")) {
            ctx.pr.set_message(format!("checksum {}", filename));
            hash::ensure_checksum(&tarball_path, hash_str, Some(ctx.pr.as_ref()), "sha256")?;
        }

        // Verify GitHub attestations for precompiled binaries
        self.verify_github_attestations(ctx, &tarball_path, &tv.version)
            .await?;

        ctx.pr.set_message(format!("extract {}", filename));
        let install_path = tv.install_path();
        file::create_dir_all(&install_path)?;
        file::untar(
            &tarball_path,
            &install_path,
            &file::TarOptions {
                format: file::TarFormat::TarGz,
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;

        Ok(Some(tv.clone()))
    }

    /// Verify GitHub artifact attestations for precompiled Ruby binary
    /// Returns Ok(()) if verification succeeds or is skipped (attestations unavailable)
    /// Returns Err if verification is enabled and fails
    async fn verify_github_attestations(
        &self,
        ctx: &InstallContext,
        tarball_path: &std::path::Path,
        version: &str,
    ) -> Result<()> {
        let settings = Settings::get();

        // Check Ruby-specific setting, fall back to global
        let enabled = settings
            .ruby
            .github_attestations
            .unwrap_or(settings.github_attestations);
        if !enabled {
            debug!("GitHub attestations verification disabled for Ruby");
            return Ok(());
        }

        let source = &settings.ruby.precompiled_url;

        // Skip for custom URL templates (not GitHub repos)
        if source.contains("://") {
            debug!("Skipping attestation verification for custom URL template");
            return Ok(());
        }

        let (owner, repo) = match source.split_once('/') {
            Some((o, r)) => (o, r),
            None => {
                warn!("Invalid precompiled_url format: {}", source);
                return Ok(());
            }
        };

        ctx.pr.set_message("verify GitHub attestations".to_string());

        match sigstore_verification::verify_github_attestation(
            tarball_path,
            owner,
            repo,
            env::GITHUB_TOKEN.as_deref(),
            None, // Accept any workflow from repo
        )
        .await
        {
            Ok(true) => {
                ctx.pr
                    .set_message("✓ GitHub attestations verified".to_string());
                debug!(
                    "GitHub attestations verified successfully for ruby@{}",
                    version
                );
                Ok(())
            }
            Ok(false) => Err(eyre!(
                "GitHub attestations verification failed for ruby@{version}\n{ATTESTATION_HELP}"
            )),
            Err(sigstore_verification::AttestationError::NoAttestations) => Err(eyre!(
                "No GitHub attestations found for ruby@{version}\n{ATTESTATION_HELP}"
            )),
            Err(e) => Err(eyre!(
                "GitHub attestations verification failed for ruby@{version}: {e}\n{ATTESTATION_HELP}"
            )),
        }
    }
}

#[async_trait]
impl Backend for RubyPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;
        let settings = Settings::get();

        let mut features = vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }];

        // Report GitHub attestations if enabled for precompiled binaries
        let github_attestations_enabled = settings
            .ruby
            .github_attestations
            .unwrap_or(settings.github_attestations);
        if self.should_try_precompiled() && github_attestations_enabled {
            features.push(SecurityFeature::GithubAttestations {
                signer_workflow: None,
            });
        }

        features
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        timeout::run_with_timeout_async(
            async || {
                if let Err(err) = self.update_build_tool(None).await {
                    warn!("{err}");
                }

                // Fetch Ruby release dates from GitHub in parallel with version list
                let release_dates = self.fetch_ruby_release_dates().await;

                let ruby_build_bin = self.ruby_build_bin();
                let versions = plugins::core::run_fetch_task_with_timeout(move || {
                    let output = cmd!(ruby_build_bin, "--definitions").read()?;
                    let versions: Vec<String> = output
                        .split('\n')
                        .sorted_by_cached_key(|s| regex!(r#"^\d"#).is_match(s)) // show matz ruby first
                        .map(|s| s.to_string())
                        .collect();
                    Ok(versions)
                })?;

                // Map versions to VersionInfo with created_at timestamps
                let version_infos = versions
                    .into_iter()
                    .map(|version| {
                        let created_at = release_dates.get(&version).cloned();
                        VersionInfo {
                            version,
                            created_at,
                            ..Default::default()
                        }
                    })
                    .collect();

                Ok(version_infos)
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".ruby-version".into(), "Gemfile".into()])
    }

    async fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
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
        let settings = Settings::get();
        if settings.ruby.compile.is_none() && !settings.experimental {
            warn_once!(
                "precompiled ruby will be the default in 2026.8.0.\n\
                 To use precompiled binaries now: mise settings ruby.compile=false\n\
                 To keep compiling from source: mise settings ruby.compile=true"
            );
        }

        // Try precompiled if compile=false or experimental + not opted out
        if self.should_try_precompiled()
            && let Some(installed_tv) = self.install_precompiled(ctx, &tv).await?
        {
            hint!(
                "ruby_precompiled",
                "installing precompiled ruby from jdx/ruby\n\
                    if you experience issues, switch to ruby-build by running",
                "mise settings ruby.compile=1"
            );
            self.install_rubygems_hook(&installed_tv)?;
            if let Err(err) = self
                .install_default_gems(&ctx.config, &installed_tv, ctx.pr.as_ref())
                .await
            {
                warn!("failed to install default ruby gems {err:#}");
            }
            return Ok(installed_tv);
        }
        // No precompiled available, fall through to compile from source

        // Compile from source
        if let Err(err) = self.update_build_tool(Some(ctx)).await {
            warn!("ruby build tool update error: {err:#}");
        }
        ctx.pr.set_message("ruby-build".into());
        self.install_cmd(&ctx.config, &tv, ctx.pr.as_ref())
            .await?
            .execute()?;

        self.install_rubygems_hook(&tv)?;
        if let Err(err) = self
            .install_default_gems(&ctx.config, &tv, ctx.pr.as_ref())
            .await
        {
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

    fn resolve_lockfile_options(
        &self,
        _request: &ToolRequest,
        target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let mut opts = BTreeMap::new();
        let settings = Settings::get();
        let is_current_platform = target.is_current();

        // Ruby uses ruby-install vs ruby-build (ruby compiles from source either way)
        // Only include if using non-default ruby-install tool
        let ruby_install = if is_current_platform {
            settings.ruby.ruby_install
        } else {
            false
        };
        if ruby_install {
            opts.insert("ruby_install".to_string(), "true".to_string());
        }

        opts
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Precompiled binary info if enabled
        if self.should_try_precompiled()
            && let Some(platform) = self.precompiled_platform_for_target(target)
            && let Some((url, checksum)) = self
                .resolve_precompiled_url(&tv.version, &platform, false)
                .await?
        {
            return Ok(PlatformInfo {
                url: Some(url),
                checksum,
                size: None,
                url_api: None,
                conda_deps: None,
            });
        }

        // Default: source tarball
        match self.get_ruby_download_info(&tv.version).await? {
            Some((url, checksum)) => Ok(PlatformInfo {
                url: Some(url),
                checksum: Some(checksum),
                size: None,
                url_api: None,
                conda_deps: None,
            }),
            None => Ok(PlatformInfo::default()),
        }
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
    fn test_tag_to_version() {
        // Standard versions
        assert_eq!(
            RubyPlugin::tag_to_version("v3_3_0"),
            Some("3.3.0".to_string())
        );
        assert_eq!(
            RubyPlugin::tag_to_version("v3_2_2"),
            Some("3.2.2".to_string())
        );
        assert_eq!(
            RubyPlugin::tag_to_version("v2_7_8"),
            Some("2.7.8".to_string())
        );

        // Preview and RC versions
        assert_eq!(
            RubyPlugin::tag_to_version("v3_3_0_preview1"),
            Some("3.3.0-preview1".to_string())
        );
        assert_eq!(
            RubyPlugin::tag_to_version("v3_3_0_rc1"),
            Some("3.3.0-rc1".to_string())
        );

        // Invalid tags
        assert_eq!(RubyPlugin::tag_to_version("3_3_0"), None); // Missing 'v' prefix
        assert_eq!(RubyPlugin::tag_to_version("v3_3"), None); // Missing patch version
        assert_eq!(RubyPlugin::tag_to_version("jruby-9.4.0"), None); // Different format
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
