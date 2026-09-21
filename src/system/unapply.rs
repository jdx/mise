//! Remove the resources one config environment contributes.
//!
//! The removal set is the difference between the desired state with the
//! environment selected and the desired state without it. Nothing about an
//! earlier run is recorded: the module's own configuration is still on disk
//! when it is deselected, and that declaration is what describes its removal.
//! A resource the base configuration or another selected environment still
//! declares is therefore never a candidate.
//!
//! Removal is conservative. A target that no longer matches the declaration is
//! left in place unless `--force` is given, in the same way `mise dot unapply`
//! preserves files it cannot identify as managed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::config::Config;
use crate::system::managed_files::{ManagedDirectoryRequest, ManagedFileRequest, ManagedState};
use crate::system::resources::ResourceAction;
use crate::system::{edits, files, managed_files, secrets, user_services};

#[derive(Debug, Default)]
pub(crate) struct UnapplyOpts {
    pub dry_run: bool,
    /// Remove targets whose current state no longer matches their declaration.
    pub force: bool,
    pub verbose: bool,
}

/// A resource this unapply removes.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Removal {
    pub kind: &'static str,
    pub name: String,
}

/// A resource the environment contributes that this unapply leaves alone.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Skip {
    pub kind: &'static str,
    pub name: String,
    pub reason: String,
}

/// Declarations in sections that keep their own removal commands.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Uncovered {
    pub section: &'static str,
    pub count: usize,
    pub command: &'static str,
}

#[derive(Debug, Default)]
pub(crate) struct Unapply {
    files: Vec<ManagedFileRequest>,
    directories: Vec<ManagedDirectoryRequest>,
    user_services: Vec<String>,
    dotfiles: Vec<files::FileRequest>,
    edits: Vec<edits::EditRequest>,
    pub removals: Vec<Removal>,
    pub skipped: Vec<Skip>,
    pub uncovered: Vec<Uncovered>,
}

impl Unapply {
    pub(crate) fn is_empty(&self) -> bool {
        self.removals.is_empty()
    }
}

/// Plan the removal of everything `environments` adds to the desired state.
pub(crate) fn plan(
    config: &Config,
    environments: &[String],
    secrets: &secrets::SecretValues,
    opts: &UnapplyOpts,
) -> Result<Unapply> {
    let base = config.without_environments(environments);
    let mut unapply = Unapply::default();

    let (mut files, mut directories) = managed_files::requests_from_config(config, secrets)?;
    let (base_files, base_directories) =
        managed_files::prepare_requests_from_config(&base, secrets)?;
    let base_file_paths = paths(base_files.iter().map(|file| &file.path));
    let base_directory_paths = paths(base_directories.iter().map(|directory| &directory.path));
    files.retain(|file| !base_file_paths.contains(&file.path));
    directories.retain(|directory| !base_directory_paths.contains(&directory.path));

    for mut file in files {
        let Some(removal) = classify(&file.plan()?.action, "file", &file.path, opts, &mut unapply)
        else {
            continue;
        };
        file.state = ManagedState::Absent;
        unapply.files.push(file);
        unapply.removals.push(removal);
    }
    // Deeper paths first so a directory is empty by the time it is removed.
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.path.components().count()));
    for mut directory in directories {
        let Some(removal) = classify(
            &directory.plan()?.action,
            "directory",
            &directory.path,
            opts,
            &mut unapply,
        ) else {
            continue;
        };
        directory.state = ManagedState::Absent;
        unapply.directories.push(directory);
        unapply.removals.push(removal);
    }

    let base_services = user_services::requests_from_config(&base)?
        .into_iter()
        .map(|request| request.name)
        .collect::<HashSet<_>>();
    // Without a service manager there is nothing installed to remove, and
    // failing here would block the rest of the module's removal.
    let services_available = user_services::is_available();
    for request in user_services::requests_from_config(config)? {
        if base_services.contains(&request.name) {
            continue;
        }
        if !services_available {
            unapply.skipped.push(Skip {
                kind: "user-service",
                name: request.name,
                reason: user_services::unavailable_reason(),
            });
            continue;
        }
        unapply.removals.push(Removal {
            kind: "user-service",
            name: request.name.clone(),
        });
        unapply.user_services.push(request.name);
    }

    let base_dotfiles = paths(
        files::files_from_config(&base)?
            .iter()
            .map(|request| &request.target),
    );
    for request in files::files_from_config(config)? {
        if base_dotfiles.contains(&request.target)
            || missing(
                &request.target,
                "dotfile",
                &request.target_raw,
                opts,
                &mut unapply,
            )
        {
            continue;
        }
        unapply.removals.push(Removal {
            kind: "dotfile",
            name: request.target_raw.clone(),
        });
        unapply.dotfiles.push(request);
    }

    let base_edits = edits::edits_from_config(&base)?
        .iter()
        .map(edit_key)
        .collect::<HashSet<_>>();
    for request in edits::edits_from_config(config)? {
        let name = format!("{}/{}", request.path_raw, request.id);
        if base_edits.contains(&edit_key(&request))
            || missing(&request.path, "edit", &name, opts, &mut unapply)
        {
            continue;
        }
        unapply.removals.push(Removal { kind: "edit", name });
        unapply.edits.push(request);
    }

    unapply.uncovered = uncovered_sections(config, environments);
    Ok(unapply)
}

/// Decide whether a managed path can be removed from its current state.
///
/// `Noop` means the target still matches what the module declared, which is the
/// only state removal is certain about. A target that drifted needs `--force`,
/// and one that is missing or of an unexpected type is not this command's to
/// touch.
fn classify(
    action: &ResourceAction,
    kind: &'static str,
    path: &Path,
    opts: &UnapplyOpts,
    unapply: &mut Unapply,
) -> Option<Removal> {
    let name = path.to_string_lossy().into_owned();
    match action {
        ResourceAction::Noop => Some(Removal { kind, name }),
        ResourceAction::Create => {
            if opts.verbose {
                unapply.skipped.push(Skip {
                    kind,
                    name,
                    reason: "already absent".into(),
                });
            }
            None
        }
        _ if opts.force => Some(Removal { kind, name }),
        ResourceAction::Unknown => {
            unapply.skipped.push(Skip {
                kind,
                name,
                reason: "unexpected path type".into(),
            });
            None
        }
        _ => {
            unapply.skipped.push(Skip {
                kind,
                name,
                reason: "changed since it was applied; use --force".into(),
            });
            None
        }
    }
}

/// Whether a target is already gone, so promising to remove it would be a
/// no-op. A broken symlink still counts as present: unapply removes those.
fn missing(
    target: &Path,
    kind: &'static str,
    name: &str,
    opts: &UnapplyOpts,
    unapply: &mut Unapply,
) -> bool {
    if target.symlink_metadata().is_ok() {
        return false;
    }
    if opts.verbose {
        unapply.skipped.push(Skip {
            kind,
            name: name.to_string(),
            reason: "already absent".into(),
        });
    }
    true
}

fn paths<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> HashSet<PathBuf> {
    paths.cloned().collect()
}

fn edit_key(request: &edits::EditRequest) -> String {
    format!("{}\u{0}{}", request.path.display(), request.id)
}

/// Count the declarations in sections this command does not remove, so the
/// output can name the command that does.
fn uncovered_sections(config: &Config, environments: &[String]) -> Vec<Uncovered> {
    let mut packages = 0;
    let mut repos = 0;
    let mut compose = 0;
    for (path, config_file) in &config.config_files {
        if !crate::config::environments_for_config_path(path)
            .iter()
            .any(|environment| environments.iter().any(|name| name == environment))
        {
            continue;
        }
        let Some(bootstrap) = config_file.bootstrap_config() else {
            continue;
        };
        packages += bootstrap.packages.len();
        repos += bootstrap.repos.len();
        compose += bootstrap.compose.len();
    }
    [
        (
            "bootstrap.packages",
            packages,
            "mise bootstrap packages prune --manager <manager>",
        ),
        ("bootstrap.repos", repos, "remove the checkout"),
        (
            "bootstrap.compose",
            compose,
            "declare state = \"absent\" and apply",
        ),
    ]
    .into_iter()
    .filter(|(_, count, _)| *count > 0)
    .map(|(section, count, command)| Uncovered {
        section,
        count,
        command,
    })
    .collect()
}

/// Remove the planned resources.
pub(crate) async fn execute(
    config: &Config,
    unapply: &Unapply,
    secrets: &secrets::SecretValues,
    opts: &UnapplyOpts,
) -> Result<()> {
    let mut dotfile_opts = files::UnapplyOpts {
        dry_run: opts.dry_run,
        verbose: opts.verbose,
        force: opts.force,
        yes: true,
    };
    let edit_opts = edits::UnapplyOpts {
        dry_run: opts.dry_run,
        verbose: opts.verbose,
        force: opts.force,
        yes: true,
    };

    // Validate every domain before any of them mutates the filesystem.
    let edit_plan = edits::plan_unapply(&unapply.edits, &edit_opts)?;
    let mut dotfile_plan = files::plan_unapply(&unapply.dotfiles, &dotfile_opts)?;
    files::resolve_unapply(config, &mut dotfile_plan, &dotfile_opts, secrets)?;
    edits::validate_unapply(&edit_plan)?;

    if !unapply.edits.is_empty() {
        edits::execute_unapply(&edit_plan, &edit_opts)?;
    }
    if !unapply.dotfiles.is_empty() {
        dotfile_opts.yes = true;
        files::execute_unapply(&dotfile_plan, &dotfile_opts)?;
    }

    if !unapply.files.is_empty() || !unapply.directories.is_empty() {
        let accounts = if cfg!(target_os = "linux") {
            Some(crate::system::accounts::requests_from_config(config)?)
        } else {
            None
        };
        managed_files::apply_with_accounts(
            &unapply.files,
            &unapply.directories,
            accounts.as_ref(),
            false,
            opts.dry_run,
            true,
        )?;
    }

    for name in &unapply.user_services {
        let removed = user_services::remove_named(name, opts.dry_run).await?;
        let manager = user_services::manager_name();
        if !removed {
            info!("user service {name}: no {manager} installed");
        } else if opts.dry_run {
            info!("user service {name}: would remove its {manager}");
        } else {
            info!("user service {name}: removed its {manager}");
        }
    }
    Ok(())
}
