use super::*;

impl Vfox {
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

    pub async fn backend_search_tools(
        &self,
        sdk: &str,
        query: String,
    ) -> Result<Option<Vec<BackendTool>>> {
        let plugin = self.get_sdk_with_env(sdk)?;
        if !plugin
            .get_metadata()?
            .hooks
            .contains("backend_search_tools")
        {
            return Ok(None);
        }
        let ctx = BackendSearchToolsContext { query };
        plugin
            .backend_search_tools(ctx)
            .await
            .map(|r| Some(r.tools))
    }

    pub async fn backend_list_tools(&self, sdk: &str) -> Result<Option<Vec<BackendTool>>> {
        let plugin = self.get_sdk_with_env(sdk)?;
        if !plugin.get_metadata()?.hooks.contains("backend_list_tools") {
            return Ok(None);
        }
        plugin
            .backend_list_tools(BackendListToolsContext {})
            .await
            .map(|r| Some(r.tools))
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

    /// Runs the plugin's `BackendUninstall` hook, if it has one, before mise removes the
    /// install directory.
    pub async fn backend_uninstall(
        &self,
        sdk: &str,
        tool: &str,
        version: &str,
        install_path: PathBuf,
        download_path: PathBuf,
        options: IndexMap<String, toml::Value>,
    ) -> Result<()> {
        let plugin = self.get_sdk_with_env(sdk)?;
        if !plugin.get_metadata()?.hooks.contains("backend_uninstall") {
            return Ok(());
        }
        plugin
            .backend_uninstall(BackendUninstallContext {
                tool: tool.to_string(),
                version: version.to_string(),
                install_path,
                download_path,
                options,
            })
            .await
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

    pub async fn package_installed(
        &self,
        sdk: &str,
        ctx: PackageInstalledContext,
    ) -> Result<PackageInstalledResponse> {
        self.get_sdk_with_env(sdk)?.package_installed(ctx).await
    }

    pub async fn package_install(
        &self,
        sdk: &str,
        ctx: PackageActionContext,
    ) -> Result<PackageActionResponse> {
        self.get_sdk_with_env(sdk)?.package_install(ctx).await
    }

    pub async fn package_upgrade(
        &self,
        sdk: &str,
        ctx: PackageActionContext,
    ) -> Result<PackageActionResponse> {
        self.get_sdk_with_env(sdk)?.package_upgrade(ctx).await
    }

    pub async fn package_uninstall(
        &self,
        sdk: &str,
        ctx: PackageUninstallContext,
    ) -> Result<PackageActionResponse> {
        self.get_sdk_with_env(sdk)?.package_uninstall(ctx).await
    }
}
