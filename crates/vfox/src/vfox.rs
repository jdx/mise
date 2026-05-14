use indexmap::IndexMap;
use itertools::Itertools;
use reqwest::Url;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, mpsc};
use tempfile::TempDir;
use xx::file;

use crate::error::Result;
use crate::hooks::available::AvailableVersion;
use crate::hooks::backend_exec_env::BackendExecEnvContext;
use crate::hooks::backend_install::BackendInstallContext;
use crate::hooks::backend_list_versions::BackendListVersionsContext;
use crate::hooks::env_keys::{EnvKey, EnvKeysContext};
use crate::hooks::mise_env::{MiseEnvContext, MiseEnvResult};
use crate::hooks::mise_path::MisePathContext;
use crate::hooks::parse_legacy_file::ParseLegacyFileResponse;
use crate::hooks::post_install::PostInstallContext;
use crate::hooks::pre_install::{PreInstall, PreInstallAttestation, VerifiedAttestation};
use crate::hooks::pre_uninstall::PreUninstallContext;
use crate::http::{CLIENT, retry_async};
use crate::metadata::Metadata;
use crate::plugin::Plugin;
use crate::registry;
use crate::sdk_info::SdkInfo;

/// Install result containing optional checksum used for verification
#[derive(Debug, Default)]
pub struct InstallResult {
    /// The SHA256 checksum if one was provided and verified
    pub sha256: Option<String>,
    /// The type of attestation that was successfully verified (if any)
    pub verified_attestation: Option<VerifiedAttestation>,
    /// Whether a checksum (sha256/sha512) was verified during install
    pub checksum_verified: bool,
}

pub struct Vfox {
    pub runtime_version: String,
    pub install_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub download_dir: PathBuf,
    /// When true, skip attestation verification during install if the plugin also provides
    /// a sha256/sha512 checksum (so checksum integrity still applies). If the plugin has
    /// no checksums, attestation always runs regardless of this flag.
    /// Set by the caller when the lockfile already has a provenance entry from a prior install.
    pub skip_verification: bool,
    /// Optional environment to set on plugins before executing backend hooks.
    /// When set, `plugin.set_cmd_env()` is called so Lua `cmd.exec()` uses this env
    /// instead of inheriting the process environment. This allows dependency tools'
    /// bin paths to be on PATH during version resolution and installation.
    pub cmd_env: Option<IndexMap<String, String>>,
    /// Shell command used by Lua `cmd.exec()`.
    pub default_inline_shell: Option<Vec<String>>,
    /// Optional GitHub token for Lua http requests to GitHub API endpoints.
    pub github_token: Option<String>,
    /// Optional lazy resolver for the GitHub token. When set, the token is only
    /// resolved if a Lua plugin actually makes an HTTP request to a GitHub API
    /// URL — avoiding e.g. spawning `github.credential_command` for innocuous
    /// commands like `mise hook-env` that never need a token. Takes precedence
    /// over `github_token` when both are set.
    pub github_token_resolver: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    /// Optional runtime env type (`gnu` or `musl`) exposed to plugin hooks.
    pub runtime_env_type: Option<String>,
    log_tx: Option<mpsc::Sender<String>>,
}

impl std::fmt::Debug for Vfox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vfox")
            .field("runtime_version", &self.runtime_version)
            .field("install_dir", &self.install_dir)
            .field("plugin_dir", &self.plugin_dir)
            .field("cache_dir", &self.cache_dir)
            .field("download_dir", &self.download_dir)
            .field("skip_verification", &self.skip_verification)
            .field("cmd_env", &self.cmd_env)
            .field("github_token", &self.github_token.as_deref().map(|_| "***"))
            .field(
                "github_token_resolver",
                &self.github_token_resolver.as_ref().map(|_| "<closure>"),
            )
            .field("runtime_env_type", &self.runtime_env_type)
            .finish_non_exhaustive()
    }
}

impl Vfox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn log_subscribe(&mut self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.log_tx = Some(tx);
        rx
    }

    fn log_emit(&self, msg: String) {
        if let Some(tx) = &self.log_tx {
            let _ = tx.send(msg);
        }
    }

    pub fn list_available_sdks() -> &'static BTreeMap<String, Url> {
        registry::list_sdks()
    }

    pub async fn list_available_versions(&self, sdk: &str) -> Result<Vec<AvailableVersion>> {
        let sdk = self.get_sdk_with_env(sdk)?;
        sdk.available_async().await
    }

    pub fn list_installed_versions(&self, sdk: &str) -> Result<Vec<SdkInfo>> {
        let path = self.install_dir.join(sdk);
        if !path.exists() {
            return Ok(Default::default());
        }
        let sdk = self.get_sdk(sdk)?;
        let versions = xx::file::ls(&path)?;
        versions
            .into_iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|f| f.to_str())
                    .map(|s| s.to_string())
            })
            .sorted()
            .map(|version| {
                let path = path.join(&version);
                sdk.sdk_info(version, path)
            })
            .collect::<Result<_>>()
    }
    pub fn list_sdks(&self) -> Result<Vec<Plugin>> {
        if !self.plugin_dir.exists() {
            return Ok(Default::default());
        }
        let plugins = xx::file::ls(&self.plugin_dir)?;
        plugins
            .into_iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|f| f.to_str())
                    .map(|s| s.to_string())
            })
            .sorted()
            .map(|name| self.get_sdk(&name))
            .collect()
    }

    pub fn get_sdk(&self, name: &str) -> Result<Plugin> {
        let mut plugin = Plugin::from_name_or_dir(name, &self.plugin_dir.join(name))?;
        plugin.runtime_env_type = self.runtime_env_type.clone();
        self.set_cmd_shell(&plugin)?;
        Ok(plugin)
    }

    fn get_sdk_with_env(&self, name: &str) -> Result<Plugin> {
        let plugin = self.get_sdk(name)?;
        if let Some(env) = &self.cmd_env {
            plugin.set_cmd_env(env)?;
        }
        self.set_github_token(&plugin)?;
        Ok(plugin)
    }

    fn set_cmd_shell(&self, plugin: &Plugin) -> Result<()> {
        if let Some(shell) = &self.default_inline_shell {
            plugin.set_cmd_shell(shell)?;
        }
        Ok(())
    }

    fn set_github_token(&self, plugin: &Plugin) -> Result<()> {
        // Both are registered when both are set; the Lua-side `github_token()`
        // tries the resolver first and falls back to the string. That matches
        // the documented precedence on `github_token_resolver`.
        if let Some(token) = &self.github_token {
            plugin.set_github_token(token)?;
        }
        if let Some(resolver) = &self.github_token_resolver {
            plugin.set_github_token_resolver(resolver.clone())?;
        }
        Ok(())
    }

    pub fn install_plugin(&self, sdk: &str) -> Result<Plugin> {
        // Check filesystem first - allows user to override embedded plugins
        let plugin_dir = self.plugin_dir.join(sdk);
        if plugin_dir.exists() {
            let mut plugin = Plugin::from_dir(&plugin_dir)?;
            plugin.runtime_env_type = self.runtime_env_type.clone();
            return Ok(plugin);
        }

        // Fall back to embedded plugin if available
        if let Some(embedded) = crate::embedded_plugins::get_embedded_plugin(sdk) {
            let mut plugin = Plugin::from_embedded(sdk, embedded)?;
            plugin.runtime_env_type = self.runtime_env_type.clone();
            return Ok(plugin);
        }

        // Otherwise install from registry
        let url = registry::sdk_url(sdk).ok_or_else(|| format!("Unknown SDK: {sdk}"))?;
        self.install_plugin_from_url(url)
    }

    pub fn install_plugin_from_url(&self, url: &Url) -> Result<Plugin> {
        let sdk = url
            .path_segments()
            .and_then(|mut s| {
                let filename = s.next_back().unwrap();
                filename
                    .strip_prefix("vfox-")
                    .map(|s| s.to_string())
                    .or_else(|| Some(filename.to_string()))
            })
            .ok_or("No filename in URL")?;
        let plugin_dir = self.plugin_dir.join(&sdk);
        if !plugin_dir.exists() {
            debug!("Installing plugin {sdk}");
            xx::git::clone(url.as_ref(), &plugin_dir, &Default::default())?;
        }
        let mut plugin = Plugin::from_dir(&plugin_dir)?;
        plugin.runtime_env_type = self.runtime_env_type.clone();
        Ok(plugin)
    }

    pub fn uninstall_plugin(&self, sdk: &str) -> Result<()> {
        let plugin_dir = self.plugin_dir.join(sdk);
        if plugin_dir.exists() {
            file::remove_dir_all(&plugin_dir)?;
        }
        Ok(())
    }

    pub async fn install<ID: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
    ) -> Result<InstallResult> {
        self.install_plugin(sdk)?;
        let sdk = self.get_sdk_with_env(sdk)?;
        let pre_install = sdk.pre_install(version).await?;
        let install_dir = install_dir.as_ref();
        trace!("{pre_install:?}");
        let mut verified_attestation = None;
        let mut checksum_verified = false;
        if let Some(url) = pre_install.url.as_ref().map(|s| Url::from_str(s)) {
            let file = self.download(&url?, &sdk, version).await?;
            verified_attestation = self.verify(&pre_install, &file).await?;
            self.extract(&file, install_dir)?;
            // Note: sha1/md5 intentionally excluded — they are unimplemented! and
            // not considered strong enough to satisfy the checksum_verified semantic.
            checksum_verified = pre_install.sha256.is_some() || pre_install.sha512.is_some();
        }

        if sdk.get_metadata()?.hooks.contains("post_install") {
            let sdk_info = sdk.sdk_info(version.to_string(), install_dir.to_path_buf())?;
            sdk.post_install(PostInstallContext {
                root_path: install_dir.to_path_buf(),
                runtime_version: self.runtime_version.clone(),
                sdk_info: BTreeMap::from([(sdk_info.name.clone(), sdk_info)]),
            })
            .await?;
        }
        Ok(InstallResult {
            sha256: pre_install.sha256,
            verified_attestation,
            checksum_verified,
        })
    }

    pub async fn pre_uninstall<ID: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
    ) -> Result<()> {
        let sdk = self.get_sdk_with_env(sdk)?;
        if sdk.get_metadata()?.hooks.contains("pre_uninstall") {
            let sdk_info = sdk.sdk_info(version.to_string(), install_dir.as_ref().to_path_buf())?;
            sdk.pre_uninstall(PreUninstallContext {
                main: sdk_info.clone(),
                sdk_info: BTreeMap::from([(sdk_info.name.clone(), sdk_info)]),
            })
            .await?;
        }
        Ok(())
    }

    pub fn uninstall(&self, sdk: &str, version: &str) -> Result<()> {
        let path = self.install_dir.join(sdk).join(version);
        file::remove_dir_all(&path)?;
        Ok(())
    }

    pub async fn pre_install_for_platform(
        &self,
        sdk: &str,
        version: &str,
        os: &str,
        arch: &str,
    ) -> Result<PreInstall> {
        let sdk = self.get_sdk_with_env(sdk)?;
        sdk.pre_install_for_platform(version, os, arch).await
    }

    /// Returns the download URL and the highest-priority verified attestation type
    /// declared by the plugin for the given platform, without performing actual
    /// verification or installation.
    pub async fn pre_install_provenance_for_platform(
        &self,
        sdk: &str,
        version: &str,
        os: &str,
        arch: &str,
    ) -> Result<(Option<String>, Option<VerifiedAttestation>)> {
        let pre = self
            .pre_install_for_platform(sdk, version, os, arch)
            .await?;
        let att = pre.attestation.and_then(attestation_to_verified);
        // Note: pre.sha256 / pre.sha512 are intentionally not returned here;
        // checksum verification only happens during `mise install`, not `mise lock`.
        Ok((pre.url, att))
    }

    pub async fn metadata(&self, sdk: &str) -> Result<Metadata> {
        self.get_sdk(sdk)?.get_metadata()
    }

    pub async fn env_keys<T: serde::Serialize>(
        &self,
        sdk: &str,
        version: &str,
        options: T,
    ) -> Result<Vec<EnvKey>> {
        debug!("Getting env keys for {sdk} version {version}");
        let sdk = self.get_sdk_with_env(sdk)?;
        let sdk_info = sdk.sdk_info(
            version.to_string(),
            self.install_dir.join(&sdk.name).join(version),
        )?;
        let ctx = EnvKeysContext {
            args: vec![],
            version: version.to_string(),
            path: sdk_info.path.clone(),
            sdk_info: BTreeMap::from([(sdk_info.name.clone(), sdk_info.clone())]),
            main: sdk_info,
            options,
        };
        sdk.env_keys(ctx).await
    }

    pub async fn mise_env<T: serde::Serialize>(
        &self,
        sdk: &str,
        opts: T,
        env: &indexmap::IndexMap<String, String>,
        config_root: Option<&str>,
    ) -> Result<MiseEnvResult> {
        let plugin = self.get_sdk(sdk)?;
        if !plugin.get_metadata()?.hooks.contains("mise_env") {
            return Ok(MiseEnvResult::default());
        }
        if log::log_enabled!(log::Level::Trace) {
            if let Some(path) = env.get("PATH") {
                trace!("[vfox:{sdk}] mise_env PATH: {path}");
            } else {
                trace!("[vfox:{sdk}] mise_env: no PATH in env");
            }
        }
        plugin.set_cmd_env(env)?;
        self.set_github_token(&plugin)?;
        let ctx = MiseEnvContext {
            args: vec![],
            options: opts,
            config_root: config_root.map(|s| s.to_string()),
        };
        plugin.mise_env(ctx).await
    }

    pub async fn backend_list_versions(
        &self,
        sdk: &str,
        tool: &str,
        options: IndexMap<String, toml::Value>,
    ) -> Result<Vec<String>> {
        let plugin = self.get_sdk_with_env(sdk)?;
        let ctx = BackendListVersionsContext {
            tool: tool.to_string(),
            options,
        };
        plugin.backend_list_versions(ctx).await.map(|r| r.versions)
    }

    pub async fn backend_install(
        &self,
        sdk: &str,
        tool: &str,
        version: &str,
        install_path: PathBuf,
        download_path: PathBuf,
        options: IndexMap<String, toml::Value>,
    ) -> Result<()> {
        let plugin = self.get_sdk_with_env(sdk)?;
        let ctx = BackendInstallContext {
            tool: tool.to_string(),
            version: version.to_string(),
            install_path,
            download_path,
            options,
        };
        plugin.backend_install(ctx).await?;
        Ok(())
    }

    pub async fn backend_exec_env(
        &self,
        sdk: &str,
        tool: &str,
        version: &str,
        install_path: PathBuf,
        options: IndexMap<String, toml::Value>,
    ) -> Result<Vec<EnvKey>> {
        let plugin = self.get_sdk_with_env(sdk)?;
        let ctx = BackendExecEnvContext {
            tool: tool.to_string(),
            version: version.to_string(),
            install_path,
            options,
        };
        plugin.backend_exec_env(ctx).await.map(|r| r.env_vars)
    }

    pub async fn mise_path<T: serde::Serialize>(
        &self,
        sdk: &str,
        opts: T,
        env: &indexmap::IndexMap<String, String>,
        config_root: Option<&str>,
    ) -> Result<Vec<String>> {
        let plugin = self.get_sdk(sdk)?;
        if !plugin.get_metadata()?.hooks.contains("mise_path") {
            return Ok(vec![]);
        }
        plugin.set_cmd_env(env)?;
        self.set_github_token(&plugin)?;
        let ctx = MisePathContext {
            args: vec![],
            options: opts,
            config_root: config_root.map(|s| s.to_string()),
        };
        plugin.mise_path(ctx).await
    }

    pub async fn parse_legacy_file(
        &self,
        sdk: &str,
        file: &Path,
    ) -> Result<ParseLegacyFileResponse> {
        let sdk = self.get_sdk(sdk)?;
        sdk.parse_legacy_file(file).await
    }

    async fn download(&self, url: &Url, sdk: &Plugin, version: &str) -> Result<PathBuf> {
        self.log_emit(format!("Downloading {url}"));
        let filename = url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .ok_or("No filename in URL")?;
        let path = self
            .download_dir
            .join(format!("{sdk}-{version}"))
            .join(filename);
        let url_str = url.to_string();
        let bytes = retry_async(&url_str, || async {
            let resp = CLIENT.get(url.clone()).send().await?;
            let resp = resp.error_for_status()?;
            resp.bytes().await
        })
        .await?;
        file::mkdirp(path.parent().unwrap())?;
        let mut file = tokio::fs::File::create(&path).await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        file.sync_all().await?;
        Ok(path)
    }

    async fn verify(
        &self,
        pre_install: &PreInstall,
        file: &Path,
    ) -> Result<Option<VerifiedAttestation>> {
        self.log_emit(format!("Verifying {file:?} checksum"));
        if let Some(sha256) = &pre_install.sha256 {
            xx::hash::ensure_checksum_sha256(file, sha256)?;
        }
        if let Some(sha512) = &pre_install.sha512 {
            xx::hash::ensure_checksum_sha512(file, sha512)?;
        }
        if let Some(_sha1) = &pre_install.sha1 {
            unimplemented!("sha1")
        }
        if let Some(_md5) = &pre_install.md5 {
            unimplemented!("md5")
        }
        let mut verified: Option<VerifiedAttestation> = None;
        // Only skip attestation verification when the plugin provides a checksum
        // (sha256/sha512) — otherwise there would be no integrity check at all.
        let has_checksum = pre_install.sha256.is_some() || pre_install.sha512.is_some();
        if let Some(attestation) = &pre_install.attestation
            && !(self.skip_verification && has_checksum)
        {
            self.log_emit(format!("Verify {file:?} attestation"));
            if let Some(owner) = &attestation.github_owner
                && let Some(repo) = &attestation.github_repo
            {
                let token = std::env::var("MISE_GITHUB_TOKEN")
                    .or_else(|_| std::env::var("GITHUB_TOKEN"))
                    .or(Err("GitHub artifact attestation verification requires either the MISE_GITHUB_TOKEN or GITHUB_TOKEN environment variable set"))?;
                mise_sigstore::verify_github_attestation(
                    file,
                    owner.as_str(),
                    repo.as_str(),
                    Some(token.as_str()),
                    attestation.github_signer_workflow.as_deref(),
                )
                .await?;
                // All configured verifications always execute (no short-circuit).
                // Priority only affects which variant is *recorded* in `verified`.
                // GitHub attestations have the highest recording priority.
                verified = Some(VerifiedAttestation::GithubAttestations {
                    owner: owner.clone(),
                    repo: repo.clone(),
                    signer_workflow: attestation.github_signer_workflow.clone(),
                });
            }

            if let Some(sig_or_bundle_path) = &attestation.cosign_sig_or_bundle_path {
                if let Some(public_key_path) = &attestation.cosign_public_key_path {
                    mise_sigstore::verify_cosign_signature_with_key(
                        file,
                        sig_or_bundle_path,
                        public_key_path,
                    )
                    .await?;
                } else {
                    mise_sigstore::verify_cosign_signature(file, sig_or_bundle_path).await?;
                }
                // Cosign has the lowest recording priority: only record it if no
                // higher-priority verification was already recorded.
                if verified.is_none() {
                    verified = Some(VerifiedAttestation::Cosign {
                        sig_or_bundle_path: sig_or_bundle_path.clone(),
                        public_key_path: attestation.cosign_public_key_path.clone(),
                    });
                }
            }

            if let Some(provenance_path) = &attestation.slsa_provenance_path {
                let min_level = attestation.slsa_min_level.unwrap_or(1u8);
                mise_sigstore::verify_slsa_provenance(file, provenance_path, min_level).await?;
                // SLSA has mid-tier recording priority: record it unless GitHub
                // attestation (higher priority) was already recorded.
                // Note: if Cosign also passed, SLSA supersedes it (SLSA > Cosign).
                if !matches!(
                    verified,
                    Some(VerifiedAttestation::GithubAttestations { .. })
                ) {
                    verified = Some(VerifiedAttestation::Slsa {
                        provenance_path: provenance_path.clone(),
                    });
                }
            }
        }
        Ok(verified)
    }

    fn extract(&self, file: &Path, install_dir: &Path) -> Result<()> {
        self.log_emit(format!("Extracting {file:?} to {install_dir:?}"));
        let filename = file.file_name().unwrap().to_string_lossy().to_string();
        let parent = install_dir.parent().unwrap();
        file::mkdirp(parent)?;
        let tmp = TempDir::with_prefix_in(&filename, parent)?;
        file::remove_dir_all(install_dir)?;
        let move_to_install = || {
            let subdirs = file::ls(tmp.path())?;
            if subdirs.len() == 1 && subdirs.first().unwrap().is_dir() {
                let subdir = subdirs.first().unwrap();
                file::mv(subdir, install_dir)?;
            } else {
                file::mv(tmp.path(), install_dir)?;
            }
            Result::Ok(())
        };
        if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
            xx::archive::untar_gz(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".tar.xz") || filename.ends_with(".txz") {
            xx::archive::untar_xz(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".tar.bz2")
            || filename.ends_with(".tbz2")
            || filename.ends_with(".tbz")
        {
            xx::archive::untar_bz2(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".zip") {
            xx::archive::unzip(file, tmp.path())?;
            move_to_install()?;
        } else {
            file::mv(file, install_dir.join(&filename))?;
            #[cfg(unix)]
            file::make_executable(install_dir.join(&filename))?;
        }
        Ok(())
    }
}

/// Convert a `PreInstallAttestation` to the highest-priority `VerifiedAttestation` variant
/// declared by the plugin. Priority: GitHub > SLSA > Cosign.
///
/// This is used by `pre_install_provenance_for_platform` to report what *type* of attestation
/// the plugin declares, without actually performing sigstore verification.
fn attestation_to_verified(att: PreInstallAttestation) -> Option<VerifiedAttestation> {
    // GitHub attestations have the highest priority
    if let Some(owner) = att.github_owner
        && let Some(repo) = att.github_repo
    {
        return Some(VerifiedAttestation::GithubAttestations {
            owner,
            repo,
            signer_workflow: att.github_signer_workflow,
        });
    }
    // SLSA is second priority
    if let Some(provenance_path) = att.slsa_provenance_path {
        return Some(VerifiedAttestation::Slsa { provenance_path });
    }
    // Cosign is third priority
    if let Some(sig_or_bundle_path) = att.cosign_sig_or_bundle_path {
        return Some(VerifiedAttestation::Cosign {
            sig_or_bundle_path,
            public_key_path: att.cosign_public_key_path,
        });
    }
    None
}

impl Default for Vfox {
    fn default() -> Self {
        Self {
            runtime_version: "1.0.0".to_string(),
            plugin_dir: home().join(".version-fox/plugin"),
            cache_dir: home().join(".version-fox/cache"),
            download_dir: home().join(".version-fox/downloads"),
            install_dir: home().join(".version-fox/installs"),
            skip_verification: false,
            cmd_env: None,
            default_inline_shell: None,
            github_token: None,
            github_token_resolver: None,
            runtime_env_type: None,
            log_tx: None,
        }
    }
}

fn home() -> PathBuf {
    homedir::my_home()
        .ok()
        .flatten()
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Vfox {
        pub fn test() -> Self {
            Self {
                runtime_version: "1.0.0".to_string(),
                plugin_dir: PathBuf::from("plugins"),
                cache_dir: PathBuf::from("test/cache"),
                download_dir: PathBuf::from("test/downloads"),
                install_dir: PathBuf::from("test/installs"),
                skip_verification: false,
                cmd_env: None,
                default_inline_shell: None,
                github_token: None,
                github_token_resolver: None,
                runtime_env_type: None,
                log_tx: None,
            }
        }
    }

    #[tokio::test]
    async fn test_env_keys() {
        let vfox = Vfox::test();
        // dummy plugin already exists in plugins/dummy, no need to install
        let keys = vfox
            .env_keys(
                "dummy",
                "1.0.0",
                serde_json::Value::Object(Default::default()),
            )
            .await
            .unwrap();
        let output = format!("{keys:?}").replace(
            &vfox.install_dir.to_string_lossy().to_string(),
            "<INSTALL_DIR>",
        );
        assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_install_plugin() {
        let vfox = Vfox::test();
        // dummy plugin already exists in plugins/dummy, just verify it's there
        assert!(vfox.plugin_dir.join("dummy").exists());
        let plugin = Plugin::from_dir(&vfox.plugin_dir.join("dummy")).unwrap();
        assert_eq!(plugin.name, "dummy");
    }

    #[tokio::test]
    async fn test_install() {
        let vfox = Vfox::test();
        let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
        // dummy plugin already exists in plugins/dummy
        vfox.install("dummy", "1.0.0", &install_dir).await.unwrap();
        // dummy plugin doesn't actually install binaries, so we just check the directory
        assert!(vfox.install_dir.join("dummy").join("1.0.0").exists());
        vfox.uninstall("dummy", "1.0.0").unwrap();
        assert!(!vfox.install_dir.join("dummy").join("1.0.0").exists());
        file::remove_dir_all(vfox.install_dir).unwrap();
        file::remove_dir_all(vfox.download_dir).unwrap();
    }

    #[tokio::test]
    async fn test_github_token_resolver_not_called_for_local_hooks() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // env_keys and pre_uninstall on the dummy plugin do no network I/O,
        // so a lazy GitHub token resolver registered on Vfox must not be
        // invoked. This is the regression check for
        // https://github.com/jdx/mise/discussions/9797 — `mise hook-env` and
        // friends must not spawn `github.credential_command`.
        let temp_dir = tempfile::tempdir().unwrap();
        let mut vfox = Vfox::test();
        vfox.install_dir = temp_dir.path().join("installs");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_inner = calls.clone();
        vfox.github_token_resolver = Some(Arc::new(move || {
            calls_inner.fetch_add(1, Ordering::SeqCst);
            None
        }));

        vfox.env_keys(
            "dummy",
            "1.0.0",
            serde_json::Value::Object(Default::default()),
        )
        .await
        .unwrap();

        let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
        std::fs::create_dir_all(&install_dir).unwrap();
        vfox.pre_uninstall("dummy", "1.0.0", &install_dir)
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_pre_uninstall() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut vfox = Vfox::test();
        vfox.install_dir = temp_dir.path().join("installs");
        let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
        std::fs::create_dir_all(&install_dir).unwrap();

        vfox.pre_uninstall("dummy", "1.0.0", &install_dir)
            .await
            .unwrap();

        let marker = std::fs::read_to_string(install_dir.join("pre_uninstall_marker")).unwrap();
        assert_eq!(
            marker,
            format!(
                "dummy:1.0.0:{}",
                install_dir.to_string_lossy().replace('\\', "/")
            )
        );
    }

    #[tokio::test]
    #[ignore] // disable for now
    async fn test_install_cmake() {
        let vfox = Vfox::test();
        vfox.install_plugin("cmake").unwrap();
        let install_dir = vfox.install_dir.join("cmake").join("3.21.0");
        vfox.install("cmake", "3.21.0", &install_dir).await.unwrap();
        if cfg!(target_os = "linux") {
            assert!(
                vfox.install_dir
                    .join("cmake")
                    .join("3.21.0")
                    .join("bin")
                    .join("cmake")
                    .exists()
            );
        } else if cfg!(target_os = "macos") {
            assert!(
                vfox.install_dir
                    .join("cmake")
                    .join("3.21.0")
                    .join("CMake.app")
                    .join("Contents")
                    .join("bin")
                    .join("cmake")
                    .exists()
            );
        } else if cfg!(target_os = "windows") {
            assert!(
                vfox.install_dir
                    .join("cmake")
                    .join("3.21.0")
                    .join("bin")
                    .join("cmake.exe")
                    .exists()
            );
        }
        vfox.uninstall_plugin("cmake").unwrap();
        assert!(!vfox.plugin_dir.join("cmake").exists());
        vfox.uninstall("cmake", "3.21.0").unwrap();
        assert!(!vfox.install_dir.join("cmake").join("3.21.0").exists());
        file::remove_dir_all(vfox.plugin_dir.join("cmake")).unwrap();
        file::remove_dir_all(vfox.install_dir).unwrap();
        file::remove_dir_all(vfox.download_dir).unwrap();
    }

    #[tokio::test]
    async fn test_metadata() {
        let vfox = Vfox::test();
        // dummy plugin already exists in plugins/dummy
        let metadata = vfox.metadata("dummy").await.unwrap();
        let out = format!("{metadata:?}");
        assert_snapshot!(out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_backend_list_versions_with_cmd_env() {
        let mut vfox = Vfox::test();
        let mut env = IndexMap::new();
        env.insert("MY_TEST_VAR".to_string(), "hello".to_string());
        env.insert(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        );
        vfox.cmd_env = Some(env);

        let versions = vfox
            .backend_list_versions("dummy-backend", "test-tool", IndexMap::new())
            .await
            .unwrap();
        assert_eq!(versions, vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn test_backend_list_versions_without_cmd_env() {
        let vfox = Vfox::test();
        let versions = vfox
            .backend_list_versions("dummy-backend", "test-tool", IndexMap::new())
            .await
            .unwrap();
        assert_eq!(versions, vec!["fallback".to_string()]);
    }
}
