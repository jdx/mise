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
use crate::path::PathExt;
use crate::system::managed_files::{ManagedDirectoryRequest, ManagedFileRequest, ManagedState};
use crate::system::resources::ResourceAction;
use crate::system::services_common::ServiceState;
use crate::system::{edits, files, managed_files, secrets, user_services};

#[derive(Debug, Default)]
pub struct UnapplyOpts {
    pub dry_run: bool,
    /// Remove targets whose current state no longer matches their declaration.
    pub force: bool,
    pub verbose: bool,
}

/// A resource this unapply removes.
#[derive(Debug, Eq, PartialEq)]
pub struct Removal {
    pub kind: &'static str,
    pub name: String,
}

/// A resource the environment contributes that this unapply leaves alone.
#[derive(Debug, Eq, PartialEq)]
pub struct Skip {
    pub kind: &'static str,
    pub name: String,
    pub reason: String,
}

/// Declarations in sections that keep their own removal commands.
#[derive(Debug, Eq, PartialEq)]
pub struct Uncovered {
    pub section: &'static str,
    pub count: usize,
    pub command: &'static str,
}

#[derive(Debug, Default)]
pub struct Unapply {
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
    pub fn is_empty(&self) -> bool {
        self.removals.is_empty()
    }
}

/// Plan the removal of everything `environments` adds to the desired state.
pub async fn plan(
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
    // Only a declaration of presence keeps a resource: one the rest of the
    // configuration declares absent is not shared with the module, it is
    // something the machine is meant to be rid of either way.
    let base_file_paths = paths(
        base_files
            .iter()
            .filter(|file| file.state == ManagedState::Present)
            .map(|file| &file.path),
    );
    let base_directory_paths = paths(
        base_directories
            .iter()
            .filter(|directory| directory.state == ManagedState::Present)
            .map(|directory| &directory.path),
    );
    files.retain(|file| !base_file_paths.contains(&file.path));
    directories.retain(|directory| !base_directory_paths.contains(&directory.path));

    for mut file in files {
        // A template that renders empty applied nothing, so like a declared
        // absence there is nothing for unapply to undo. `--force` does not
        // change that: it covers a target that drifted from what the module
        // wrote, and an empty render leaves nothing to compare the target with.
        let reason = if file.rendered_empty() {
            "template rendered empty, nothing was applied for it"
        } else {
            DECLARED_ABSENT
        };
        if declares_absence(file.state, "file", &file.path, reason, opts, &mut unapply) {
            continue;
        }
        // The module only set this file's permissions; its content, and so
        // the file itself, belongs to something else. Say so every time: the
        // permissions it set stay behind.
        if file.is_metadata_only() {
            unapply.skipped.push(Skip {
                kind: "file",
                name: file.path.to_string_lossy().into_owned(),
                reason: "its content is not managed by mise, only its permissions".into(),
            });
            continue;
        }
        let Some(removal) = classify(&file.plan()?.action, "file", &file.path, opts, &mut unapply)
        else {
            continue;
        };
        file.state = ManagedState::Absent;
        unapply.files.push(file);
        unapply.removals.push(removal);
    }
    let base_services = user_services::requests_from_config(&base)?
        .into_iter()
        .filter(|request| request.state != ServiceState::Absent)
        .map(|request| request.name)
        .collect::<HashSet<_>>();
    // Without a service manager there is nothing installed to remove, and
    // failing here would block the rest of the module's removal.
    let services_available = user_services::is_available();
    let mut service_candidates = vec![];
    for request in user_services::requests_from_config(config)? {
        if base_services.contains(&request.name) {
            continue;
        }
        if request.state == ServiceState::Absent {
            if opts.verbose {
                unapply.skipped.push(Skip {
                    kind: "user-service",
                    name: request.name,
                    reason: "declared absent, nothing was installed for it".into(),
                });
            }
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
        service_candidates.push(request);
    }
    // Removal deletes the installed unit, agent, or task by name, so the
    // installed definition has to be compared with the declaration first. One
    // service mise cannot render or query is reported like any other service it
    // keeps, rather than stopping the rest of the module from being removed.
    for request in service_candidates {
        let name = request.name.clone();
        let status = match user_services::status(std::slice::from_ref(&request)).await {
            Ok(mut statuses) => statuses.pop(),
            Err(error) => {
                unapply.skipped.push(Skip {
                    kind: "user-service",
                    name,
                    reason: format!("{error}"),
                });
                continue;
            }
        };
        let Some(status) = status else {
            continue;
        };
        if status.not_installed() {
            if opts.verbose {
                unapply.skipped.push(Skip {
                    kind: "user-service",
                    name: status.name,
                    reason: "already absent".into(),
                });
            }
            continue;
        }
        if !status.matches_declaration() && !opts.force {
            unapply.skipped.push(Skip {
                kind: "user-service",
                name: status.name,
                reason: format!("{}; use --force to remove it", status.current),
            });
            continue;
        }
        unapply.removals.push(Removal {
            kind: "user-service",
            name: status.name.clone(),
        });
        unapply.user_services.push(status.name);
    }

    let base_dotfiles = paths(
        files::files_from_config(&base)?
            .iter()
            .map(|request| &request.target),
    );
    for request in files::files_from_config(config)? {
        if base_dotfiles.contains(&request.target) {
            continue;
        }
        let name = request.target_raw.clone();
        // The dotfile planner owns the rules for identifying a managed target.
        // Asking it about one entry at a time turns an entry it refuses into a
        // reported skip here, instead of an error raised once the rest of the
        // module has already been removed.
        let planned = match files::plan_unapply(std::slice::from_ref(&request), &dotfile_opts(opts))
        {
            Ok(plans) => Ok(!plans.is_empty()),
            Err(error) => Err(skip_reason(&error)),
        };
        match planned {
            Ok(true) => {
                unapply.removals.push(Removal {
                    kind: "dotfile",
                    name,
                });
                unapply.dotfiles.push(request);
            }
            Ok(false) => {
                if opts.verbose {
                    let reason = if request.mode == files::FileMode::Absent {
                        "declared absent, nothing was applied for it"
                    } else {
                        "already absent"
                    };
                    unapply.skipped.push(Skip {
                        kind: "dotfile",
                        name,
                        reason: reason.into(),
                    });
                }
            }
            Err(reason) => unapply.skipped.push(Skip {
                kind: "dotfile",
                name,
                reason,
            }),
        }
    }

    let base_edits = edits::edits_from_config(&base)?
        .iter()
        .map(edit_key)
        .collect::<HashSet<_>>();
    for request in edits::edits_from_config(config)? {
        if base_edits.contains(&edit_key(&request)) {
            continue;
        }
        let name = format!("{}/{}", request.path_raw, request.id);
        let planned = match edits::plan_unapply(std::slice::from_ref(&request), &edit_opts(opts)) {
            Ok(plans) => Ok(!plans.is_empty()),
            Err(error) => Err(skip_reason(&error)),
        };
        match planned {
            Ok(true) => {
                unapply.removals.push(Removal { kind: "edit", name });
                unapply.edits.push(request);
            }
            Ok(false) => {
                if opts.verbose {
                    unapply.skipped.push(Skip {
                        kind: "edit",
                        name,
                        reason: "already applied or absent".into(),
                    });
                }
            }
            Err(reason) => unapply.skipped.push(Skip {
                kind: "edit",
                name,
                reason,
            }),
        }
    }

    // Directories come last: removal is never recursive, so whether one can be
    // removed depends on what everything else in this plan takes with it.
    // Learning otherwise during the apply would leave the module half removed.
    let mut scheduled = unapply
        .files
        .iter()
        .map(|file| file.path.clone())
        .chain(unapply.dotfiles.iter().map(|file| file.target.clone()))
        .collect::<HashSet<_>>();
    // Deeper paths first so a parent sees its removed children as gone.
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.path.components().count()));
    for mut directory in directories {
        if declares_absence(
            directory.state,
            "directory",
            &directory.path,
            DECLARED_ABSENT,
            opts,
            &mut unapply,
        ) {
            continue;
        }
        let Some(removal) = classify(
            &directory.plan()?.action,
            "directory",
            &directory.path,
            opts,
            &mut unapply,
        ) else {
            continue;
        };
        let reason = match directory_contents(&directory.path, &scheduled) {
            DirectoryContents::Removable => None,
            DirectoryContents::Retains(remaining) => {
                Some(format!("not empty, {} remains", remaining.display_user()))
            }
            DirectoryContents::Uninspectable(error) => Some(format!("cannot read it, {error}")),
        };
        if let Some(reason) = reason {
            unapply.skipped.push(Skip {
                kind: "directory",
                name: directory.path.to_string_lossy().into_owned(),
                reason,
            });
            continue;
        }
        scheduled.insert(directory.path.clone());
        directory.state = ManagedState::Absent;
        unapply.directories.push(directory);
        unapply.removals.push(removal);
    }

    unapply.uncovered = uncovered_sections(config, environments);
    Ok(unapply)
}

/// What a directory holds that this plan does not remove.
enum DirectoryContents {
    /// Nothing, once the rest of this plan has run.
    Removable,
    /// An entry this plan leaves behind. The declaration describes the
    /// directory, not whatever else ended up inside it.
    Retains(PathBuf),
    /// The directory could not be read, so what it holds is unknown. Removal is
    /// not recursive and would fail on a non-empty directory, after the other
    /// domains have already changed the machine.
    Uninspectable(String),
}

fn directory_contents(path: &Path, scheduled: &HashSet<PathBuf>) -> DirectoryContents {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return DirectoryContents::Uninspectable(error.to_string()),
    };
    for entry in entries {
        match entry {
            Ok(entry) if !scheduled.contains(&entry.path()) => {
                return DirectoryContents::Retains(entry.path());
            }
            Ok(_) => {}
            Err(error) => return DirectoryContents::Uninspectable(error.to_string()),
        }
    }
    DirectoryContents::Removable
}

/// Whether this declaration describes an absence rather than something the
/// module applied. Undoing `state = "absent"` would mean creating the resource,
/// which is not what removing a module means.
fn declares_absence(
    state: ManagedState,
    kind: &'static str,
    path: &Path,
    reason: &str,
    opts: &UnapplyOpts,
    unapply: &mut Unapply,
) -> bool {
    if state != ManagedState::Absent {
        return false;
    }
    if opts.verbose {
        unapply.skipped.push(Skip {
            kind,
            name: path.to_string_lossy().into_owned(),
            reason: reason.into(),
        });
    }
    true
}

const DECLARED_ABSENT: &str = "declared absent, nothing was applied for it";

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
        // `--force` covers a target that drifted, never one whose type is not
        // what the declaration describes: the apply refuses those, and failing
        // there would leave the module partly removed.
        ResourceAction::Unknown => {
            unapply.skipped.push(Skip {
                kind,
                name,
                reason: "unexpected path type".into(),
            });
            None
        }
        _ if opts.force => Some(Removal { kind, name }),
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

fn dotfile_opts(opts: &UnapplyOpts) -> files::UnapplyOpts {
    files::UnapplyOpts {
        dry_run: opts.dry_run,
        verbose: opts.verbose,
        force: opts.force,
        // Confirmation covers the whole plan once, before any domain runs.
        yes: true,
    }
}

fn edit_opts(opts: &UnapplyOpts) -> edits::UnapplyOpts {
    edits::UnapplyOpts {
        dry_run: opts.dry_run,
        verbose: opts.verbose,
        force: opts.force,
        yes: true,
    }
}

/// The reason one entry was refused, without the heading and entry prefix the
/// domain planners add when they report a batch.
fn skip_reason(error: &eyre::Report) -> String {
    let text = format!("{error}");
    let line = text.lines().next_back().unwrap_or_default().trim();
    line.find("\": ")
        .or_else(|| line.find("): "))
        .map(|index| &line[index + 3..])
        .unwrap_or(line)
        .to_string()
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
    // Independent bootstrap roots keep their own config maps, and a module can
    // declare all of its packages in one of them.
    for config_files in config.bootstrap_config_maps() {
        for (path, config_file) in config_files {
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
pub async fn execute(
    config: &Config,
    unapply: &Unapply,
    secrets: &secrets::SecretValues,
    opts: &UnapplyOpts,
) -> Result<()> {
    let dotfile_opts = dotfile_opts(opts);
    let edit_opts = edit_opts(opts);

    // Validate every domain before any of them mutates the filesystem.
    let edit_plan = edits::plan_unapply(&unapply.edits, &edit_opts)?;
    let mut dotfile_plan = files::plan_unapply(&unapply.dotfiles, &dotfile_opts)?;
    files::resolve_unapply(config, &mut dotfile_plan, &dotfile_opts, secrets)?;
    edits::validate_unapply(&edit_plan)?;

    if !unapply.edits.is_empty() {
        edits::execute_unapply(&edit_plan, &edit_opts)?;
    }
    if !unapply.dotfiles.is_empty() {
        files::execute_unapply(config, &dotfile_plan, &dotfile_opts)?;
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
