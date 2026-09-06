use crate::cli::args::ToolArg;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::install_context::InstallContext;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;
use console::style;
use eyre::{Result, bail, eyre};
use path_absolutize::Absolutize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::OnceCell;

/// Install a tool version to a specific path
///
/// Used for building a tool to a directory for use outside of mise
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise install-into node@20.0.0 ./mynode && ./mynode/bin/node -v
v20.0.0"###,
        help = r###"install node@20.0.0 into ./mynode"###
    )
)]
pub(crate) struct InstallInto {
    /// Tool to install
    /// e.g.: node@20
    #[usage(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// Path to install the tool into
    #[usage(value_hint = ValueHint::DirPath)]
    path: PathBuf,
}

impl InstallInto {
    pub(crate) async fn run(self) -> Result<()> {
        let install_path = self.path.absolutize()?.into_owned();
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
            dependency_context: OnceCell::new(),
        };
        tv.install_path = Some(install_path.clone());
        tv.install_path_is_exact = true;
        tv.install_path_is_explicit = true;
        // Serialize every `install-into` writer targeting this destination,
        // including different tools or versions whose ordinary tool-version
        // locks would not overlap. Keep the lock through confirmation and the
        // backend replacement so no cooperating writer can populate the path
        // between the occupancy check and deletion.
        // Resolve parent aliases, including an existing prefix when the rest
        // of the destination does not exist yet. Do not resolve the final
        // component: installation replaces that entry, even if it is a symlink.
        let lock_path = match (install_path.parent(), install_path.file_name()) {
            (Some(parent), Some(name)) => crate::file::desymlink_path(parent).join(name),
            _ => crate::file::desymlink_path(&install_path),
        };
        let lock_dir = crate::dirs::CACHE.join("lockfiles");
        if crate::file::path_starts_with_resolved(&lock_dir, &install_path) {
            bail!(
                "install-into destination {} contains mise's lock directory {}; choose a different destination",
                display_path(&install_path),
                display_path(&lock_dir)
            );
        }
        let lock_display_path = install_path.clone();
        let _destination_lock = tokio::task::spawn_blocking(move || {
            crate::lock_file::LockFile::new(&lock_path)
                .with_callback(move |_| {
                    debug!(
                        "waiting for install-into destination lock on {}",
                        display_path(&lock_display_path)
                    );
                })
                .lock()
        })
        .await??;
        // install-into force-reinstalls, which uninstalls (rm -rf) whatever
        // already exists at the install path. Check immediately before the
        // install performs that deletion (rather than at the start of `run`) so
        // a directory that became non-empty during tool resolution can't be
        // clobbered without an explicit opt-in. Refuse to overwrite a non-empty
        // directory (e.g. `.`) unless the user passes -y/--yes or confirms
        // interactively; the prompt defaults to "no" since it is destructive.
        // (#8115)
        if path_has_contents(&install_path) {
            let proceed = Settings::get().yes
                || prompt::confirm_with_default(
                    format!(
                        "{} is not empty; install-into will delete its contents. Continue?",
                        display_path(&install_path)
                    ),
                    false,
                )?
                .is_yes();
            if !proceed {
                bail!(
                    "refusing to overwrite non-empty directory {}; pass {} or choose an empty/new path",
                    display_path(&install_path),
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
