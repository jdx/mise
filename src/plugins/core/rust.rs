use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, CONFIG, SETTINGS};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, Toolset};
use crate::{dirs, file, github, plugins};
use eyre::Result;

#[derive(Debug)]
pub struct RustPlugin {
    ba: BackendArg,
}

impl RustPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("rust"),
        }
    }

    fn setup_rustup(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        if rustup_home().join("settings.toml").exists() && cargo_bin().exists() {
            return Ok(());
        }
        ctx.pr.set_message("Downloading rustup-init".into());
        HTTP.download_file(
            "https://sh.rustup.rs",
            &rustup_path(),
            Some(ctx.pr.as_ref()),
        )?;
        file::make_executable(rustup_path())?;
        file::create_dir_all(rustup_home())?;
        let cmd = CmdLineRunner::new(rustup_path())
            .with_pr(ctx.pr.as_ref())
            .arg("--no-modify-path")
            .arg("--default-toolchain")
            .arg("none")
            .arg("-y")
            .envs(self.exec_env(&CONFIG, CONFIG.get_toolset()?, tv)?);
        cmd.execute()?;
        Ok(())
    }

    fn test_rust(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message(format!("{RUSTC_BIN} -V"));
        CmdLineRunner::new(RUSTC_BIN)
            .with_pr(ctx.pr.as_ref())
            .arg("-V")
            .envs(self.exec_env(&CONFIG, CONFIG.get_toolset()?, tv)?)
            .prepend_path(self.list_bin_paths(tv)?)?
            .execute()
    }
}

impl Backend for RustPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        let versions = github::list_releases("rust-lang/rust")?
            .into_iter()
            .map(|r| r.tag_name)
            .rev()
            .chain(vec!["nightly".into(), "beta".into(), "stable".into()])
            .collect();
        Ok(versions)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec!["rust-toolchain.toml".into()])
    }

    fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        let toml = file::read_to_string(path)?;
        let toml = toml.parse::<toml::Value>()?;
        if let Some(toolchain) = toml.get("toolchain") {
            if let Some(channel) = toolchain.get("channel") {
                return Ok(channel.as_str().unwrap().to_string());
            }
        }
        Ok("".into())
    }

    fn install_version_impl(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        self.setup_rustup(ctx, &tv)?;

        CmdLineRunner::new(RUSTUP_BIN)
            .with_pr(ctx.pr.as_ref())
            .arg("toolchain")
            .arg("install")
            .arg(&tv.version)
            .prepend_path(self.list_bin_paths(&tv)?)?
            .envs(self.exec_env(&CONFIG, CONFIG.get_toolset()?, &tv)?)
            .execute()?;

        file::remove_all(tv.install_path())?;
        file::make_symlink(&cargo_home().join("bin"), &tv.install_path())?;

        self.test_rust(ctx, &tv)?;

        Ok(tv)
    }

    fn list_bin_paths(&self, _tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        Ok(vec![cargo_bindir()])
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        let toolchain = tv.version.to_string();
        Ok([
            (
                "CARGO_HOME".to_string(),
                cargo_home().to_string_lossy().to_string(),
            ),
            (
                "RUSTUP_HOME".to_string(),
                rustup_home().to_string_lossy().to_string(),
            ),
            ("RUSTUP_TOOLCHAIN".to_string(), toolchain),
        ]
        .into())
    }
}

#[cfg(unix)]
const RUSTC_BIN: &str = "rustc";

#[cfg(windows)]
const RUSTC_BIN: &str = "rustc.exe";

#[cfg(unix)]
const RUSTUP_INIT_BIN: &str = "rustup-init";

#[cfg(windows)]
const RUSTUP_INIT_BIN: &str = "rustup-init.exe";

#[cfg(unix)]
const RUSTUP_BIN: &str = "rustup";

#[cfg(windows)]
const RUSTUP_BIN: &str = "rustup.exe";

#[cfg(unix)]
const CARGO_BIN: &str = "cargo";

#[cfg(windows)]
const CARGO_BIN: &str = "cargo.exe";

fn rustup_path() -> PathBuf {
    dirs::CACHE.join("rust").join(RUSTUP_INIT_BIN)
}

fn rustup_home() -> PathBuf {
    SETTINGS
        .rust
        .rustup_home
        .clone()
        .unwrap_or(dirs::HOME.join(".rustup"))
}

fn cargo_home() -> PathBuf {
    SETTINGS
        .rust
        .cargo_home
        .clone()
        .unwrap_or(dirs::HOME.join(".cargo"))
}

fn cargo_bin() -> PathBuf {
    cargo_bindir().join(CARGO_BIN)
}
fn cargo_bindir() -> PathBuf {
    cargo_home().join("bin")
}
