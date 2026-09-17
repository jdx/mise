use std::path::PathBuf;

use eyre::{Result, bail};

use super::add::DotfilesAdd;
use crate::config::Config;
use crate::file;
use crate::path::PathExt;
use crate::system;
use crate::system::edits::{BlockSource, EditOp};
use crate::system::files::FileMode;
use crate::system::history::OperationScope;
use crate::system::history::tracked::{self, TrackedSet};
use crate::ui::prompt;

/// Edit a managed dotfile source
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise dot edit ~/.zshrc
mise dot edit --apply ~/.config/starship.toml"###
    )
)]
pub(crate) struct DotfilesEdit {
    /// Target to edit
    #[usage(value_name = "TARGET")]
    target: String,

    /// Apply this target after the editor exits
    #[usage(long)]
    apply: bool,

    /// Dotfile mode to use if the target is not yet managed
    #[usage(long, short)]
    mode: Option<String>,

    /// Source path to use if the target is not yet managed
    #[usage(long, short, value_name = "PATH")]
    source: Option<PathBuf>,

    /// Skip the confirmation prompt when adding an unmanaged target
    #[usage(long, short)]
    yes: bool,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

impl DotfilesEdit {
    /// Open the managed source and optionally converge its target afterward.
    pub(crate) async fn run(self) -> Result<()> {
        // The editor itself changes the managed source, so the whole command
        // is one generation, not just the optional apply.
        OperationScope::wrap("bootstrap dotfiles edit", false, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let mut config = Config::get().await?;
        let target = system::files::resolve_target_arg(&self.target);
        if self.apply {
            let files = system::files::files_from_config(&config)?;
            system::files::validate_composed_file_footprints(&files)?;
        }

        if let Some(path) = source_for_target(&config, &target, &self.target)? {
            open_or_create(&path)?;
            crate::cli::editor::open_in_editor(&path)?;
            if self.apply {
                apply_target(&self.target, self.prompt_secrets).await?;
            }
            return Ok(());
        }

        if !self.yes && console::user_attended_stderr() {
            let ok = prompt::confirm(format!("dotfiles: add {}?", self.target))?.is_yes();
            if !ok {
                info!("dotfiles: skipped");
                return Ok(());
            }
        } else if !self.yes {
            bail!("{} is not managed by [dotfiles]", self.target);
        }

        DotfilesAdd {
            targets: vec![self.target.clone()],
            changed: false,
            mode: self.mode.clone(),
            source: self.source.clone(),
            global: true,
            local: false,
            path: None,
            dry_run: false,
            no_apply: true,
            force: false,
            yes: true,
            prompt_secrets: self.prompt_secrets,
        }
        .run()
        .await?;

        config = Config::reset().await?;
        let Some(path) = source_for_target(&config, &target, &self.target)? else {
            bail!("failed to add {}", self.target);
        };
        open_or_create(&path)?;
        crate::cli::editor::open_in_editor(&path)?;
        if self.apply {
            apply_target(&self.target, self.prompt_secrets).await?;
        }
        Ok(())
    }
}

fn source_for_target(
    config: &Config,
    target: &std::path::Path,
    raw: &str,
) -> Result<Option<PathBuf>> {
    for req in system::files::files_from_config(config)? {
        if system::files::matches_target(&req.target, &req.target_raw, &[raw.to_string()]) {
            // a tracked file has no source: it stays where it is, so that is
            // what to edit. inline content lives in the config that declares
            // it, like an inline edit entry.
            return Ok(Some(match req.mode {
                FileMode::Track => {
                    warn_if_the_edit_escapes_history(config, &req.target);
                    req.target
                }
                FileMode::Content => req.origin.config,
                _ => req.source,
            }));
        }
    }
    let matching_edits = system::edits::edits_from_config(config)?
        .into_iter()
        .filter(|req| system::edits::matches_target(req, &[raw.to_string()]))
        .collect::<Vec<_>>();
    match matching_edits.as_slice() {
        [] => {}
        [req] => {
            return Ok(Some(match &req.op {
                EditOp::Block {
                    source: BlockSource::File(path),
                    ..
                } => path.clone(),
                EditOp::Block {
                    source: BlockSource::Inline(_),
                    ..
                }
                | EditOp::Line { .. } => req.config_path.clone(),
            }));
        }
        edits => {
            let keys = edits
                .iter()
                .map(|req| req.config_key())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("{raw}: multiple [dotfiles] edit entries match; choose one of: {keys}");
        }
    }
    if target.is_relative() {
        bail!("{raw}: target must be absolute or start with ~/");
    }
    Ok(None)
}

/// History captures a tracked symlink as a link, never its destination, so an
/// editor that follows the link writes to a file the surrounding checkpoint
/// does not hold. Say so rather than record a generation that misses the edit.
fn warn_if_the_edit_escapes_history(config: &Config, target: &std::path::Path) {
    if !file::is_symlink_or_junction(target) {
        return;
    }
    let destination = tracked::normalize(target);
    if destination == target {
        return;
    }
    match TrackedSet::from_config(config).and_then(|set| set.would_capture(&destination)) {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            debug!("dotfiles: could not resolve the tracked set: {err:#}");
        }
    }
    warn!(
        "{} is tracked as a symlink: this edits {}, which history does not capture; track that path too to save its contents",
        target.display_user(),
        destination.display_user()
    );
}

fn open_or_create(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write(path, "")?;
    }
    Ok(())
}

/// Apply a selected target after validating the complete composed footprint.
async fn apply_target(target: &str, prompt_secrets: bool) -> Result<()> {
    let config = Config::reset().await?;
    let secrets = system::secrets::resolve(&config, prompt_secrets)?;
    let targets = vec![target.to_string()];
    let all_files = system::files::files_from_config(&config)?;
    system::files::validate_composed_file_footprints(&all_files)?;
    let files = all_files
        .into_iter()
        .filter(|req| system::files::matches_target(&req.target, &req.target_raw, &targets))
        .collect::<Vec<_>>();
    let edits = system::edits::edits_from_config(&config)?
        .into_iter()
        .filter(|req| system::edits::matches_target(req, &targets))
        .collect::<Vec<_>>();
    if !files.is_empty() {
        let opts = system::files::ApplyOpts {
            dry_run: false,
            verbose: false,
            force: false,
            force_hint: "use `mise dot apply --force`",
            yes: true,
        };
        system::files::apply(&config, &files, &opts, &secrets)?;
    }
    if !edits.is_empty() {
        let opts = system::edits::ApplyOpts {
            part: "dotfiles",
            dry_run: false,
            verbose: false,
            yes: true,
        };
        system::edits::apply(&config, &edits, &opts)?;
    }
    Ok(())
}
