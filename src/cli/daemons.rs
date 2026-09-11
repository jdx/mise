use crate::config::{Config, Settings};
use crate::daemons::{
    self,
    runtime::{self, Runtime},
};
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
        let loaded = config.daemons()?;
        let mut roots = loaded.roots();
        if !roots.iter().any(|r| r == root) {
            roots.push(root.to_path_buf());
        }
        let mut rows = Vec::new();
        let mut matched = false;
        for root in roots {
            let scoped = runtime::config_for_root(&config, &root).await?;
            let set = scoped.daemons()?.for_root(&root);
            let previous = runtime::read_state(&root)?;
            if set.daemons.is_empty() && previous.ids.is_empty() {
                continue;
            }
            let install = matches!(action, "start" | "restart");
            let (scoped, ts) = runtime::toolset(&scoped, install).await?;
            let runtime = Runtime::from_toolset(&scoped, &ts, Some(&previous.bin)).await;
            if action == "ls" {
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
                    rows.push(serde_json::json!({ "id": id, "name": name, "source": daemon.map(|d| &d.source), "preset": daemon.and_then(|d| d.preset.as_ref()), "status": status.as_ref().and_then(|s| s["status"].as_str()).unwrap_or("available"), "pid": status.as_ref().and_then(|s| s["pid"].as_u64()) }));
                }
                continue;
            }
            let runtime = runtime?;
            if install {
                runtime::validate_tools(&set, &scoped, &ts).await?;
            }
            let (state, _project_lock) = if install {
                let (state, lock) = runtime.prepare(&root, &set).await?;
                (state, Some(lock))
            } else {
                (previous, None)
            };
            let mut selected = Vec::new();
            let mut flags = Vec::new();
            for arg in &args {
                if let Some(id) = state
                    .ids
                    .iter()
                    .find(|id| *id == arg || id.rsplit('/').next() == Some(arg.as_str()))
                {
                    selected.push(id.clone());
                } else {
                    flags.push(arg.clone());
                }
            }
            if selected.is_empty()
                && args.first().is_some_and(|a| !a.starts_with('-'))
                && flags.len() == args.len()
            {
                continue;
            }
            if selected.is_empty() {
                selected = state.ids.clone();
            }
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
                forwarded.extend(flags);
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
