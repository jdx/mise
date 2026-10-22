use std::collections::{BTreeMap, HashMap, HashSet};
use std::env::temp_dir;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{Result, WrapErr, bail, eyre};
use itertools::Itertools;
use xx::regex;

use crate::args::BackendArg;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::lock_checksum;
use crate::backend::{Backend, VersionInfo, normalize_idiomatic_contents, strict_metadata};
use crate::cmd::CmdLineRunner;
use crate::config::{CompilePurpose, Config, Settings, SettingsExt};
use crate::duration::DAILY;
use crate::env::PATH_KEY;
use crate::git::{CloneOptions, Git};
use crate::github::{self, GithubRelease};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::lockfile::{PlatformInfo, ProvenanceType};
use crate::plugins::PluginSource;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, hash, plugins, timeout};

/// ruby-lang.org's table of every MRI release, with its date.
const RUBY_LANG_RELEASES_URL: &str =
    "https://raw.githubusercontent.com/ruby/www.ruby-lang.org/master/_data/releases.yml";

/// Maven Central's directory of jruby distributions, one entry per release.
const JRUBY_MAVEN_LISTING_URL: &str = "https://repo1.maven.org/maven2/org/jruby/jruby-dist/";

/// Reads the `- version:` / `date:` pairs of ruby-lang.org's `releases.yml` into timestamps.
fn parse_ruby_lang_releases(index: &str) -> HashMap<String, String> {
    let mut dates = HashMap::new();
    let mut version = None;
    for line in index.lines() {
        if let Some(v) = line.strip_prefix("- version:") {
            version = Some(v.trim().to_string());
        } else if let Some(d) = line.strip_prefix("  date:")
            && let Some(v) = version.take()
            && let Some(ts) = date_to_timestamp(d.trim())
        {
            dates.insert(v, ts);
        }
    }
    dates
}

/// Turns `YYYY-MM-DD` into a UTC timestamp, or `None` for any other shape.
fn date_to_timestamp(date: &str) -> Option<String> {
    date.parse::<jiff::civil::Date>()
        .ok()
        .map(|d| format!("{d}T00:00:00Z"))
}

/// Reads an Apache-style directory listing (`<a href="9.4.9.0/">` then `YYYY-MM-DD HH:MM`) into
/// timestamps keyed by directory name.
fn parse_maven_listing(listing: &str) -> HashMap<String, String> {
    regex!(r#"href="(\d[^/"]*)/"[^\n]*?(\d{4}-\d{2}-\d{2}) (\d{2}:\d{2})"#)
        .captures_iter(listing)
        .map(|c| (c[1].to_string(), format!("{}T{}:00Z", &c[2], &c[3])))
        .collect()
}

/// oracle/truffleruby tags releases `vm-<version>` up to 22.x and `graal-<version>` after.
fn truffleruby_tag_to_version(tag: &str) -> Option<&str> {
    tag.strip_prefix("vm-")
        .or_else(|| tag.strip_prefix("graal-"))
}

#[derive(Debug)]
pub(super) struct RubyPlugin {
    ba: Arc<BackendArg>,
}

impl RubyPlugin {
    pub(super) fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("ruby")),
        }
    }

    fn ruby_build_path(&self) -> PathBuf {
        self.ba.cache_path().join("ruby-build")
    }
    fn ruby_install_path(&self) -> PathBuf {
        self.ba.cache_path().join("ruby-install")
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
                // error getting latest ruby-build version: keep the one
                // installed rather than reinstalling it on every check
                (_cur, Err(err)) => {
                    warn!("failed to get latest ruby-build version: {}", err);
                    return Ok(());
                }
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
                    file::should_strip_components(&temp_archive, file::ExtractionFormat::Zip)?;

                file::unzip(
                    &temp_archive,
                    &tmp,
                    &file::ExtractOptions {
                        strip_components: if strip_components { 1 } else { 0 },
                        ..Default::default()
                    },
                )?;
            }
            PluginSource::Git {
                url: repo_url,
                git_ref,
                subdir,
            } => {
                let git = Git::new(tmp.clone());
                let mut clone_options = CloneOptions::default();
                if let Some(pr) = pr {
                    clone_options = clone_options.pr(pr);
                }
                git.clone(&repo_url, clone_options)?;
                if let Some(ref_) = &git_ref {
                    git.update(Some(ref_.to_string()))?;
                }
                if let Some(subdir) = subdir {
                    let subdir_path = tmp.join(subdir);
                    if !subdir_path.is_dir() {
                        return Err(eyre!(
                            "plugin subdirectory does not exist: {}",
                            file::display_path(&subdir_path)
                        ));
                    }
                    return Ok(subdir_path);
                }
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
        let mut packages = body
            .lines()
            .filter_map(Settings::parse_default_package_line)
            .peekable();
        if packages.peek().is_some() {
            Settings::warn_default_package_file_deprecated(
                "ruby.default_packages_file",
                "ruby gem",
            );
        }
        for package in packages {
            pr.set_message(format!("install default gem: {package}"));
            let gem = self.gem_path(tv);
            let mut cmd = CmdLineRunner::new(gem)
                .with_pr(pr)
                .arg("install")
                .envs(config.env().await?)
                .env_values(tv.install_env());
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
        // Through mise-versions, like every GitHub release lookup: asked on
        // every `ls-remote ruby`, this would otherwise spend the GitHub API's
        // 60 unauthenticated requests an hour.
        let release = github::get_release("rbenv/ruby-build", "latest").await?;
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
        ctx: &'a InstallContext,
        tv: &ToolVersion,
    ) -> Result<CmdLineRunner<'a>> {
        let settings = Settings::get();
        let cmd = if settings.ruby.ruby_install {
            CmdLineRunner::new(self.ruby_install_bin()).args(self.install_args_ruby_install(tv)?)
        } else {
            CmdLineRunner::new(self.ruby_build_bin())
                .args(self.install_args_ruby_build(tv)?)
                .stdin_string(self.fetch_patches().await?)
        };
        // Only with declared `depends`: the dependency env rebuilds PATH without
        // mise install dirs, which would change plain source builds.
        let dependency_env = if ctx
            .dependency_context(&tv.request)
            .await?
            .declarations
            .is_empty()
        {
            BTreeMap::new()
        } else {
            self.dependency_env_for_install(ctx, tv).await?
        };
        Ok(cmd
            .with_pr(ctx.pr.as_ref())
            .envs(dependency_env)
            .envs(ctx.config.env().await?)
            .env_values(tv.install_env()))
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
        if let Some(opts) = &settings.ruby.ruby_build_cli_opts {
            args.extend(shell_words::split(opts)?);
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
        plugins::core::patch_sources(Settings::get().ruby.apply_patches.as_deref())
    }

    /// ruby-build takes every patch as one blob on stdin, so they are concatenated here.
    async fn fetch_patches(&self) -> Result<String> {
        Ok(
            plugins::core::fetch_patch_contents(&self.fetch_patch_sources())
                .await?
                .join("\n"),
        )
    }

    // ===== Precompiled Ruby support =====

    /// Check if precompiled binaries are required, with no fallback to compiling.
    /// `ruby.compile = false` is a strict opt-in: installs fail instead of falling back
    /// to ruby-build, and remote version listings only offer versions that have a
    /// precompiled binary for this platform.
    fn precompiled_only(&self) -> bool {
        Settings::get().ruby_compile(CompilePurpose::Inspect) == Some(false)
    }

    /// Get platform identifier for precompiled binaries
    /// Returns platform in jdx/ruby format: "macos", "arm64_linux", or "x86_64_linux"
    fn precompiled_platform(&self) -> Option<String> {
        self.precompiled_platform_for_target(&PlatformTarget::from_current())
    }

    /// Collect the versions that have a precompiled asset for `platform`.
    ///
    /// An install resolves a version to exactly one release — the highest build revision, or
    /// the base tag when revisions aren't required — so availability is decided by that same
    /// release. A newer revision that is missing an asset for this platform means the version
    /// is not installable even when an older revision has one, which keeps this list from
    /// offering versions [`Self::find_precompiled_asset_in_repo`] would reject.
    fn precompiled_versions_from_releases(
        releases: &[GithubRelease],
        requires_build_revision: bool,
        platform: &str,
    ) -> HashSet<String> {
        let mut best: HashMap<&str, (Option<u32>, bool)> = HashMap::new();
        for release in releases {
            let (version, revision) = Self::split_build_revision_tag(&release.tag_name);
            if requires_build_revision && revision.is_none() {
                continue;
            }
            match best.get(version) {
                // `None` (the base tag) sorts below every numeric revision, matching how
                // installs prefer `3.3.11-2` over `3.3.11-1` over `3.3.11`.
                Some((best_revision, _)) if *best_revision >= revision => continue,
                _ => {}
            }
            let asset_name = format!("ruby-{version}.{platform}.tar.gz");
            let has_asset = release.assets.iter().any(|asset| asset.name == asset_name);
            best.insert(version, (revision, has_asset));
        }
        best.into_iter()
            .filter(|(_, (_, has_asset))| *has_asset)
            .map(|(version, _)| version.to_string())
            .collect()
    }

    /// Cache key context for strict precompiled version listings.
    ///
    /// The filtered list depends on which source is queried and which platform's assets are
    /// looked for, so a change to any of those must not reuse the previous list.
    fn precompiled_cache_context(&self) -> Option<String> {
        if !self.precompiled_only() {
            return None;
        }
        let settings = Settings::get();
        Some(hash::hash_to_str(&(
            "ruby-precompiled",
            &settings.ruby.precompiled_url,
            &settings.ruby.precompiled_arch,
            &settings.ruby.precompiled_os,
            self.precompiled_platform(),
        )))
    }

    /// Restrict a version list to versions that have a precompiled binary for this platform.
    ///
    /// Entries are only removed, never reordered, so `latest` and prefix resolution keep the
    /// same ordering semantics as a source install.
    async fn retain_precompiled_versions(
        &self,
        versions: Vec<VersionInfo>,
    ) -> Result<Vec<VersionInfo>> {
        let settings = Settings::get();
        let source = &settings.ruby.precompiled_url;
        if source.contains("://") {
            // A URL template can't be enumerated, so every version is assumed available.
            return Ok(versions);
        }
        let Some(platform) = self.precompiled_platform() else {
            bail!(
                "no precompiled ruby is available for this platform\n\
                 To compile ruby from source, run: mise settings ruby.compile=true"
            );
        };
        // Only the releases `list_releases` returns are considered. Without
        // MISE_LIST_ALL_VERSIONS that is the most recent page, which is where the
        // precompiled builds live.
        let releases = github::list_releases(source).await?;
        let available = Self::precompiled_versions_from_releases(
            &releases,
            Self::source_requires_build_revision(source),
            &platform,
        );
        Ok(versions
            .into_iter()
            .filter(|v| available.contains(&v.version))
            .collect())
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

    /// Fetch publication timestamps for ruby-build definitions, keyed by definition name.
    ///
    /// MRI comes from ruby/ruby's GitHub releases, which only cover the recent versions, then from
    /// ruby-lang.org's release index for the rest. jruby comes from Maven Central's `jruby-dist`
    /// listing and truffleruby from oracle/truffleruby's GitHub releases. Rolling `-dev`
    /// definitions and the other implementations (rubinius, mruby, ree, ...) publish no release
    /// date, so they stay undated.
    async fn fetch_ruby_release_dates(&self) -> Result<HashMap<String, String>> {
        let mut dates = HashMap::new();
        match github::list_releases("ruby/ruby").await {
            Ok(releases) => {
                for release in releases {
                    if let Some(version) = Self::tag_to_version(&release.tag_name) {
                        dates.insert(version, release.released_at().to_string());
                    }
                }
            }
            Err(err) => {
                if strict_metadata() {
                    return Err(err).wrap_err("failed to fetch Ruby release metadata");
                }
                debug!("Failed to fetch Ruby release dates: {err}");
            }
        }
        match HTTP_FETCH.get_text_cached(RUBY_LANG_RELEASES_URL).await {
            Ok(index) => {
                for (version, date) in parse_ruby_lang_releases(&index) {
                    dates.entry(version).or_insert(date);
                }
            }
            Err(err) => {
                if strict_metadata() {
                    return Err(err).wrap_err("failed to fetch the ruby-lang.org release index");
                }
                debug!("Failed to fetch the ruby-lang.org release index: {err}");
            }
        }
        match HTTP_FETCH.get_html(JRUBY_MAVEN_LISTING_URL).await {
            Ok(listing) => {
                for (version, date) in parse_maven_listing(&listing) {
                    dates.insert(format!("jruby-{version}"), date);
                }
            }
            Err(err) => {
                if strict_metadata() {
                    return Err(err).wrap_err("failed to fetch the jruby release listing");
                }
                debug!("Failed to fetch the jruby release listing: {err}");
            }
        }
        match github::list_releases_including_prereleases("oracle/truffleruby").await {
            Ok(releases) => {
                for release in releases {
                    if let Some(version) = truffleruby_tag_to_version(&release.tag_name) {
                        // The GraalVM build ships from the same release as the native one.
                        for name in ["truffleruby", "truffleruby+graalvm"] {
                            dates.insert(
                                format!("{name}-{version}"),
                                release.released_at().to_string(),
                            );
                        }
                    }
                }
            }
            Err(err) => {
                if strict_metadata() {
                    return Err(err).wrap_err("failed to fetch TruffleRuby release metadata");
                }
                debug!("Failed to fetch TruffleRuby release dates: {err}");
            }
        }
        Ok(dates)
    }

    /// Try to install from precompiled binary
    /// Returns Ok(None) if no precompiled version is available for this version/platform
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        let Some(platform) = self.precompiled_platform() else {
            return Ok(None);
        };

        let locked_build_revision =
            Self::extract_build_revision_from_lock_platforms(tv, &tv.version);
        let Some((url, checksum)) = self
            .resolve_precompiled_url(&tv.version, &platform, locked_build_revision.as_deref())
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
        HTTP.download_file_stored(
            &url,
            &tarball_path,
            lock_checksum(tv, &self.get_platform_key(), &url).as_deref(),
            Some(ctx.pr.as_ref()),
        )
        .await?;

        if let Some(hash_str) = checksum.as_ref().and_then(|c| c.strip_prefix("sha256:")) {
            ctx.pr.set_message(format!("checksum {}", filename));
            hash::ensure_checksum(&tarball_path, hash_str, Some(ctx.pr.as_ref()), "sha256")?;
        }

        // Locked bytes remain checked even when provenance verification is reused.
        let platform_key = PlatformTarget::from_current().to_key();
        let locked_info = tv
            .lock_platforms
            .get(&platform_key)
            .filter(|info| info.url.as_deref() == Some(url.as_str()));
        let reuse_provenance = locked_info.is_some_and(|info| info.has_checksum_and_provenance())
            && !Settings::get().force_provenance_verify();
        if let Some((algorithm, expected)) = locked_info
            .and_then(|info| info.checksum.as_deref())
            .and_then(|checksum| checksum.split_once(':'))
        {
            hash::ensure_checksum(&tarball_path, expected, Some(ctx.pr.as_ref()), algorithm)?;
        }
        let locked_provenance = tv
            .lock_platforms
            .get_mut(&platform_key)
            .and_then(|pi| pi.provenance.take());

        // Verify GitHub artifact attestations for precompiled binaries
        // Returns Ok(true) if verified, Ok(false) if skipped, Err if failed
        let verified = if reuse_provenance {
            if self.detect_precompiled_provenance().is_none() {
                bail!(
                    "lockfile requires Ruby provenance but GitHub attestations are disabled or the precompiled source is not a GitHub repository"
                );
            }
            true
        } else {
            self.verify_github_artifact_attestations(ctx.pr.as_ref(), &tarball_path, &tv.version)
                .await?
        };

        // Record provenance only if verification actually succeeded (not skipped)
        if verified {
            let pi = tv.lock_platforms.entry(platform_key.clone()).or_default();
            pi.provenance = Some(ProvenanceType::GithubAttestations);
        }

        // Enforce lockfile provenance
        if let Some(ref expected) = locked_provenance {
            let got = tv
                .lock_platforms
                .get(&platform_key)
                .and_then(|pi| pi.provenance.as_ref());
            if !got.is_some_and(|g| std::mem::discriminant(g) == std::mem::discriminant(expected)) {
                let got_str = got
                    .map(|g| g.to_string())
                    .unwrap_or_else(|| "no verification".to_string());
                return Err(eyre!(
                    "Lockfile requires {expected} provenance for {tv} but {got_str} was used. \
                     This may indicate a downgrade attack. Enable the corresponding verification setting \
                     or update the lockfile."
                ));
            }
        }

        ctx.pr.set_message(format!("extract {}", filename));
        let install_path = tv.install_path();
        file::create_dir_all(&install_path)?;
        file::untar(
            &tarball_path,
            &install_path,
            file::ExtractionFormat::TarGz,
            &file::ExtractOptions {
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;

        Ok(Some(tv.clone()))
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

        // Report GitHub artifact attestations if enabled for precompiled binaries
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

    async fn remote_version_cache_context(&self, _config: &Arc<Config>) -> Result<Option<String>> {
        // Strict precompiled mode lists a subset of the versions ruby-build knows about, so it
        // must not share a cache — or the shared versions host list — with source installs.
        Ok(self.precompiled_cache_context())
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        timeout::run_with_timeout_async(
            async || {
                if let Err(err) = self.update_build_tool(None).await {
                    warn!("{err}");
                }

                // Fetch Ruby release dates from GitHub in parallel with version list
                let release_dates = self.fetch_ruby_release_dates().await?;

                let ruby_build_bin = self.ruby_build_bin();
                let ruby_build_str = ruby_build_bin.to_string_lossy().to_string();
                let output = crate::cmd::cmd_read_async_inherited_env(
                    &ruby_build_str,
                    &["--definitions"],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                let versions: Vec<String> = output
                    .split('\n')
                    .sorted_by_cached_key(|s| regex!(r#"^\d"#).is_match(s)) // show matz ruby first
                    .map(|s| s.to_string())
                    .collect();

                // Map versions to VersionInfo with created_at timestamps
                let version_infos: Vec<VersionInfo> = versions
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

                if self.precompiled_only() {
                    return self.retain_precompiled_versions(version_infos).await;
                }

                Ok(version_infos)
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn _parse_idiomatic_file(&self, path: &Path) -> Result<Vec<String>> {
        let v = match path.file_name() {
            Some(name) if name == "Gemfile" => super::ruby_common::parse_gemfile(path)?,
            _ => {
                // .ruby-version
                let body = normalize_idiomatic_contents(&file::read_to_string(path)?);
                body.trim()
                    .trim_start_matches("ruby-")
                    .trim_start_matches('v')
                    .to_string()
            }
        };
        if v.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![v])
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let mut tv = tv;
        // Try precompiled unless source compilation was explicitly requested.
        if self.should_try_precompiled() {
            if let Some(installed_tv) = self.install_precompiled(ctx, &mut tv).await? {
                hint!(
                    "ruby_precompiled",
                    "installing precompiled ruby from jdx/ruby\n\
                    if you experience issues, switch to ruby-build by running",
                    "mise settings ruby.compile=true"
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
            // `ruby.compile = false` opts out of source builds entirely, so a missing
            // precompiled binary is an error instead of a silent ruby-build fallback.
            if self.precompiled_only() {
                hint!(
                    "ruby_compile",
                    "To compile ruby from source, run",
                    "mise settings ruby.compile=true"
                );
                match self.precompiled_platform() {
                    Some(platform) => bail!("no precompiled ruby found for {tv} on {platform}"),
                    None => bail!("no precompiled ruby is available for this platform"),
                }
            }
        }
        // No precompiled available, fall through to compile from source

        // Compile from source
        let _ = Settings::get().ruby_compile(CompilePurpose::Install);
        if let Err(err) = self.update_build_tool(Some(ctx)).await {
            warn!("ruby build tool update error: {err:#}");
        }
        ctx.pr.set_message("ruby-build".into());
        self.install_cmd(ctx, &tv).await?.execute()?;

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
    ) -> Result<BTreeMap<String, String>> {
        self.lock_options(target)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        self.lock_info(tv, target).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::ruby_lock::DEFAULT_RUBY_PRECOMPILED_URL;
    use super::*;
    use crate::config::settings::SettingsPartial;
    use crate::platform::Platform;
    use crate::toolset::ToolSource;
    use confique::Layer;
    use pretty_assertions::assert_eq;

    const DEFAULT_RUBY_BUILD_REPO: &str = "https://github.com/rbenv/ruby-build.git";
    const DEFAULT_RUBY_INSTALL_REPO: &str = "https://github.com/postmodern/ruby-install.git";

    fn resolve_ruby_lockfile_options(
        configure_settings: impl FnOnce(&mut SettingsPartial),
    ) -> BTreeMap<String, String> {
        resolve_ruby_lockfile_options_for_target(configure_settings, PlatformTarget::from_current())
    }

    fn resolve_ruby_lockfile_options_for_target(
        configure_settings: impl FnOnce(&mut SettingsPartial),
        target: PlatformTarget,
    ) -> BTreeMap<String, String> {
        let _guard = crate::test::SettingsGuard::lock();
        let mut settings = SettingsPartial::empty();
        configure_settings(&mut settings);
        Settings::reset(Some(settings));

        let backend = RubyPlugin::new();
        let request = ToolRequest::new(backend.ba().clone(), "3.3.0", ToolSource::Unknown).unwrap();
        backend.resolve_lockfile_options(&request, &target).unwrap()
    }

    fn with_ruby_settings<T>(
        configure_settings: impl FnOnce(&mut SettingsPartial),
        f: impl FnOnce(&RubyPlugin) -> T,
    ) -> T {
        let _guard = crate::test::SettingsGuard::lock();
        let mut settings = SettingsPartial::empty();
        configure_settings(&mut settings);
        Settings::reset(Some(settings));

        f(&RubyPlugin::new())
    }

    fn ruby_precompiled_only(compile: Option<bool>) -> bool {
        with_ruby_settings(
            |settings| settings.ruby.compile = compile,
            |backend| backend.precompiled_only(),
        )
    }

    fn ruby_build_args(
        configure_settings: impl FnOnce(&mut SettingsPartial),
    ) -> Result<Vec<String>> {
        ruby_build_args_and_install_path(configure_settings).map(|(args, _)| args)
    }

    /// The ruby-build arguments for 3.3.0 and the directory it is to be installed into,
    /// which depends on the install layout.
    fn ruby_build_args_and_install_path(
        configure_settings: impl FnOnce(&mut SettingsPartial),
    ) -> Result<(Vec<String>, PathBuf)> {
        with_ruby_settings(configure_settings, |backend| {
            let request =
                ToolRequest::new(backend.ba().clone(), "3.3.0", ToolSource::Unknown).unwrap();
            let tv = ToolVersion::new(request, "3.3.0".to_string());
            Ok((backend.install_args_ruby_build(&tv)?, tv.install_path()))
        })
    }

    fn ruby_precompiled_cache_context(
        configure_settings: impl FnOnce(&mut SettingsPartial),
    ) -> Option<String> {
        with_ruby_settings(configure_settings, |backend| {
            backend.precompiled_cache_context()
        })
    }

    fn release(tag: &str, assets: &[&str]) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            published_at: None,
            assets: assets
                .iter()
                .map(|name| crate::github::GithubAsset {
                    from_versions_host: false,
                    name: (*name).to_string(),
                    browser_download_url: format!("https://example.com/{name}"),
                    url: format!("https://api.example.com/{name}"),
                    digest: None,
                    updated_at: None,
                })
                .collect(),
        }
    }

    fn non_current_platform_target() -> PlatformTarget {
        let platform = ["linux-x64", "macos-arm64", "windows-x64"]
            .into_iter()
            .map(|platform| Platform::parse(platform).unwrap())
            .find(|platform| platform != &Platform::current())
            .unwrap();
        PlatformTarget::new(platform)
    }

    #[test]
    fn test_parse_ruby_lang_releases() {
        let index = "\
# comment
- version: 4.1.0-preview1
  date: 2026-10-12
  tag: v4.1.0-preview1
- version: 1.8.7-p374
  date: 2013-06-27
- version: 9.9.9
  date: not-a-date
- version: 2.1.10
  date: 2016-04-01
";
        let dates = parse_ruby_lang_releases(index);
        assert_eq!(dates.len(), 3);
        assert_eq!(dates["4.1.0-preview1"], "2026-10-12T00:00:00Z");
        assert_eq!(dates["1.8.7-p374"], "2013-06-27T00:00:00Z");
        assert_eq!(dates["2.1.10"], "2016-04-01T00:00:00Z");
    }

    #[test]
    fn test_parse_maven_listing() {
        let listing = r#"<a href="../">../</a>
<a href="1.7.27/" title="1.7.27/">1.7.27/</a>                                            2019-05-16 09:12         -
<a href="9.4.9.0/" title="9.4.9.0/">9.4.9.0/</a>                                          2024-04-23 18:30         -
<a href="maven-metadata.xml" title="maven-metadata.xml">maven-metadata.xml</a>            2026-09-21 22:30       1234
"#;
        let dates = parse_maven_listing(listing);
        assert_eq!(dates.len(), 2);
        assert_eq!(dates["1.7.27"], "2019-05-16T09:12:00Z");
        assert_eq!(dates["9.4.9.0"], "2024-04-23T18:30:00Z");
    }

    #[test]
    fn test_truffleruby_tag_to_version() {
        assert_eq!(truffleruby_tag_to_version("vm-22.3.1"), Some("22.3.1"));
        assert_eq!(truffleruby_tag_to_version("graal-24.2.0"), Some("24.2.0"));
        assert_eq!(
            truffleruby_tag_to_version("vm-1.0.0-rc10"),
            Some("1.0.0-rc10")
        );
        assert_eq!(truffleruby_tag_to_version("v1.0.0"), None);
    }

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
    fn test_ruby_precompiled_versions_host_only_for_default_source() {
        assert!(RubyPlugin::use_versions_host_for_precompiled_source(
            DEFAULT_RUBY_PRECOMPILED_URL
        ));
        assert!(!RubyPlugin::use_versions_host_for_precompiled_source(
            "acme/ruby"
        ));
    }

    #[test]
    fn test_ruby_render_precompiled_url_splits_arch_and_os() {
        let template = "https://example.com/{version}/ruby-{arch}-{os}-{platform}.tar.gz";
        with_ruby_settings(
            |_| {},
            |plugin| {
                assert_eq!(
                    plugin.render_precompiled_url(template, "3.3.0", "x86_64_linux"),
                    "https://example.com/3.3.0/ruby-x86_64-linux-x86_64_linux.tar.gz"
                );
                assert_eq!(
                    plugin.render_precompiled_url(template, "3.3.0", "arm64_linux"),
                    "https://example.com/3.3.0/ruby-arm64-linux-arm64_linux.tar.gz"
                );
                assert_eq!(
                    plugin.render_precompiled_url(template, "3.3.0", "macos"),
                    "https://example.com/3.3.0/ruby-arm64-macos-macos.tar.gz"
                );
            },
        );
    }

    #[test]
    fn test_ruby_render_precompiled_url_keeps_override_arch_and_os() {
        let template = "https://example.com/{version}/ruby-{arch}-{os}-{platform}.tar.gz";
        let url = with_ruby_settings(
            |settings| {
                settings.ruby.precompiled_arch = Some("arm64".to_string());
                settings.ruby.precompiled_os = Some("linux_musl".to_string());
            },
            |plugin| {
                let platform = plugin.precompiled_platform().unwrap();
                plugin.render_precompiled_url(template, "3.3.0", &platform)
            },
        );
        assert_eq!(
            url,
            "https://example.com/3.3.0/ruby-arm64-linux_musl-arm64_linux_musl.tar.gz"
        );
    }

    #[test]
    fn test_ruby_default_precompiled_source_requires_build_revision() {
        assert!(RubyPlugin::source_requires_build_revision(
            DEFAULT_RUBY_PRECOMPILED_URL
        ));
        assert!(!RubyPlugin::source_requires_build_revision("acme/ruby"));
    }

    #[test]
    fn test_ruby_split_build_revision_tag() {
        assert_eq!(
            RubyPlugin::split_build_revision_tag("3.3.11-1"),
            ("3.3.11", Some(1))
        );
        assert_eq!(
            RubyPlugin::split_build_revision_tag("3.3.11-12"),
            ("3.3.11", Some(12))
        );
        assert_eq!(
            RubyPlugin::split_build_revision_tag("3.3.11"),
            ("3.3.11", None)
        );
        assert_eq!(
            RubyPlugin::split_build_revision_tag("3.4.0-preview1"),
            ("3.4.0-preview1", None)
        );
    }

    #[test]
    fn test_ruby_precompiled_versions_use_highest_build_revision_only() {
        // An install resolves 3.3.12 to the -2 release, which has no asset for this platform,
        // so 3.3.12 must not be listed even though -1 does have one.
        let releases = vec![
            release("3.3.12-1", &["ruby-3.3.12.x86_64_linux.tar.gz"]),
            release("3.3.12-2", &["ruby-3.3.12.macos.tar.gz"]),
        ];
        let versions =
            RubyPlugin::precompiled_versions_from_releases(&releases, true, "x86_64_linux");
        assert!(versions.is_empty());

        // Release order must not change the answer.
        let reversed = releases.into_iter().rev().collect_vec();
        let versions =
            RubyPlugin::precompiled_versions_from_releases(&reversed, true, "x86_64_linux");
        assert!(versions.is_empty());
        let versions = RubyPlugin::precompiled_versions_from_releases(&reversed, true, "macos");
        assert_eq!(versions, HashSet::from(["3.3.12".to_string()]));
    }

    #[test]
    fn test_ruby_precompiled_cache_context_tracks_source_and_platform() {
        assert_eq!(ruby_precompiled_cache_context(|_| {}), None);

        let default_source = ruby_precompiled_cache_context(|settings| {
            settings.ruby.compile = Some(false);
        });
        let same = ruby_precompiled_cache_context(|settings| {
            settings.ruby.compile = Some(false);
        });
        assert!(default_source.is_some());
        assert_eq!(default_source, same);

        let custom_source = ruby_precompiled_cache_context(|settings| {
            settings.ruby.compile = Some(false);
            settings.ruby.precompiled_url = Some("acme/ruby".to_string());
        });
        assert_ne!(default_source, custom_source);

        let custom_platform = ruby_precompiled_cache_context(|settings| {
            settings.ruby.compile = Some(false);
            settings.ruby.precompiled_arch = Some("arm64".to_string());
            settings.ruby.precompiled_os = Some("linux".to_string());
        });
        assert_ne!(default_source, custom_platform);
        assert_ne!(custom_source, custom_platform);
    }

    #[test]
    fn test_ruby_precompiled_versions_require_build_revision_for_default_source() {
        let releases = vec![
            release("3.3.11", &["ruby-3.3.11.x86_64_linux.tar.gz"]),
            release("3.3.12-1", &["ruby-3.3.12.x86_64_linux.tar.gz"]),
            release("4.0.6-2", &["ruby-4.0.6.macos.tar.gz"]),
        ];

        // The default source only ships usable binaries under build revision tags, so the
        // plain `3.3.11` tag is not offered.
        let versions =
            RubyPlugin::precompiled_versions_from_releases(&releases, true, "x86_64_linux");
        assert_eq!(versions, HashSet::from(["3.3.12".to_string()]));

        // Custom sources have no build revision requirement.
        let versions =
            RubyPlugin::precompiled_versions_from_releases(&releases, false, "x86_64_linux");
        assert_eq!(
            versions,
            HashSet::from(["3.3.11".to_string(), "3.3.12".to_string()])
        );

        // Only assets for the requested platform count.
        let versions = RubyPlugin::precompiled_versions_from_releases(&releases, true, "macos");
        assert_eq!(versions, HashSet::from(["4.0.6".to_string()]));
        let versions =
            RubyPlugin::precompiled_versions_from_releases(&releases, true, "arm64_linux");
        assert!(versions.is_empty());
    }

    #[test]
    fn test_ruby_precompiled_only_requires_explicit_false() {
        assert!(!ruby_precompiled_only(None));
        assert!(!ruby_precompiled_only(Some(true)));
        assert!(ruby_precompiled_only(Some(false)));
    }

    fn ruby_precompiled_platform_for_target(
        configure_settings: impl FnOnce(&mut SettingsPartial),
        platform: &str,
    ) -> Option<String> {
        with_ruby_settings(configure_settings, |backend| {
            backend.precompiled_platform_for_target(&PlatformTarget::new(
                Platform::parse(platform).unwrap(),
            ))
        })
    }

    #[test]
    fn test_ruby_precompiled_platform_skips_musl_linux() {
        assert_eq!(
            ruby_precompiled_platform_for_target(|_| {}, "linux-x64").as_deref(),
            Some("x86_64_linux")
        );
        assert_eq!(
            ruby_precompiled_platform_for_target(|_| {}, "linux-arm64").as_deref(),
            Some("arm64_linux")
        );
        assert_eq!(
            ruby_precompiled_platform_for_target(|_| {}, "macos-arm64").as_deref(),
            Some("macos")
        );

        // jdx/ruby has no musl builds, so a musl target has no precompiled platform;
        // installs fall back to ruby-build and lockfiles record the source tarball
        assert_eq!(
            ruby_precompiled_platform_for_target(|_| {}, "linux-x64-musl"),
            None
        );
        assert_eq!(
            ruby_precompiled_platform_for_target(|_| {}, "linux-arm64-musl"),
            None
        );

        // Explicit overrides bypass auto-detection, including the musl skip
        assert_eq!(
            ruby_precompiled_platform_for_target(
                |settings| {
                    settings.ruby.precompiled_arch = Some("x86_64".to_string());
                    settings.ruby.precompiled_os = Some("linux".to_string());
                },
                "linux-x64-musl"
            )
            .as_deref(),
            Some("x86_64_linux")
        );
    }

    #[test]
    fn test_ruby_precompiled_platform_skips_musl_current() {
        let platform = with_ruby_settings(
            |settings| {
                settings.os = Some("linux".to_string());
                settings.arch = Some("x64".to_string());
                settings.libc = Some("musl".to_string());
            },
            |backend| backend.precompiled_platform(),
        );
        assert_eq!(platform, None);
    }

    #[test]
    fn test_ruby_build_revision_tags_are_numeric_suffixes() {
        assert!(RubyPlugin::is_build_revision_tag("3.3.11", "3.3.11-1"));
        assert!(RubyPlugin::is_build_revision_tag("3.3.11", "3.3.11-10"));
        assert!(!RubyPlugin::is_build_revision_tag("3.3.11", "3.3.11"));
        assert!(!RubyPlugin::is_build_revision_tag(
            "3.3.11",
            "3.3.11-preview1"
        ));
        assert!(!RubyPlugin::is_build_revision_tag("3.3.11", "3.3.10-1"));
    }

    #[test]
    fn test_ruby_lockfile_options_include_precompiled_inputs() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.compile = Some(false);
            settings.ruby.precompiled_url = Some("acme/ruby".to_string());
            settings.ruby.precompiled_arch = Some("arm64".to_string());
            settings.ruby.precompiled_os = Some("linux".to_string());
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                ("compile".to_string(), "false".to_string()),
                ("precompiled_arch".to_string(), "arm64".to_string()),
                ("precompiled_os".to_string(), "linux".to_string()),
                ("precompiled_url".to_string(), "acme/ruby".to_string()),
                (
                    "ruby_build_repo".to_string(),
                    DEFAULT_RUBY_BUILD_REPO.to_string(),
                ),
                ("ruby_install".to_string(), "false".to_string()),
            ])
        );
    }

    #[test]
    fn test_ruby_lock_info_url_uses_precompiled_overrides() {
        let _guard = crate::test::SettingsGuard::lock();
        let mut settings = SettingsPartial::empty();
        settings.ruby.compile = Some(false);
        settings.ruby.precompiled_url =
            Some("https://example.com/ruby-{version}-{platform}.tar.gz".to_string());
        settings.ruby.precompiled_arch = Some("arm64".to_string());
        settings.ruby.precompiled_os = Some("linux".to_string());
        Settings::reset(Some(settings));

        let backend = RubyPlugin::new();
        let request = ToolRequest::new(backend.ba().clone(), "3.3.0", ToolSource::Unknown).unwrap();
        let target = PlatformTarget::new(Platform::parse("macos-arm64").unwrap());
        let opts = backend.resolve_lockfile_options(&request, &target).unwrap();
        let tv = ToolVersion::new(request, "3.3.0".to_string());
        let info = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(backend.resolve_lock_info(&tv, &target))
            .unwrap();

        assert_eq!(
            opts.get("precompiled_arch").map(String::as_str),
            Some("arm64")
        );
        assert_eq!(
            opts.get("precompiled_os").map(String::as_str),
            Some("linux")
        );
        assert_eq!(
            info.url.as_deref(),
            Some("https://example.com/ruby-3.3.0-arm64_linux.tar.gz")
        );
    }

    #[test]
    fn test_ruby_lockfile_options_include_source_build_inputs() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.compile = Some(true);
            settings.ruby.ruby_build_cli_opts = Some("--keep".to_string());
            settings.ruby.ruby_build_opts = Some("--enable-yjit".to_string());
            settings.ruby.apply_patches = Some("https://example.com/ruby.patch".to_string());
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                (
                    "apply_patches".to_string(),
                    "https://example.com/ruby.patch".to_string(),
                ),
                ("compile".to_string(), "true".to_string()),
                (
                    "ruby_build_repo".to_string(),
                    DEFAULT_RUBY_BUILD_REPO.to_string(),
                ),
                ("ruby_build_cli_opts".to_string(), "--keep".to_string()),
                ("ruby_build_opts".to_string(), "--enable-yjit".to_string()),
                ("ruby_install".to_string(), "false".to_string()),
            ])
        );
    }

    #[test]
    fn test_ruby_build_cli_and_configure_option_order() {
        let (args, install_path) = ruby_build_args_and_install_path(|settings| {
            settings.ruby.apply_patches = Some("https://example.com/ruby.patch".to_string());
            settings.ruby.ruby_build_cli_opts =
                Some("--keep --definitions='/path with spaces'".to_string());
            settings.ruby.ruby_build_opts =
                Some("--enable-yjit --with-openssl-dir='/opt with spaces'".to_string());
        })
        .unwrap();

        assert_eq!(
            args[0..4],
            [
                "--patch",
                "--keep",
                "--definitions=/path with spaces",
                "3.3.0"
            ]
        );
        assert_eq!(Path::new(&args[4]), install_path);
        assert_eq!(
            args[5..],
            ["--", "--enable-yjit", "--with-openssl-dir=/opt with spaces"]
        );
    }

    #[test]
    fn test_ruby_build_cli_opts_reject_invalid_shell_words() {
        let result = ruby_build_args(|settings| {
            settings.ruby.ruby_build_cli_opts = Some("--keep '".to_string());
        });

        assert!(result.is_err());
    }

    #[test]
    fn test_ruby_lockfile_options_include_precompiled_default() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.precompiled_url = Some("acme/ruby".to_string());
            settings.ruby.precompiled_arch = Some("arm64".to_string());
            settings.ruby.precompiled_os = Some("linux".to_string());
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                ("compile".to_string(), "false".to_string()),
                ("precompiled_arch".to_string(), "arm64".to_string()),
                ("precompiled_os".to_string(), "linux".to_string()),
                ("precompiled_url".to_string(), "acme/ruby".to_string()),
                (
                    "ruby_build_repo".to_string(),
                    DEFAULT_RUBY_BUILD_REPO.to_string(),
                ),
                ("ruby_install".to_string(), "false".to_string()),
            ])
        );
    }

    #[test]
    fn test_ruby_lockfile_options_include_precompiled_defaults() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.compile = Some(false);
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                ("compile".to_string(), "false".to_string()),
                (
                    "precompiled_url".to_string(),
                    DEFAULT_RUBY_PRECOMPILED_URL.to_string(),
                ),
                (
                    "ruby_build_repo".to_string(),
                    DEFAULT_RUBY_BUILD_REPO.to_string(),
                ),
                ("ruby_install".to_string(), "false".to_string()),
            ])
        );
    }

    #[test]
    fn test_ruby_lockfile_options_include_source_inputs_for_non_current_targets() {
        let opts = resolve_ruby_lockfile_options_for_target(
            |settings| {
                settings.ruby.compile = Some(true);
                settings.ruby.ruby_build_opts = Some("--enable-yjit".to_string());
                settings.ruby.apply_patches = Some("https://example.com/ruby.patch".to_string());
            },
            non_current_platform_target(),
        );

        assert_eq!(
            opts,
            BTreeMap::from([
                (
                    "apply_patches".to_string(),
                    "https://example.com/ruby.patch".to_string(),
                ),
                ("compile".to_string(), "true".to_string()),
                (
                    "ruby_build_repo".to_string(),
                    DEFAULT_RUBY_BUILD_REPO.to_string(),
                ),
                ("ruby_build_opts".to_string(), "--enable-yjit".to_string()),
                ("ruby_install".to_string(), "false".to_string()),
            ])
        );
    }

    #[test]
    fn test_ruby_lockfile_options_skip_build_inputs_for_windows_targets() {
        let opts = resolve_ruby_lockfile_options_for_target(
            |settings| {
                settings.ruby.compile = Some(false);
                settings.ruby.ruby_build_opts = Some("--enable-yjit".to_string());
                settings.ruby.apply_patches = Some("https://example.com/ruby.patch".to_string());
                settings.ruby.precompiled_url = Some("acme/ruby".to_string());
                settings.ruby.precompiled_arch = Some("arm64".to_string());
                settings.ruby.precompiled_os = Some("linux".to_string());
            },
            PlatformTarget::new(Platform::parse("windows-x64").unwrap()),
        );

        assert_eq!(opts, BTreeMap::new());
    }

    #[test]
    fn test_ruby_lockfile_options_include_source_fallback_inputs() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.compile = Some(false);
            settings.ruby.ruby_build_opts = Some("--enable-yjit".to_string());
            settings.ruby.apply_patches = Some("https://example.com/ruby.patch".to_string());
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                (
                    "apply_patches".to_string(),
                    "https://example.com/ruby.patch".to_string(),
                ),
                ("compile".to_string(), "false".to_string()),
                (
                    "precompiled_url".to_string(),
                    DEFAULT_RUBY_PRECOMPILED_URL.to_string(),
                ),
                (
                    "ruby_build_repo".to_string(),
                    DEFAULT_RUBY_BUILD_REPO.to_string(),
                ),
                ("ruby_build_opts".to_string(), "--enable-yjit".to_string()),
                ("ruby_install".to_string(), "false".to_string()),
            ])
        );
    }

    #[test]
    fn test_ruby_lockfile_options_include_ruby_install_inputs() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.compile = Some(true);
            settings.ruby.ruby_install = Some(true);
            settings.ruby.ruby_install_opts = Some("--no-reinstall".to_string());
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                ("compile".to_string(), "true".to_string()),
                ("ruby_install".to_string(), "true".to_string()),
                (
                    "ruby_install_opts".to_string(),
                    "--no-reinstall".to_string()
                ),
                (
                    "ruby_install_repo".to_string(),
                    DEFAULT_RUBY_INSTALL_REPO.to_string(),
                ),
            ])
        );
    }

    #[test]
    fn test_ruby_lockfile_options_include_ruby_install_inputs_with_patches() {
        let opts = resolve_ruby_lockfile_options(|settings| {
            settings.ruby.compile = Some(true);
            settings.ruby.ruby_install = Some(true);
            settings.ruby.apply_patches = Some("https://example.com/ruby.patch".to_string());
        });

        assert_eq!(
            opts,
            BTreeMap::from([
                (
                    "apply_patches".to_string(),
                    "https://example.com/ruby.patch".to_string(),
                ),
                ("compile".to_string(), "true".to_string()),
                ("ruby_install".to_string(), "true".to_string()),
                (
                    "ruby_install_repo".to_string(),
                    DEFAULT_RUBY_INSTALL_REPO.to_string(),
                ),
            ])
        );
    }
}
