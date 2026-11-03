//! Lockfile support shared by the Unix and Windows Ruby backends.
//!
//! A lockfile can be produced for any platform from any host, so a Windows host
//! has to resolve the precompiled and source archives that `ruby.rs` installs
//! on other platforms even though it never builds them itself.

use std::collections::BTreeMap;

use eyre::{Result, eyre};

use crate::backend::platform_target::PlatformTarget;
use crate::config::{CompilePurpose, Settings, SettingsExt};
use crate::github;
use crate::http::HTTP_FETCH;
use crate::lockfile::{PlatformInfo, ProvenanceType};
use crate::toolset::ToolVersion;

use super::ruby::RubyPlugin;

const RUBY_INDEX_URL: &str = "https://cache.ruby-lang.org/pub/ruby/index.txt";
pub(super) const DEFAULT_RUBY_PRECOMPILED_URL: &str = "jdx/ruby";
const ATTESTATION_HELP: &str = "To disable attestation verification, set MISE_RUBY_GITHUB_ATTESTATIONS=false\n\
    or add `ruby.github_attestations = false` under [settings] in mise.toml";

impl RubyPlugin {
    /// Fetch Ruby source tarball info from cache.ruby-lang.org index
    /// Returns (url, sha256) for the given version
    pub(super) async fn get_ruby_download_info(
        &self,
        version: &str,
    ) -> Result<Option<(String, String)>> {
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

    /// Detect provenance type for precompiled Ruby binaries.
    /// Records GithubAttestations based on settings and URL format without an API probe.
    /// This assumes all releases from the configured precompiled source have attestations;
    /// if a release lacks them, install will fail at verification time.
    pub(super) fn detect_precompiled_provenance(&self) -> Option<ProvenanceType> {
        let settings = Settings::get();
        let enabled = settings
            .ruby
            .github_attestations
            .unwrap_or(settings.github_attestations);
        if !enabled {
            return None;
        }
        let source = &settings.ruby.precompiled_url;
        // Custom URL templates aren't verified via GitHub attestation API
        if source.contains("://") {
            return None;
        }
        // Must be a valid owner/repo format for GitHub attestation verification
        if !source.contains('/') {
            return None;
        }
        Some(ProvenanceType::GithubAttestations)
    }

    pub(super) fn is_default_ruby_source(source: &str) -> bool {
        source == DEFAULT_RUBY_PRECOMPILED_URL
    }

    pub(super) fn use_versions_host_for_precompiled_source(source: &str) -> bool {
        Self::is_default_ruby_source(source)
    }

    /// Check if precompiled binaries should be tried.
    /// Precompiled binaries are the default unless source compilation is explicitly requested.
    pub(super) fn should_try_precompiled(&self) -> bool {
        Settings::get().ruby_compile(CompilePurpose::Inspect) != Some(true)
    }

    /// Get platform identifier for a specific target (used for lockfiles)
    /// Returns platform in jdx/ruby format: "macos", "arm64_linux", or "x86_64_linux"
    pub(super) fn precompiled_platform_for_target(
        &self,
        target: &PlatformTarget,
    ) -> Option<String> {
        let settings = Settings::get();

        // Check for user overrides first
        if let (Some(arch), Some(os)) = (
            settings.ruby.precompiled_arch.as_deref(),
            settings.ruby.precompiled_os.as_deref(),
        ) {
            return Some(format!("{}_{}", arch, os));
        }

        match target.os_name() {
            "macos" => {
                // macOS only supports arm64 and uses "macos" without arch prefix
                match target.arch_name() {
                    "arm64" | "aarch64" => Some("macos".to_string()),
                    _ => None,
                }
            }
            "linux" => {
                // jdx/ruby Linux binaries are glibc-only (manylinux2014); there is no
                // musl build, so a glibc tarball would fail at runtime on Alpine etc.
                if target.libc() == Some("musl") {
                    return None;
                }
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
    pub(super) fn render_precompiled_url(
        &self,
        template: &str,
        version: &str,
        platform: &str,
    ) -> String {
        let settings = Settings::get();
        let (arch, os) = match (
            settings.ruby.precompiled_arch.as_deref(),
            settings.ruby.precompiled_os.as_deref(),
        ) {
            // The overrides form the platform verbatim and either one can
            // contain an underscore, so use them as given.
            (Some(arch), Some(os)) => (arch, os),
            // Built-in platforms are "{arch}_{os}" and the arch can itself
            // contain an underscore (x86_64), so split on the last one. The
            // arm64-only macOS build is named plain "macos".
            _ => platform.rsplit_once('_').unwrap_or(("arm64", platform)),
        };
        template
            .replace("{version}", version)
            .replace("{platform}", platform)
            .replace("{os}", os)
            .replace("{arch}", arch)
    }

    /// Extract the build revision tag from existing lock_platforms URLs.
    ///
    /// URLs look like: `.../releases/download/3.3.11-1/ruby-3.3.11...`
    /// This extracts "3.3.11-1" when the version is "3.3.11".
    pub(super) fn extract_build_revision_from_lock_platforms(
        tv: &ToolVersion,
        version: &str,
    ) -> Option<String> {
        for pi in tv.lock_platforms.values() {
            if let Some(url) = &pi.url {
                // Match `/download/{tag}/` in GitHub release URLs
                let prefix = "/releases/download/";
                if let Some(start) = url.find(prefix) {
                    let after = &url[start + prefix.len()..];
                    if let Some(end) = after.find('/') {
                        let tag = &after[..end];
                        // Check if this is a build revision of the version
                        if Self::is_build_revision_tag(version, tag) {
                            return Some(tag.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    pub(super) fn source_requires_build_revision(source: &str) -> bool {
        Self::is_default_ruby_source(source)
    }

    pub(super) fn is_build_revision_tag(version: &str, tag: &str) -> bool {
        matches!(Self::split_build_revision_tag(tag), (v, Some(_)) if v == version)
    }

    /// Split a release tag into its version and build revision.
    ///
    /// `3.3.11-1` -> `("3.3.11", Some(1))`, `3.3.11` -> `("3.3.11", None)`.
    /// Non-numeric suffixes are part of the version: `3.4.0-preview1` -> `("3.4.0-preview1", None)`.
    pub(super) fn split_build_revision_tag(tag: &str) -> (&str, Option<u32>) {
        match tag.rsplit_once('-') {
            Some((version, revision)) => match revision.parse::<u32>() {
                Ok(revision) => (version, Some(revision)),
                Err(_) => (tag, None),
            },
            None => (tag, None),
        }
    }

    /// Find precompiled asset from a GitHub repo's releases.
    pub(super) async fn find_precompiled_asset_in_repo(
        &self,
        repo: &str,
        version: &str,
        platform: &str,
        locked_build_revision: Option<&str>,
    ) -> Result<Option<(String, Option<String>)>> {
        let requires_build_revision = Self::source_requires_build_revision(repo);
        // The default precompiled repo is explicitly allowlisted by mise-versions.
        // Custom repositories must fetch their release metadata directly from GitHub.
        let use_versions_host = Self::use_versions_host_for_precompiled_source(repo);
        let release = if let Some(tag) = locked_build_revision {
            // Use the exact build revision from the lockfile
            debug!("using locked build revision {tag} for ruby {version}");
            match github::get_release_with_versions_host(repo, tag, use_versions_host).await {
                Ok(r) => r,
                Err(err) => {
                    debug!("locked build revision {tag} not found, finding latest: {err}");
                    match github::get_release_with_build_revision_status(
                        repo,
                        version,
                        use_versions_host,
                    )
                    .await
                    {
                        Ok((r, found_build_revision)) => {
                            if requires_build_revision && !found_build_revision {
                                debug!("no build revision release found for ruby {version}");
                                return Ok(None);
                            }
                            r
                        }
                        Err(err) => {
                            debug!("no precompiled ruby found for {version}: {err}");
                            return Ok(None);
                        }
                    }
                }
            }
        } else {
            match github::get_release_with_build_revision_status(repo, version, use_versions_host)
                .await
            {
                Ok((r, found_build_revision)) => {
                    if requires_build_revision && !found_build_revision {
                        debug!("no build revision release found for ruby {version}");
                        return Ok(None);
                    }
                    r
                }
                Err(err) => {
                    debug!("no precompiled ruby found for {version}: {err}");
                    return Ok(None);
                }
            }
        };
        if release.tag_name != version {
            debug!(
                "using build revision {} for ruby {version}",
                release.tag_name
            );
        }
        let standard_name = format!("ruby-{}.{}.tar.gz", version, platform);

        for asset in &release.assets {
            if asset.name == standard_name {
                return Ok(Some((
                    asset.browser_download_url.clone(),
                    asset.digest.clone(),
                )));
            }
        }

        Ok(None)
    }

    /// Resolve precompiled binary URL and checksum for a given version and platform
    pub(super) async fn resolve_precompiled_url(
        &self,
        version: &str,
        platform: &str,
        locked_build_revision: Option<&str>,
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
            self.find_precompiled_asset_in_repo(source, version, platform, locked_build_revision)
                .await
        }
    }

    /// Verify GitHub artifact attestations for precompiled Ruby binary
    /// Returns Ok(true) if verification succeeds
    /// Returns Ok(false) if verification was skipped (disabled or not applicable)
    /// Returns Err if verification is enabled and fails
    pub(super) async fn verify_github_artifact_attestations(
        &self,
        pr: &dyn crate::ui::progress_report::SingleReport,
        tarball_path: &std::path::Path,
        version: &str,
    ) -> Result<bool> {
        let settings = Settings::get();

        // Check Ruby-specific setting, fall back to global
        let enabled = settings
            .ruby
            .github_attestations
            .unwrap_or(settings.github_attestations);
        if !enabled {
            debug!("GitHub artifact attestations verification disabled for Ruby");
            return Ok(false);
        }

        let source = &settings.ruby.precompiled_url;

        // Skip for custom URL templates (not GitHub repos)
        if source.contains("://") {
            debug!("Skipping GitHub artifact attestation verification for custom URL template");
            return Ok(false);
        }

        let (owner, repo) = match source.split_once('/') {
            Some((o, r)) => (o, r),
            None => {
                warn!("Invalid precompiled_url format: {}", source);
                return Ok(false);
            }
        };

        pr.set_message("verify GitHub artifact attestations".to_string());

        match crate::github::sigstore::verify_attestation(
            tarball_path,
            owner,
            repo,
            None, // Accept any workflow from repo
            None,
            Self::use_versions_host_for_precompiled_source(source),
        )
        .await
        {
            Ok(true) => {
                pr.set_message("✓ GitHub artifact attestations verified".to_string());
                debug!(
                    "GitHub artifact attestations verified successfully for ruby@{}",
                    version
                );
                Ok(true)
            }
            Ok(false) => Err(eyre!(
                "GitHub artifact attestations verification failed for ruby@{version}\n{ATTESTATION_HELP}"
            )),
            Err(crate::github::sigstore::AttestationError::NoAttestations) => Err(eyre!(
                "No GitHub artifact attestations found for ruby@{version}\n{ATTESTATION_HELP}"
            )),
            Err(e) => Err(eyre!(
                "GitHub artifact attestations verification failed for ruby@{version}: {e}\n{ATTESTATION_HELP}"
            )),
        }
    }

    pub(super) fn lock_options(&self, target: &PlatformTarget) -> Result<BTreeMap<String, String>> {
        if target.os_name() == "windows" {
            // Windows uses RubyInstaller2, so ruby-build/precompiled settings do not affect it.
            return Ok(BTreeMap::new());
        }

        let mut opts = BTreeMap::new();
        let settings = Settings::get();
        let ruby = &settings.ruby;
        let try_precompiled = self.should_try_precompiled();

        opts.insert("compile".to_string(), (!try_precompiled).to_string());

        // Ruby uses ruby-install vs ruby-build. The installer and its options
        // can affect the source-built output, including fallback after a
        // missing precompiled binary.
        opts.insert("ruby_install".to_string(), ruby.ruby_install.to_string());
        if ruby.ruby_install {
            if let Some(ruby_install_opts) = ruby.ruby_install_opts.clone() {
                opts.insert("ruby_install_opts".to_string(), ruby_install_opts);
            }
            opts.insert(
                "ruby_install_repo".to_string(),
                ruby.ruby_install_repo.clone(),
            );
        } else {
            if let Some(ruby_build_cli_opts) = ruby.ruby_build_cli_opts.clone() {
                opts.insert("ruby_build_cli_opts".to_string(), ruby_build_cli_opts);
            }
            if let Some(ruby_build_opts) = ruby.ruby_build_opts.clone() {
                opts.insert("ruby_build_opts".to_string(), ruby_build_opts);
            }
            opts.insert("ruby_build_repo".to_string(), ruby.ruby_build_repo.clone());
        }

        if let Some(apply_patches) = ruby.apply_patches.clone() {
            opts.insert("apply_patches".to_string(), apply_patches);
        }

        if try_precompiled {
            opts.insert("precompiled_url".to_string(), ruby.precompiled_url.clone());
            if let Some(precompiled_arch) = ruby.precompiled_arch.clone() {
                opts.insert("precompiled_arch".to_string(), precompiled_arch);
            }
            if let Some(precompiled_os) = ruby.precompiled_os.clone() {
                opts.insert("precompiled_os".to_string(), precompiled_os);
            }
        }

        Ok(opts)
    }

    pub(super) async fn lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Windows uses RubyInstaller2 binaries, not source tarballs
        if target.os_name() == "windows" {
            return super::ruby_common::resolve_rubyinstaller_lock_info(&tv.version).await;
        }

        // Precompiled binary info if enabled
        if self.should_try_precompiled()
            && let Some(platform) = self.precompiled_platform_for_target(target)
            && let Some((url, mut checksum)) = {
                let locked_build_revision =
                    Self::extract_build_revision_from_lock_platforms(tv, &tv.version);
                self.resolve_precompiled_url(
                    &tv.version,
                    &platform,
                    locked_build_revision.as_deref(),
                )
                .await?
            }
        {
            // Detect provenance for precompiled binaries
            let mut provenance = self.detect_precompiled_provenance();
            if provenance.is_some() {
                let artifact =
                    crate::lockfile::generate::download_for_verification(&url, &mut checksum)
                        .await?;
                if !self
                    .verify_github_artifact_attestations(
                        &crate::ui::progress_report::QuietReport::new(),
                        &artifact.path().join("artifact"),
                        &tv.version,
                    )
                    .await?
                {
                    provenance = None;
                }
            }
            return Ok(PlatformInfo {
                url: Some(url),
                checksum,
                provenance,
                ..Default::default()
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
                ..Default::default()
            }),
            None => Ok(PlatformInfo::default()),
        }
    }
}
