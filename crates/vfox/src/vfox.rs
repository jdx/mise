use itertools::Itertools;
use reqwest::Url;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc;
use tempfile::TempDir;
use xx::file;

use crate::error::Result;
use crate::hooks::available::AvailableVersion;
use crate::hooks::backend_exec_env::BackendExecEnvContext;
use crate::hooks::backend_install::BackendInstallContext;
use crate::hooks::backend_list_versions::BackendListVersionsContext;
use crate::hooks::env_keys::{EnvKey, EnvKeysContext};
use crate::hooks::mise_env::MiseEnvContext;
use crate::hooks::mise_path::MisePathContext;
use crate::hooks::parse_legacy_file::ParseLegacyFileResponse;
use crate::hooks::post_install::PostInstallContext;
use crate::hooks::pre_install::PreInstall;
use crate::http::CLIENT;
use crate::metadata::Metadata;
use crate::plugin::Plugin;
use crate::registry;
use crate::sdk_info::SdkInfo;

#[derive(Debug)]
pub struct Vfox {
    pub runtime_version: String,
    pub install_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub download_dir: PathBuf,
    log_tx: Option<mpsc::Sender<String>>,
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
        let sdk = self.get_sdk(sdk)?;
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
        Plugin::from_dir(&self.plugin_dir.join(name))
    }

    pub fn install_plugin(&self, sdk: &str) -> Result<Plugin> {
        let plugin_dir = self.plugin_dir.join(sdk);
        if !plugin_dir.exists() {
            let url = registry::sdk_url(sdk).ok_or_else(|| format!("Unknown SDK: {sdk}"))?;
            return self.install_plugin_from_url(url);
        }
        Plugin::from_dir(&plugin_dir)
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
        Plugin::from_dir(&plugin_dir)
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
    ) -> Result<()> {
        self.install_plugin(sdk)?;
        let sdk = self.get_sdk(sdk)?;
        let pre_install = sdk.pre_install(version).await?;
        let install_dir = install_dir.as_ref();
        trace!("{pre_install:?}");
        if let Some(url) = pre_install.url.as_ref().map(|s| Url::from_str(s)) {
            let file = self.download(&url?, &sdk, version).await?;
            self.verify(&pre_install, &file)?;
            self.extract(&file, install_dir)?;
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

        Ok(())
    }

    pub fn uninstall(&self, sdk: &str, version: &str) -> Result<()> {
        let path = self.install_dir.join(sdk).join(version);
        file::remove_dir_all(&path)?;
        Ok(())
    }

    pub async fn metadata(&self, sdk: &str) -> Result<Metadata> {
        self.get_sdk(sdk)?.get_metadata()
    }

    pub async fn env_keys(&self, sdk: &str, version: &str) -> Result<Vec<EnvKey>> {
        debug!("Getting env keys for {sdk} version {version}");
        let sdk = self.get_sdk(sdk)?;
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
        };
        sdk.env_keys(ctx).await
    }

    pub async fn mise_env<T: serde::Serialize>(&self, sdk: &str, opts: T) -> Result<Vec<EnvKey>> {
        let plugin = self.get_sdk(sdk)?;
        let ctx = MiseEnvContext {
            args: vec![],
            options: opts,
        };
        plugin.mise_env(ctx).await
    }

    pub async fn backend_list_versions(&self, sdk: &str, tool: &str) -> Result<Vec<String>> {
        let plugin = self.get_sdk(sdk)?;
        let ctx = BackendListVersionsContext {
            tool: tool.to_string(),
        };
        plugin.backend_list_versions(ctx).await.map(|r| r.versions)
    }

    pub async fn backend_install(
        &self,
        sdk: &str,
        tool: &str,
        version: &str,
        install_path: PathBuf,
    ) -> Result<()> {
        let plugin = self.get_sdk(sdk)?;
        let ctx = BackendInstallContext {
            tool: tool.to_string(),
            version: version.to_string(),
            install_path,
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
    ) -> Result<Vec<EnvKey>> {
        let plugin = self.get_sdk(sdk)?;
        let ctx = BackendExecEnvContext {
            tool: tool.to_string(),
            version: version.to_string(),
            install_path,
        };
        plugin.backend_exec_env(ctx).await.map(|r| r.env_vars)
    }

    pub async fn mise_path<T: serde::Serialize>(&self, sdk: &str, opts: T) -> Result<Vec<String>> {
        let plugin = self.get_sdk(sdk)?;
        let ctx = MisePathContext {
            args: vec![],
            options: opts,
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
        let resp = CLIENT.get(url.clone()).send().await?;
        resp.error_for_status_ref()?;
        file::mkdirp(path.parent().unwrap())?;
        let mut file = tokio::fs::File::create(&path).await?;
        let bytes = resp.bytes().await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        Ok(path)
    }

    fn verify(&self, pre_install: &PreInstall, file: &Path) -> Result<()> {
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
        Ok(())
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
        if filename.ends_with(".tar.gz") {
            xx::archive::untar_gz(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".tar.xz") {
            xx::archive::untar_xz(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".tar.bz2") {
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

impl Default for Vfox {
    fn default() -> Self {
        Self {
            runtime_version: "1.0.0".to_string(),
            plugin_dir: home().join(".version-fox/plugin"),
            cache_dir: home().join(".version-fox/cache"),
            download_dir: home().join(".version-fox/downloads"),
            install_dir: home().join(".version-fox/installs"),
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
                log_tx: None,
            }
        }
    }

    #[tokio::test]
    async fn test_env_keys() {
        let vfox = Vfox::test();
        // dummy plugin already exists in plugins/dummy, no need to install
        let keys = vfox.env_keys("dummy", "1.0.0").await.unwrap();
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
    #[ignore] // disable for now
    async fn test_install_cmake() {
        let vfox = Vfox::test();
        vfox.install_plugin("cmake").unwrap();
        let install_dir = vfox.install_dir.join("cmake").join("3.21.0");
        vfox.install("cmake", "3.21.0", &install_dir).await.unwrap();
        if cfg!(target_os = "linux") {
            assert!(vfox
                .install_dir
                .join("cmake")
                .join("3.21.0")
                .join("bin")
                .join("cmake")
                .exists());
        } else if cfg!(target_os = "macos") {
            assert!(vfox
                .install_dir
                .join("cmake")
                .join("3.21.0")
                .join("CMake.app")
                .join("Contents")
                .join("bin")
                .join("cmake")
                .exists());
        } else if cfg!(target_os = "windows") {
            assert!(vfox
                .install_dir
                .join("cmake")
                .join("3.21.0")
                .join("bin")
                .join("cmake.exe")
                .exists());
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
}
