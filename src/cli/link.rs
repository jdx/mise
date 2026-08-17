use std::path::PathBuf;

use clap::ValueHint;
use color_eyre::eyre::{Result, eyre};
use console::style;
use eyre::bail;
use path_absolutize::Absolutize;

use crate::file::{make_symlink, remove_all};
use crate::toolset::{ToolRequest, ToolVersion, install_state};
use crate::{backend, config, file};
use crate::{cli::args::ToolArg, config::Config};

/// Symlinks a tool version into mise
///
/// Use this for adding installs either custom compiled outside mise or built with a different tool.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ln", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Link {
    /// Tool name and version to create a symlink for
    #[clap(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// The local path to the tool version
    /// e.g.: ~/.nvm/versions/node/v20.0.0
    #[clap(value_hint = ValueHint::DirPath, verbatim_doc_comment)]
    path: PathBuf,

    /// Overwrite an existing tool version if it exists
    #[clap(long, short = 'f')]
    force: bool,
}

impl Link {
    pub async fn run(self) -> Result<()> {
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
        let target = self.tool.ba.installs_path.join(&version_pathname);
        if !file::is_symlink_to(&target, &path) && file::same_file(&path, &target) {
            bail!("cannot link {} to its own install path", self.tool.style());
        }
        {
            let _state_lock =
                install_state::lock_tool_version(&self.tool.ba.short, &version_pathname)?;
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
                install_state::clear_incomplete_marker(&self.tool.ba.short, &version_pathname)?;
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # build node-20.0.0 with node-build and link it into mise
    $ <bold>node-build 20.0.0 ~/.nodes/20.0.0</bold>
    $ <bold>mise link node@20.0.0 ~/.nodes/20.0.0</bold>

    # have mise use the node version provided by Homebrew
    $ <bold>brew install node</bold>
    $ <bold>mise link node@brew $(brew --prefix node)</bold>
    $ <bold>mise use node@brew</bold>
"#
);
