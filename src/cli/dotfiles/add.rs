use std::path::PathBuf;

use eyre::{Result, bail};
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{Config, ConfigPathOptions, resolve_target_config_path};
use crate::dirs;
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

    /// Add the entry without applying it
    #[clap(long)]
    pub(super) no_apply: bool,

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
            let target = system::files::resolve_target_arg(target_raw)
                .components()
                .collect::<PathBuf>();
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
                target_raw: normalized_target_raw(&target),
                target,
                source,
                mode: write_mode,
                implied_source: self.source.is_none(),
                explicit_mode: self.mode.is_some(),
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
                if !self.no_apply {
                    miseprintln!("{}", describe_apply(item));
                }
            }
            return Ok(());
        }

        let backup_dir = tempfile::tempdir()?;
        let original_config = if config_path.exists() {
            Some(file::read(&config_path)?)
        } else {
            None
        };
        let mut source_backups = vec![];
        let mut target_backups = vec![];
        let mut moved_targets = vec![];
        let mut apply_started = false;
        let result = (|| -> Result<()> {
            let mut accepted = vec![];
            let mut updated_targets = vec![];
            let mut apply_requests = vec![];
            for (index, item) in planned.iter().enumerate() {
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
                    source_backups.push((
                        item.source.clone(),
                        backup_path(
                            &item.source,
                            &backup_dir.path().join("sources").join(index.to_string()),
                        )?,
                    ));
                    if !self.no_apply
                        && item.mode == FileMode::Symlink
                        && item.target.is_dir()
                        && !item.target.is_symlink()
                    {
                        remove_path(&item.source)?;
                        if let Some(parent) = item.source.parent() {
                            file::create_dir_all(parent)?;
                        }
                        file::move_file(&item.target, &item.source)?;
                        moved_targets.push((item.target.clone(), item.source.clone()));
                        info!(
                            "dotfiles: moved {} to {}",
                            item.target.display_user(),
                            item.source.display_user()
                        );
                    } else {
                        target_backups.push((
                            item.target.clone(),
                            backup_path(
                                &item.target,
                                &backup_dir.path().join("targets").join(index.to_string()),
                            )?,
                        ));
                        system::files::copy_path(&item.target, &item.source)?;
                        info!(
                            "dotfiles: copied {} to {}",
                            item.target.display_user(),
                            item.source.display_user()
                        );
                    }
                } else if !item.source.exists() {
                    target_backups.push((
                        item.target.clone(),
                        backup_path(
                            &item.target,
                            &backup_dir.path().join("targets").join(index.to_string()),
                        )?,
                    ));
                    source_backups.push((item.source.clone(), PathBackup::Missing));
                    if let Some(parent) = item.source.parent() {
                        file::create_dir_all(parent)?;
                    }
                    file::write(&item.source, "")?;
                    info!("dotfiles: created {}", item.source.display_user());
                } else if !self.no_apply {
                    target_backups.push((
                        item.target.clone(),
                        backup_path(
                            &item.target,
                            &backup_dir.path().join("targets").join(index.to_string()),
                        )?,
                    ));
                }
                if item.already_managed.is_some() {
                    updated_targets.push(item.target_raw.as_str());
                }
                accepted.push(item);
                apply_requests.push(item.as_request(&config_path));
            }

            let apply_opts = system::files::ApplyOpts {
                dry_run: false,
                verbose: false,
                force: false,
                force_hint: "run `mise bootstrap dotfiles apply --force`",
                yes: true,
            };
            let apply_plan = if !self.no_apply && !apply_requests.is_empty() {
                Some(system::files::plan_apply(
                    &config,
                    &apply_requests,
                    &apply_opts,
                )?)
            } else {
                None
            };

            let added_targets = accepted
                .iter()
                .filter(|item| item.already_managed.is_none())
                .collect::<Vec<_>>();
            if !added_targets.is_empty() {
                if !config_path.exists() {
                    let cf = MiseToml::init(&config_path);
                    cf.save()?;
                }
                let raw = file::read_to_string(&config_path)?;
                let mut doc: DocumentMut = raw.parse()?;
                ensure_dotfiles_table(&mut doc);
                for item in &added_targets {
                    write_entry(&mut doc, item);
                }
                sort_dotfiles_table(&mut doc);
                file::write(&config_path, doc.to_string())?;
                for item in &added_targets {
                    info!(
                        "dotfiles: added {} to {}",
                        item.target_raw,
                        config_path.display_user()
                    );
                }
            }
            if !updated_targets.is_empty() {
                info!("dotfiles: updated {}", updated_targets.join(", "));
            }
            if let Some(plan) = apply_plan {
                apply_started = true;
                system::files::execute_apply(plan, &apply_opts)?;
            }
            Ok(())
        })();
        if let Err(err) = result {
            if let Err(rollback_err) = rollback_add(
                &source_backups,
                &target_backups,
                &moved_targets,
                apply_started,
                &config_path,
                original_config.as_deref(),
            ) {
                bail!("{err}\ndotfiles: rollback failed: {rollback_err}");
            }
            return Err(err);
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
    explicit_mode: bool,
    already_managed: Option<FileRequest>,
}

impl PlannedAdd {
    fn as_request(&self, config_path: &std::path::Path) -> FileRequest {
        FileRequest {
            target_raw: self.target_raw.clone(),
            target: self.target.clone(),
            source: self.source.clone(),
            mode: self.mode,
            exclude: vec![],
            base: config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf(),
        }
    }
}

fn ensure_dotfiles_table(doc: &mut DocumentMut) {
    if !doc.as_table().contains_key("dotfiles") {
        doc["dotfiles"] = Item::Table(Table::new());
    }
}

fn sort_dotfiles_table(doc: &mut DocumentMut) {
    if let Some(table) = doc["dotfiles"].as_table_mut() {
        table.sort_values();
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
    if item.explicit_mode || item.mode != FileMode::Symlink {
        table.insert(
            "mode",
            Value::String(toml_edit::Formatted::new(item.mode.name().to_string())),
        );
    }
    Value::InlineTable(table)
}

fn normalized_target_raw(target: &std::path::Path) -> String {
    let normalized = target.components().collect::<PathBuf>();
    match normalized.strip_prefix(*dirs::HOME) {
        Ok(rel) if !rel.as_os_str().is_empty() => format!("~/{}", rel.display()),
        Ok(_) => "~".to_string(),
        Err(_) => normalized.to_string_lossy().to_string(),
    }
}

fn describe_apply(item: &PlannedAdd) -> String {
    let source = item.source.display_user();
    let target = item.target.display_user();
    match item.mode {
        FileMode::Symlink => format!("ln -sf {source} {target}"),
        FileMode::SymlinkEach => format!("ln -sf {source}/* into {target}/"),
        FileMode::Copy if item.source.is_dir() => format!("cp -r {source} {target}"),
        FileMode::Copy => format!("cp {source} {target}"),
        FileMode::Template => format!("render {source} -> {target}"),
    }
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

enum PathBackup {
    Missing,
    Copied(PathBuf),
    Symlink(PathBuf),
}

fn backup_path(path: &std::path::Path, backup: &std::path::Path) -> Result<PathBackup> {
    if path.is_symlink() {
        Ok(PathBackup::Symlink(std::fs::read_link(path)?))
    } else if path.exists() {
        system::files::copy_path(path, backup)?;
        Ok(PathBackup::Copied(backup.to_path_buf()))
    } else {
        Ok(PathBackup::Missing)
    }
}

fn restore_paths(backups: &[(PathBuf, PathBackup)]) -> Result<()> {
    let mut errors = vec![];
    for (path, backup) in backups.iter().rev() {
        let result = (|| -> Result<()> {
            remove_path(path)?;
            match backup {
                PathBackup::Missing => {}
                PathBackup::Copied(backup) => system::files::copy_path(backup, path)?,
                PathBackup::Symlink(target) => {
                    if let Some(parent) = path.parent() {
                        file::create_dir_all(parent)?;
                    }
                    file::make_symlink(target, path)?;
                }
            }
            Ok(())
        })();
        if let Err(err) = result {
            errors.push(format!("{}: {err}", path.display_user()));
        }
    }
    if !errors.is_empty() {
        bail!("{}", errors.join("\n"));
    }
    Ok(())
}

fn remove_path(path: &std::path::Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        file::remove_file(path)?;
    } else if path.is_dir() {
        file::remove_all(path)?;
    }
    Ok(())
}

fn rollback_add(
    source_backups: &[(PathBuf, PathBackup)],
    target_backups: &[(PathBuf, PathBackup)],
    moved_targets: &[(PathBuf, PathBuf)],
    restore_targets: bool,
    config_path: &std::path::Path,
    original_config: Option<&[u8]>,
) -> Result<()> {
    let mut errors = vec![];
    for (target, source) in moved_targets.iter().rev() {
        let result = (|| -> Result<()> {
            remove_path(target)?;
            if let Some(parent) = target.parent() {
                file::create_dir_all(parent)?;
            }
            file::move_file(source, target)
        })();
        if let Err(err) = result {
            errors.push(format!(
                "moved target {} from {}: {err}",
                target.display_user(),
                source.display_user()
            ));
        }
    }
    if restore_targets && let Err(err) = restore_paths(target_backups) {
        errors.push(format!("targets: {err}"));
    }
    if let Err(err) = restore_paths(source_backups) {
        errors.push(format!("sources: {err}"));
    }
    let config_result = match original_config {
        Some(contents) => file::write(config_path, contents),
        None => remove_path(config_path),
    };
    if let Err(err) = config_result {
        errors.push(format!("config: {err}"));
    }
    if !errors.is_empty() {
        bail!("{}", errors.join("\n"));
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles add ~/.zshrc</bold>
    $ <bold>mise bootstrap dotfiles add --mode copy ~/.config/starship.toml</bold>
    $ <bold>mise bootstrap dotfiles add --source dotfiles/gitconfig ~/.gitconfig</bold>
"#
);
