use super::*;

impl Vfox {
    pub async fn pre_install_for_platform(
        &self,
        sdk: &str,
        version: &str,
        os: &str,
        arch: &str,
    ) -> Result<PreInstall> {
        self.pre_install_for_platform_with_options(sdk, version, os, arch, Default::default())
            .await
    }

    pub async fn pre_install_for_platform_with_options(
        &self,
        sdk: &str,
        version: &str,
        os: &str,
        arch: &str,
        options: IndexMap<String, toml::Value>,
    ) -> Result<PreInstall> {
        let sdk = self.get_sdk_with_env(sdk)?;
        sdk.pre_install_for_platform_with_options(version, os, arch, options)
            .await
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
        self.pre_install_provenance_for_platform_with_options(
            sdk,
            version,
            os,
            arch,
            Default::default(),
        )
        .await
    }

    pub async fn pre_install_provenance_for_platform_with_options(
        &self,
        sdk: &str,
        version: &str,
        os: &str,
        arch: &str,
        options: IndexMap<String, toml::Value>,
    ) -> Result<(Option<String>, Option<VerifiedAttestation>)> {
        let pre = self
            .pre_install_for_platform_with_options(sdk, version, os, arch, options)
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
        let install_dir = self.install_dir.join(&sdk.name).join(version);
        self.env_keys_for_sdk_install_dir(sdk, version, install_dir, options)
            .await
    }

    pub async fn env_keys_for_install_dir<T: serde::Serialize>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: impl AsRef<Path>,
        options: T,
    ) -> Result<Vec<EnvKey>> {
        debug!("Getting env keys for {sdk} version {version}");
        let sdk = self.get_sdk_with_env(sdk)?;
        self.env_keys_for_sdk_install_dir(sdk, version, install_dir, options)
            .await
    }

    async fn env_keys_for_sdk_install_dir<T: serde::Serialize>(
        &self,
        sdk: Plugin,
        version: &str,
        install_dir: impl AsRef<Path>,
        options: T,
    ) -> Result<Vec<EnvKey>> {
        let sdk_info = sdk.sdk_info(version.to_string(), install_dir.as_ref().to_path_buf())?;
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
}
