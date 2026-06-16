use std::path::PathBuf;

use eyre::{Result, bail};

use super::add::DotfilesAdd;
use crate::config::{Config, Settings};
use crate::file;
use crate::system;
use crate::system::edits::{BlockSource, EditOp};
use crate::ui::prompt;

/// Edit a managed dotfile source
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct DotfilesEdit {
    /// Target to edit
    #[clap(value_name = "TARGET")]
    target: String,

    /// Apply this target after the editor exits
    #[clap(long)]
    apply: bool,

    /// Dotfile mode to use if the target is not yet managed
    #[clap(long, short)]
    mode: Option<String>,

    /// Source path to use if the target is not yet managed
    #[clap(long, short, value_name = "PATH")]
    source: Option<PathBuf>,

    /// Skip the confirmation prompt when adding an unmanaged target
    #[clap(long, short)]
    yes: bool,
}

impl DotfilesEdit {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise dotfiles")?;
        let mut config = Config::get().await?;
        let target = system::files::resolve_target_arg(&self.target);

        if let Some(path) = source_for_target(&config, &target, &self.target)? {
            open_or_create(&path)?;
            super::open_in_editor(&path)?;
            if self.apply {
                apply_target(&self.target).await?;
            }
            return Ok(());
        }

        if !self.yes && console::user_attended_stderr() {
            let ok = prompt::confirm(format!("dotfiles: add {}?", self.target))?;
            if !ok {
                info!("dotfiles: skipped");
                return Ok(());
            }
        } else if !self.yes {
            bail!("{} is not managed by [dotfiles]", self.target);
        }

        DotfilesAdd {
            targets: vec![self.target.clone()],
            mode: self.mode.clone(),
            source: self.source.clone(),
            global: true,
            local: false,
            path: None,
            dry_run: false,
            force: false,
            yes: true,
        }
        .run()
        .await?;

        config = Config::reset().await?;
        let Some(path) = source_for_target(&config, &target, &self.target)? else {
            bail!("failed to add {}", self.target);
        };
        open_or_create(&path)?;
        super::open_in_editor(&path)?;
        if self.apply {
            apply_target(&self.target).await?;
        }
        Ok(())
    }
}

fn source_for_target(
    config: &Config,
    target: &std::path::Path,
    raw: &str,
) -> Result<Option<PathBuf>> {
    for req in system::files::files_from_config(config) {
        if system::files::matches_target(&req.target, &req.target_raw, &[raw.to_string()]) {
            return Ok(Some(req.source));
        }
    }
    let matching_edits = system::edits::edits_from_config(config)
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

fn open_or_create(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write(path, "")?;
    }
    Ok(())
}

async fn apply_target(target: &str) -> Result<()> {
    let config = Config::reset().await?;
    let targets = vec![target.to_string()];
    let files = system::files::files_from_config(&config)
        .into_iter()
        .filter(|req| system::files::matches_target(&req.target, &req.target_raw, &targets))
        .collect::<Vec<_>>();
    let edits = system::edits::edits_from_config(&config)
        .into_iter()
        .filter(|req| system::edits::matches_target(req, &targets))
        .collect::<Vec<_>>();
    if !files.is_empty() {
        let opts = system::files::ApplyOpts {
            dry_run: false,
            verbose: false,
            force: false,
            force_hint: "use `mise dotfiles apply --force`",
            yes: true,
        };
        system::files::apply(&config, &files, &opts)?;
    }
    if !edits.is_empty() {
        let opts = system::edits::ApplyOpts {
            dry_run: false,
            verbose: false,
            yes: true,
        };
        system::edits::apply(&config, &edits, &opts)?;
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise dotfiles edit ~/.zshrc</bold>
    $ <bold>mise dotfiles edit --apply ~/.config/starship.toml</bold>
"#
);
