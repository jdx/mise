use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};
use console::style;
use eyre::bail;
use path_absolutize::Absolutize;

use crate::file::{make_symlink, remove_all};
use crate::toolset::{ToolRequest, ToolVersion, install_state};
use crate::{backend, config, file};
use crate::{cli::args::ToolArg, config::Config};

/// Symlink a tool version into mise
///
/// Use this to register an install that was compiled by hand or built with another
/// tool. mise uses the version exactly as written, such as `node@24.11.0` or a name
/// of your own like `node@brew`; `latest`, aliases, and channels are rejected.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "ln",
    verbatim_doc_comment,
    example(
        "mise link node@24.11.0 ~/.nodes/24.11.0",
        help = "Link a node that node-build built into ~/.nodes/24.11.0"
    ),
    example(
        r###"mise link node@brew "$(brew --prefix node)""###,
        help = "Register Homebrew's node as node@brew; then run `mise use node@brew`"
    )
)]
pub(crate) struct Link {
    /// Tool and version to create a symlink for
    #[usage(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// Directory of the existing installation, such as `~/.nvm/versions/node/v24.11.0`
    #[usage(value_hint = ValueHint::DirPath, verbatim_doc_comment)]
    path: PathBuf,

    /// Replace an existing installation of this version
    #[usage(long, short = 'f')]
    force: bool,
}

impl Link {
    pub(crate) async fn run(self) -> Result<()> {
        let tvr = match self.tool.tvr.as_ref() {
            Some(tvr @ (ToolRequest::Version { .. } | ToolRequest::Ref { .. })) => tvr,
            Some(tvr) => bail!(
                "mise link only supports concrete versions and refs, not {}",
                tvr.version()
            ),
            None => bail!("must provide a version for {}", self.tool.style()),
        };
        let config = Config::get().await?;
        let version_pathname = match tvr {
            ToolRequest::Version { version, .. } => {
                let backend = tvr.backend()?;
                let resolved_alias = config.resolve_alias(&backend, version).await?;
                if version == "latest"
                    || resolved_alias != *version
                    || backend.is_rolling_channel(version)
                {
                    bail!(
                        "mise link only supports concrete versions and refs, not {}",
                        tvr.version()
                    );
                }
                ToolVersion::new(tvr.clone(), version.clone()).tv_pathname()
            }
            ToolRequest::Ref { .. } => ToolVersion::new(tvr.clone(), tvr.version()).tv_pathname(),
            _ => unreachable!(),
        };
        let path = self.path.absolutize()?;
        if !path.exists() {
            warn!(
                "Target path {} does not exist",
                style(path.to_string_lossy()).cyan().for_stderr()
            );
        }
        let target = self.tool.ba.installs_path().join(&version_pathname);
        if !file::is_symlink_to(&target, &path) && file::same_file(&path, &target) {
            bail!("cannot link {} to its own install path", self.tool.style());
        }
        // Under the identity layout the slot is a link to the installation, so the
        // install path is that installation however it is spelled (the version
        // link, a runtime alias, or the directory itself).
        if crate::install_layout::resolver::link_target(&target)
            .is_some_and(|install| file::same_file(&path, &install))
        {
            bail!("cannot link {} to its own install path", self.tool.style());
        }
        {
            let _state_lock = install_state::lock_tool_version(&self.tool.ba, &version_pathname)?;
            if !file::is_symlink_to(&target, &path) {
                if target.exists() {
                    if self.force {
                        remove_all(&target)?;
                    } else {
                        return Err(eyre!(
                            "Tool version {} already exists, use {} to overwrite",
                            self.tool.style(),
                            style("--force").yellow().for_stderr()
                        ));
                    }
                }
                file::create_dir_all(target.parent().unwrap())?;
                make_symlink(&path, &target)?;
            }

            if path.exists() {
                install_state::clear_incomplete_marker(&self.tool.ba, &version_pathname)?;
            }
        }

        backend::reset().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(
            &config,
            ts,
            &[],
            crate::lockfile::LockfileUpdateMode::Normal,
        )
        .await?;
        Ok(())
    }
}
