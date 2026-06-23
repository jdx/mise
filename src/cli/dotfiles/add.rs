use std::path::PathBuf;

use eyre::{Result, bail};
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{Config, ConfigPathOptions, Settings, resolve_target_config_path};
use crate::file;
use crate::path::PathExt;
use crate::system;
use crate::system::files::{FileMode, FileRequest};
use crate::ui::prompt;

/// Add or update dotfiles in `[dotfiles]`
///
/// If the target is already managed, this updates its source from the live
/// target. Otherwise it creates a `[dotfiles]` entry and seeds the source
/// under `dotfiles.root` unless `--source` is provided.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct DotfilesAdd {
    /// Targets to add or update
    #[clap(value_name = "TARGET", required = true)]
    pub(super) targets: Vec<String>,

    /// Overwrite existing sources without prompting
    #[clap(long, short)]
    pub(super) force: bool,

    /// Write to the global config
    #[clap(long, short, conflicts_with_all = ["local", "path"])]
    pub(super) global: bool,

    /// Write to the local config instead of the global config
    #[clap(long, short, conflicts_with_all = ["global", "path"])]
    pub(super) local: bool,

    /// Dotfile mode to write
    #[clap(long, short)]
    pub(super) mode: Option<String>,

    /// Print the config/source updates without writing anything
    #[clap(long, short = 'n')]
    pub(super) dry_run: bool,

    /// Write to this config file or directory
    #[clap(long, short, value_name = "PATH", conflicts_with_all = ["global", "local"])]
    pub(super) path: Option<PathBuf>,

    /// Source path to use for a single target
    #[clap(long, short, value_name = "PATH")]
    pub(super) source: Option<PathBuf>,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    pub(super) yes: bool,
}

impl DotfilesAdd {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise dotfiles")?;
        if self.source.is_some() && self.targets.len() != 1 {
            bail!("--source can only be used with one target");
        }
        let mode = match self.mode.as_deref() {
            Some(mode) => {
                FileMode::parse(mode).ok_or_else(|| eyre::eyre!("unknown dotfile mode: {mode}"))?
            }
            None => system::files::default_mode(),
        };
        let config = Config::get().await?;
        let managed = system::files::files_from_config(&config);
        let config_path = resolve_target_config_path(ConfigPathOptions {
            global: self.global || !self.local,
            path: self.path.clone(),
            env: None,
            cwd: None,
            prefer_toml: true,
            prevent_home_local: true,
        })?;

        let mut planned = vec![];
        let managed_edits = system::edits::edits_from_config(&config);
        for target_raw in &self.targets {
            let target = system::files::resolve_target_arg(target_raw);
            if target.is_relative() {
                bail!("{target_raw}: target must be absolute or start with ~/");
            }
            if managed_edits.iter().any(|req| {
                system::files::matches_target(
                    &req.path,
                    &req.path_raw,
                    std::slice::from_ref(target_raw),
                )
            }) {
                bail!(
                    "{target_raw}: target is already managed by [dotfiles] edits; remove or rename those entries before adding a whole-file dotfile"
                );
            }
            let existing = managed.iter().find(|req| {
                system::files::matches_target(
                    &req.target,
                    &req.target_raw,
                    std::slice::from_ref(target_raw),
                )
            });
            let source = if let Some(req) = existing {
                req.source.clone()
            } else if let Some(source) = &self.source {
                file::replace_path(source)
            } else {
                system::files::implied_source(&target)?
            };
            let write_mode = existing.map(|req| req.mode).unwrap_or(mode);
            if let Some(req) = existing
                && self.mode.is_some()
                && req.mode != mode
            {
                warn!(
                    "dotfiles: {} is already managed with mode {}; --mode {} was ignored",
                    target_raw,
                    req.mode.name(),
                    mode.name()
                );
            }
            planned.push(PlannedAdd {
                target_raw: target_raw.clone(),
                target,
                source,
                mode: write_mode,
                implied_source: self.source.is_none(),
                already_managed: existing.cloned(),
            });
        }

        if self.dry_run {
            for item in &planned {
                if item.already_managed.is_none() {
                    miseprintln!(
                        "{}: \"{}\" = {}",
                        config_path.display_user(),
                        item.target_raw,
                        inline_entry(item)
                    );
                }
                if item.target.exists() {
                    miseprintln!(
                        "cp {} {}",
                        item.target.display_user(),
                        item.source.display_user()
                    );
                }
            }
            return Ok(());
        }

        let writes_config = planned.iter().any(|item| item.already_managed.is_none());
        let mut doc = if writes_config {
            if !config_path.exists() {
                let cf = MiseToml::init(&config_path);
                cf.save()?;
            }
            let raw = file::read_to_string(&config_path)?;
            let mut doc: DocumentMut = raw.parse()?;
            ensure_dotfiles_table(&mut doc);
            Some(doc)
        } else {
            None
        };

        let mut added_targets = vec![];
        let mut updated_targets = vec![];
        for item in &planned {
            if item.target.exists() && !same_file(&item.target, &item.source) {
                if item.source.exists()
                    && !self.force
                    && !self.yes
                    && console::user_attended_stderr()
                {
                    let ok = prompt::confirm(format!(
                        "dotfiles: overwrite source {} from {}?",
                        item.source.display_user(),
                        item.target.display_user()
                    ))?;
                    if !ok {
                        info!("dotfiles: skipped {}", item.target_raw);
                        continue;
                    }
                }
                system::files::copy_path(&item.target, &item.source)?;
            } else if !item.source.exists() {
                if let Some(parent) = item.source.parent() {
                    file::create_dir_all(parent)?;
                }
                file::write(&item.source, "")?;
            }
            if item.already_managed.is_none()
                && let Some(doc) = &mut doc
            {
                write_entry(doc, item);
                added_targets.push(item.target_raw.as_str());
            } else {
                updated_targets.push(item.target_raw.as_str());
            }
        }

        if let Some(doc) = doc {
            file::write(&config_path, doc.to_string())?;
            if !added_targets.is_empty() {
                info!(
                    "{}: added {}",
                    config_path.display_user(),
                    added_targets.join(", ")
                );
            }
        }
        if !updated_targets.is_empty() {
            info!("dotfiles: updated {}", updated_targets.join(", "));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct PlannedAdd {
    target_raw: String,
    target: PathBuf,
    source: PathBuf,
    mode: FileMode,
    implied_source: bool,
    already_managed: Option<FileRequest>,
}

fn ensure_dotfiles_table(doc: &mut DocumentMut) {
    if !doc.as_table().contains_key("dotfiles") {
        doc["dotfiles"] = Item::Table(Table::new());
    }
}

fn write_entry(doc: &mut DocumentMut, item: &PlannedAdd) {
    doc["dotfiles"][&item.target_raw] = Item::Value(inline_entry(item));
}

fn inline_entry(item: &PlannedAdd) -> Value {
    let mut table = InlineTable::new();
    if !item.implied_source {
        table.insert(
            "source",
            Value::String(toml_edit::Formatted::new(
                item.source.display_user().to_string(),
            )),
        );
    } else if let Some(req) = &item.already_managed
        && !system::files::source_is_implied(req)
    {
        table.insert(
            "source",
            Value::String(toml_edit::Formatted::new(
                item.source.display_user().to_string(),
            )),
        );
    }
    table.insert(
        "mode",
        Value::String(toml_edit::Formatted::new(item.mode.name().to_string())),
    );
    Value::InlineTable(table)
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise dotfiles add ~/.zshrc</bold>
    $ <bold>mise dotfiles add --mode copy ~/.config/starship.toml</bold>
    $ <bold>mise dotfiles add --source dotfiles/gitconfig ~/.gitconfig</bold>
"#
);
