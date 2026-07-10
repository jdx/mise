use crate::cli::args::ToolArg;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::install_context::InstallContext;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;
use clap::ValueHint;
use console::style;
use eyre::{Result, bail, eyre};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Install a tool version to a specific path
///
/// Used for building a tool to a directory for use outside of mise
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct InstallInto {
    /// Tool to install
    /// e.g.: node@20
    #[clap(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// Path to install the tool into
    #[clap(value_hint = ValueHint::DirPath)]
    path: PathBuf,
}

impl InstallInto {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = Arc::new(
            ToolsetBuilder::new()
                .with_args(std::slice::from_ref(&self.tool))
                .build(&config)
                .await?,
        );
        let mut tv = ts
            .versions
            .get(self.tool.ba.as_ref())
            .ok_or_else(|| eyre!("Tool not found"))?
            .versions
            .first()
            .unwrap()
            .clone();
        let before_date = tv.before_date;
        let backend = tv.backend()?;
        let mpr = MultiProgressReport::get();
        let install_ctx = InstallContext {
            config: config.clone(),
            ts: ts.clone(),
            pr: mpr.add(&tv.style()),
            force: true,
            dry_run: false,
            locked: false, // install-into doesn't support locked mode
            before_date,
        };
        tv.install_path = Some(self.path.clone());
        // install-into force-reinstalls, which uninstalls (rm -rf) whatever
        // already exists at the install path. Check immediately before the
        // install performs that deletion (rather than at the start of `run`) so
        // a directory that became non-empty during tool resolution can't be
        // clobbered without an explicit opt-in. Refuse to overwrite a non-empty
        // directory (e.g. `.`) unless the user passes -y/--yes or confirms
        // interactively; the prompt defaults to "no" since it is destructive.
        // (#8115)
        if path_has_contents(&self.path) {
            let proceed = Settings::get().yes
                || prompt::confirm_with_default(
                    format!(
                        "{} is not empty; install-into will delete its contents. Continue?",
                        display_path(&self.path)
                    ),
                    false,
                )?;
            if !proceed {
                bail!(
                    "refusing to overwrite non-empty directory {}; pass {} or choose an empty/new path",
                    display_path(&self.path),
                    style("--yes").yellow().for_stderr()
                );
            }
        }
        backend.install_version(install_ctx, tv).await?;
        Ok(())
    }
}

/// True if `path` exists and is anything other than an empty directory
/// (a non-empty directory, or a regular file). Empty/new paths return false.
fn path_has_contents(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_some(), // non-empty dir
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false, // missing -> false
        // A file (NotADirectory) or an unreadable dir (e.g. PermissionDenied):
        // err toward "occupied" so we never silently clobber it.
        Err(_) => path.exists(),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # install node@20.0.0 into ./mynode
    $ <bold>mise install-into node@20.0.0 ./mynode && ./mynode/bin/node -v</bold>
    20.0.0
"#
);
