use indexmap::IndexMap;
use itertools::Itertools;
use reqwest::Url;
use reqwest::header::HeaderMap;
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
use crate::hooks::backend_list_tools::BackendListToolsContext;
use crate::hooks::backend_list_versions::BackendListVersionsContext;
use crate::hooks::backend_search_tools::BackendSearchToolsContext;
use crate::hooks::backend_tools::BackendTool;
use crate::hooks::backend_uninstall::BackendUninstallContext;
use crate::hooks::env_keys::{EnvKey, EnvKeysContext};
use crate::hooks::mise_env::{MiseEnvContext, MiseEnvResult};
use crate::hooks::mise_install_satisfied::{
    MiseInstallSatisfiedContext, MiseInstallSatisfiedResult,
};
use crate::hooks::mise_path::MisePathContext;
use crate::hooks::package::{
    PackageActionContext, PackageActionResponse, PackageInstalledContext, PackageInstalledResponse,
    PackageUninstallContext,
};
use crate::hooks::parse_legacy_file::ParseLegacyFileResponse;
use crate::hooks::post_install::PostInstallContext;
use crate::hooks::pre_install::{PreInstall, PreInstallAttestation, VerifiedAttestation};
use crate::hooks::pre_uninstall::PreUninstallContext;
use crate::http::{CLIENT, HttpHeadersResolver, retry_async};
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
    /// Whether the user asked for child stdio to be connected (mise's `raw`
    /// setting). When false, hook children spawned by `cmd.exec`/`os.execute`
    /// get `/dev/null` on stdin so a parallel install cannot race for it.
    pub raw_stdio: bool,
    /// Serializes children that write to the terminal, used by Lua `cmd.stream`
    /// and `os.execute`. Set by mise; when absent those commands run directly,
    /// which is what standalone `vfox-cli` wants.
    pub terminal_lock: Option<TerminalLock>,
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
    url_rewriter: Option<UrlRewriter>,
    http_headers_resolver: Option<HttpHeadersResolver>,
    log_tx: Option<mpsc::Sender<String>>,
    log_handler: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

/// Runs a child that writes to the terminal, under mise's terminal lock.
///
/// Called with `exclusive = true` by `cmd.stream`, which needs the terminal to
/// itself, and `exclusive = false` by `os.execute`, whose output streams to the
/// terminal and so must not overlap an exclusive child.
///
/// Takes the work to run rather than a command so the implementation can hold its
/// guards across the child's whole lifetime and release them when it returns.
/// Returns the child's exit status. (#13254)
pub type TerminalLock = Arc<
    dyn Fn(
            bool,
            &mut dyn FnMut() -> std::result::Result<i64, String>,
        ) -> std::result::Result<i64, String>
        + Send
        + Sync,
>;

pub(crate) type UrlRewriter = Arc<dyn Fn(&mut Url) + Send + Sync>;

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
            .field(
                "url_rewriter",
                &self.url_rewriter.as_ref().map(|_| "<closure>"),
            )
            .field(
                "http_headers_resolver",
                &self.http_headers_resolver.as_ref().map(|_| "<closure>"),
            )
            .field(
                "log_handler",
                &self.log_handler.as_ref().map(|_| "<closure>"),
            )
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

    pub fn set_log_handler<F>(&mut self, handler: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.log_handler = Some(Arc::new(handler));
    }

    pub fn set_url_rewriter<F>(&mut self, rewriter: F)
    where
        F: Fn(&mut Url) + Send + Sync + 'static,
    {
        self.url_rewriter = Some(Arc::new(rewriter));
    }

    pub fn set_http_headers_resolver<F>(&mut self, resolver: F)
    where
        F: Fn(&Url) -> HeaderMap + Send + Sync + 'static,
    {
        self.http_headers_resolver = Some(Arc::new(resolver));
    }

    fn rewrite_url(&self, url: &mut Url) {
        if let Some(rewriter) = &self.url_rewriter {
            rewriter(url);
        }
    }

    fn log_emit(&self, msg: String) {
        if let Some(tx) = &self.log_tx {
            let _ = tx.send(msg.clone());
        }
        if let Some(handler) = &self.log_handler {
            handler(msg);
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
        plugin.set_raw_stdio(self.raw_stdio)?;
        if let Some(runner) = &self.terminal_lock {
            plugin.set_terminal_lock(runner.clone())?;
        }
        if let Some(rewriter) = &self.url_rewriter {
            plugin.set_url_rewriter(rewriter.clone())?;
        }
        if let Some(resolver) = &self.http_headers_resolver {
            plugin.set_http_headers_resolver(resolver.clone())?;
        }
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
}

mod backend;
mod hooks;
mod install;

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
    if let Some(provenance_path) = att.slsa_provenance_path
        && att.slsa_signer_identity.is_some()
        && att.slsa_signer_issuer.is_some()
    {
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
            raw_stdio: false,
            terminal_lock: None,
            github_token: None,
            github_token_resolver: None,
            runtime_env_type: None,
            url_rewriter: None,
            http_headers_resolver: None,
            log_tx: None,
            log_handler: None,
        }
    }
}

fn home() -> PathBuf {
    homedir::my_home()
        .ok()
        .flatten()
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Compare a checksum a plugin supplied against one computed from the downloaded file.
///
/// `xx::hash` ships `ensure_checksum_*` for sha256/sha512 only, so sha1 and md5 compare here.
/// The expected value is lowercased because upstream checksum files are inconsistent about case
/// while `xx::hash` always returns lowercase hex — the same normalisation mise's own
/// `hash::ensure_checksum` applies, whose message this reuses.
fn ensure_checksum(file: &Path, algo: &str, expected: &str, actual: &str) -> Result<()> {
    let expected = expected.to_lowercase();
    if actual != expected {
        return Err(format!(
            "Checksum mismatch for file {}:\nExpected: {algo}:{expected}\nActual:   {algo}:{actual}",
            file.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
