use crate::config::{Config, Settings};
use crate::daemons::{
    self,
    runtime::{self, Runtime},
};
use crate::file::display_path;
use crate::ui::prompt::{self, Confirmation};
use eyre::{Result, bail};
use std::path::PathBuf;

/// [experimental] Manage project daemons with pitchfork
///
/// Define commands or managed Postgres/Redis presets in [daemons].
/// With no subcommand, list configured and previously managed daemons.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "daemon",
    args_conflicts_with_subcommands = true,
    verbatim_doc_comment
)]
pub(crate) struct Daemons {
    #[usage(subcommand)]
    command: Option<Commands>,
    #[usage(flatten)]
    list: List,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Start(Args),
    Stop(Args),
    Restart(Args),
    #[usage(visible_alias = "list")]
    Ls(List),
    Logs(Args),
    Status(Args),
    Tui(TuiArgs),
    Prune(Prune),
    #[usage(name = "__init", hide = true)]
    Init(Init),
}

/// Arguments passed to pitchfork; daemon names may be short or qualified.
#[derive(Debug, usage_rs::Args)]
#[usage(unknown_flags = "value")]
struct Args {
    #[usage(allow_hyphen_values = true, trailing_var_arg = true)]
    args: Vec<String>,
}

/// Open pitchfork's dashboard with optional pitchfork TUI flags.
#[derive(Debug, usage_rs::Args)]
#[usage(unknown_flags = "value")]
struct TuiArgs {
    #[usage(allow_hyphen_values = true, trailing_var_arg = true)]
    args: Vec<String>,
}

/// List project daemons without starting a supervisor or registering configuration.
#[derive(Debug, Default, usage_rs::Args)]
struct List {
    #[usage(long)]
    json: bool,
}

/// Remove daemon state left behind by deleted project directories.
///
/// Each project root keeps generated pitchfork configuration and daemon data
/// under $MISE_STATE_DIR/daemons. Deleting a project (for example with
/// `git worktree remove`) leaves both behind. This stops those daemons,
/// unregisters their configuration, and deletes their data. State for projects
/// that still exist is never removed.
///
/// Prompts before deleting anything; pass the global --yes to prune
/// non-interactively.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct Prune {
    /// Show what would be removed without deleting anything
    #[usage(long, short = 'n')]
    dry_run: bool,
}

#[derive(Debug, usage_rs::Args)]
struct Init {
    preset: String,
    data: PathBuf,
    database: String,
}

impl Daemons {
    pub(crate) fn starts(&self) -> bool {
        matches!(
            self.command,
            Some(Commands::Start(_) | Commands::Restart(_))
        )
    }

    pub(crate) async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise daemons")?;
        Settings::ensure_not_safe("managing daemons")?;
        let (action, args, json) = match self.command {
            Some(Commands::Init(args)) => {
                return daemons::presets::initialize(&args.preset, &args.data, &args.database);
            }
            Some(Commands::Prune(args)) => return args.run().await,
            Some(Commands::Start(args)) => ("start", args.args, false),
            Some(Commands::Stop(args)) => ("stop", args.args, false),
            Some(Commands::Restart(args)) => ("restart", args.args, false),
            Some(Commands::Logs(args)) => ("logs", args.args, false),
            Some(Commands::Status(args)) => ("status", args.args, false),
            Some(Commands::Tui(args)) => ("tui", args.args, false),
            Some(Commands::Ls(list)) => ("ls", vec![], list.json),
            None => ("ls", vec![], self.list.json),
        };
        let config = Config::get().await?;
        let root = config
            .project_root
            .as_deref()
            .ok_or_else(|| eyre::eyre!("mise daemons requires a project configuration"))?;
        // The dashboard is global to the supervisor, not one invocation per daemon root.
        if action == "tui" {
            if args.first().is_some_and(|arg| !arg.starts_with('-')) {
                bail!("mise daemons tui opens the dashboard; pass TUI flags, not daemon names");
            }
            let previous = runtime::read_state(root)?;
            let (config, ts) = runtime::toolset(&config, false).await?;
            let runtime = Runtime::from_toolset(&config, &ts, Some(&previous.bin)).await?;
            if !runtime.supervisor_up(root).await? {
                bail!("pitchfork supervisor is not running; run mise daemons start");
            }
            return runtime
                .exec(root, [vec!["tui".into()], args].concat())
                .await;
        }
        if action == "start" {
            hint_prunable_state();
        }
        let loaded = config.daemons()?;
        let mut roots = loaded.roots();
        if !roots.iter().any(|r| r == root) {
            roots.push(root.to_path_buf());
        }
        let (names, flags) = split_args(action, &args)?;
        let install = matches!(action, "start" | "restart");
        let mut root_ids = Vec::new();
        for root in &roots {
            let previous = runtime::read_state(root)?;
            let namespace = if previous.namespace.is_empty() {
                runtime::namespace(root)?
            } else {
                previous.namespace.clone()
            };
            let mut ids = if install { Vec::new() } else { previous.ids };
            ids.extend(
                loaded
                    .for_root(root)
                    .daemons
                    .keys()
                    .map(|name| format!("{namespace}/{name}")),
            );
            root_ids.push(ids);
        }
        // Validate the entire request before any root installs tools or changes state.
        for name in &names {
            if !root_ids.iter().flatten().any(|id| matches_name(id, name)) {
                bail!("no matching project daemons for {name:?}");
            }
        }
        let mut rows = Vec::new();
        let mut matched = false;
        for (root, ids) in roots.into_iter().zip(root_ids) {
            if !names.is_empty()
                && !ids
                    .iter()
                    .any(|id| names.iter().any(|name| matches_name(id, name)))
            {
                continue;
            }
            let scoped = runtime::config_for_root(&config, &root).await?;
            let set = scoped.daemons()?.for_root(&root);
            let previous = runtime::read_state(&root)?;
            if set.daemons.is_empty() && previous.ids.is_empty() {
                continue;
            }
            let (scoped, ts) = runtime::toolset(&scoped, install).await?;
            let runtime = Runtime::from_toolset(&scoped, &ts, Some(&previous.bin)).await;
            if action == "ls" {
                // Reported per root so a developer can see what a worktree costs
                // before deleting it (or before running `mise daemons prune`).
                let state_dir = daemons::state_dir(&root);
                let data_size = daemons::prune::dir_size(&state_dir.join("data"));
                let mut ids = previous.ids.clone();
                for name in set.daemons.keys() {
                    let id = if previous.namespace.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{name}", previous.namespace)
                    };
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                for id in ids {
                    let name = id.rsplit('/').next().unwrap_or(&id);
                    let daemon = set.daemons.get(name);
                    let status = if let Ok(runtime) = &runtime
                        && !previous.namespace.is_empty()
                    {
                        runtime.status(&root, &id).await.ok()
                    } else {
                        None
                    };
                    rows.push(serde_json::json!({ "id": id, "name": name, "source": daemon.map(|d| &d.source), "preset": daemon.and_then(|d| d.preset.as_ref()), "status": status.as_ref().and_then(|s| s["status"].as_str()).unwrap_or("available"), "pid": status.as_ref().and_then(|s| s["pid"].as_u64()), "root": root, "state_dir": state_dir, "data_size": data_size, "data_size_human": daemons::prune::human_size(data_size) }));
                }
                continue;
            }
            let runtime = runtime?;
            if install {
                runtime::validate_tools(&set, &scoped, &ts).await?;
            }
            let (state, _project_lock) = if install {
                let (state, lock) = runtime.prepare(&root, &set, true).await?;
                (state, Some(lock))
            } else {
                (previous, None)
            };
            let mut selected: Vec<_> = state
                .ids
                .iter()
                .filter(|id| names.is_empty() || names.iter().any(|name| matches_name(id, name)))
                .cloned()
                .collect();
            if install {
                selected.retain(|id| {
                    set.daemons
                        .contains_key(id.rsplit('/').next().unwrap_or(id))
                });
            }
            if selected.is_empty() {
                continue;
            }
            matched = true;
            if action == "logs" && !runtime.supervisor_up(&root).await? {
                bail!("pitchfork supervisor is not running; run mise daemons start");
            }
            if action == "status" {
                for id in selected {
                    runtime
                        .exec(&root, [vec!["status".into(), id], flags.clone()].concat())
                        .await?;
                }
            } else {
                let mut forwarded = vec![action.into()];
                forwarded.extend(selected);
                forwarded.extend(flags.clone());
                runtime.exec(&root, forwarded).await?;
            }
        }
        if action == "ls" {
            if json {
                miseprintln!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                let mut table =
                    crate::ui::table::MiseTable::new(false, &["Daemon", "Status", "Source"]);
                for row in rows {
                    table.add_row(vec![
                        comfy_table::Cell::new(row["id"].as_str().unwrap_or_default()),
                        comfy_table::Cell::new(row["status"].as_str().unwrap_or_default()),
                        comfy_table::Cell::new(row["source"].as_str().unwrap_or_default()),
                    ]);
                }
                table.print()?;
            }
        } else if !matched {
            bail!("no matching project daemons; define [daemons] in mise.toml");
        }
        Ok(())
    }
}

impl Prune {
    async fn run(self) -> Result<()> {
        let base = daemons::prune::base_dir();
        let orphans = daemons::prune::orphans(&base)?;
        if orphans.is_empty() {
            info!(
                "no daemon state from deleted projects under {}",
                display_path(&base)
            );
            return Ok(());
        }
        // A root under an unmounted volume is indistinguishable from a deleted
        // one on disk, so that case is only ever decided by a person. Without
        // anyone to ask, it is kept.
        let (mounted, unmounted): (Vec<_>, Vec<_>) = orphans
            .into_iter()
            .partition(|entry| !entry.root_may_be_unmounted());
        for entry in &unmounted {
            let what = if self.dry_run || !Settings::get().yes {
                "may be an unmounted volume rather than a deleted project"
            } else {
                "may be an unmounted volume rather than a deleted project; prune without --yes to decide"
            };
            warn!("{} {what}", display_path(&entry.state.root));
        }
        let mut orphans = mounted;
        if !Settings::get().yes || self.dry_run {
            orphans.extend(unmounted);
        }
        if orphans.is_empty() {
            return Ok(());
        }
        let sized: Vec<_> = orphans
            .into_iter()
            .map(|entry| {
                let size = daemons::prune::dir_size(&entry.dir);
                (entry, size)
            })
            .collect();
        for line in daemons::prune::describe(&sized) {
            if self.dry_run {
                info!("{line} {}", console::style("[dryrun]").bold());
            } else {
                info!("{line}");
            }
        }
        if self.dry_run {
            return Ok(());
        }
        let total: u64 = sized.iter().map(|(_, size)| size).sum();
        if !Settings::get().yes {
            let message = format!(
                "remove {} daemon state director{} and {} of data?",
                sized.len(),
                if sized.len() == 1 { "y" } else { "ies" },
                daemons::prune::human_size(total),
            );
            // Defaults to no: the data is gone for good once this proceeds.
            match prompt::confirm_with_default(message, false)? {
                Confirmation::Yes => {}
                // An unanswered prompt is a refusal, not a decision to delete.
                Confirmation::No | Confirmation::Unanswered => return Ok(()),
                Confirmation::Unavailable => bail!(
                    "mise daemons prune requires confirmation but there was nobody to ask; pass --yes to prune non-interactively"
                ),
            }
        }
        // Pitchfork is resolved once from the ambient configuration; each entry
        // falls back to the executable its own state recorded.
        let config = Config::get().await?;
        let (config, ts) = runtime::toolset(&config, false).await?;
        for (entry, size) in &sized {
            let runtime = Runtime::from_toolset(&config, &ts, Some(&entry.state.bin))
                .await
                .ok();
            if daemons::prune::remove(entry, runtime.as_ref()).await?
                == daemons::prune::Outcome::Removed
            {
                info!(
                    "removed {} ({})",
                    display_path(&entry.dir),
                    daemons::prune::human_size(*size)
                );
            }
        }
        Ok(())
    }
}

/// Points at daemon state whose project directory no longer exists. The current
/// project cannot be among them: it is the directory mise is running in. Nothing
/// is deleted here; pruning is always explicit.
fn hint_prunable_state() {
    let Ok(orphans) = daemons::prune::orphans(&daemons::prune::base_dir()) else {
        return;
    };
    let count = orphans.len();
    if count == 0 {
        return;
    }
    let plural = if count == 1 { "y" } else { "ies" };
    info!(
        "{count} daemon state director{plural} belong to deleted projects; run `mise daemons prune` to remove them"
    );
}

fn matches_name(id: &str, name: &str) -> bool {
    id == name || id.rsplit('/').next() == Some(name)
}

/// Separate positional IDs from pitchfork options before matching project roots.
/// Value-taking options must retain their values even when a value is a daemon name.
fn split_args(action: &str, args: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let mut names = Vec::new();
    let mut flags = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            names.extend(args.cloned());
            break;
        }
        if !arg.starts_with('-') {
            names.push(arg.clone());
            continue;
        }
        if matches!(action, "start" | "stop" | "restart")
            && (arg == "--group" || arg.starts_with("--group="))
        {
            bail!(
                "mise daemons selects project daemon names; use pitchfork directly for --group operations"
            );
        }
        flags.push(arg.clone());
        let takes_value = match action {
            "start" | "restart" => matches!(
                arg.as_str(),
                "--delay"
                    | "--output"
                    | "--http"
                    | "--port"
                    | "--cmd"
                    | "--health-cmd"
                    | "--health-http"
                    | "--health-port"
                    | "--expected-port"
                    | "--shell-pid"
            ),
            "logs" => matches!(
                arg.as_str(),
                "-n" | "-s"
                    | "--since"
                    | "-u"
                    | "--until"
                    | "--grep"
                    | "--regex"
                    | "--level"
                    | "--field"
                    | "--jq"
            ),
            _ => false,
        };
        let short_value = action == "logs"
            && !arg.starts_with("--")
            && arg
                .char_indices()
                .skip(1)
                .find(|(_, ch)| matches!(ch, 'n' | 's' | 'u'))
                .is_some_and(|(index, ch)| index + ch.len_utf8() == arg.len());
        if takes_value || short_value {
            let value = args
                .next()
                .ok_or_else(|| eyre::eyre!("{arg} requires a value"))?;
            flags.push(value.clone());
        } else if action == "start"
            && arg == "--bump"
            && args
                .clone()
                .next()
                .is_some_and(|value| value.parse::<u32>().is_ok())
        {
            flags.push(args.next().unwrap().clone());
        }
    }
    Ok((names, flags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_independent_of_flag_order_and_values() {
        for args in [vec!["--force", "missing"], vec!["missing", "--force"]] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            let (names, flags) = split_args("start", &args).unwrap();
            assert_eq!(names, ["missing"]);
            assert_eq!(flags, ["--force"]);
        }
        let args = ["--grep", "api", "--since=5m", "web", "-n", "20"].map(String::from);
        let (names, flags) = split_args("logs", &args).unwrap();
        assert_eq!(names, ["web"]);
        assert_eq!(flags, ["--grep", "api", "--since=5m", "-n", "20"]);
        let (names, flags) =
            split_args("logs", &["-fn".into(), "20".into(), "web".into()]).unwrap();
        assert_eq!(names, ["web"]);
        assert_eq!(flags, ["-fn", "20"]);
        assert!(split_args("logs", &["--grep".into()]).is_err());
        for action in ["start", "stop", "restart"] {
            for args in [vec!["--group", "web"], vec!["api", "--group=web"]] {
                let args = args.into_iter().map(String::from).collect::<Vec<_>>();
                assert!(split_args(action, &args).is_err());
            }
        }
        let (names, _) = split_args("start", &["--".into(), "missing".into()]).unwrap();
        assert_eq!(names, ["missing"]);
    }
}
