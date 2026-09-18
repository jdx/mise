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
        // The loop below shadows `root` with each project root it prepares.
        let project_root = root.to_path_buf();
        let mut roots = loaded.roots();
        if !roots.iter().any(|r| r == root) {
            roots.push(root.to_path_buf());
        }
        let (requested_names, flags) = split_args(action, &args)?;
        // An import that could not be resolved is fatal only when this command
        // names it. Someone whose sibling checkout is missing can still list and
        // stop their own daemons; they are told what is unavailable and why.
        if let Some((name, err)) = loaded.import_errors.iter().find(|(name, _)| {
            // Only a bare name can be the key a project gave an import. An
            // unresolved import never got a qualified ID, so a request that has
            // one names some other daemon and must be taken literally.
            requested_names
                .iter()
                .any(|requested| requested == *name && !requested.contains('/'))
        }) {
            bail!("cannot resolve [daemons.{name}]: {err}");
        }
        for (name, err) in &loaded.import_errors {
            warn!("[daemons.{name}] is unavailable: {err}");
        }
        let install = matches!(action, "start" | "restart");
        let names: Vec<String> = requested_names
            .iter()
            .map(|name| {
                let resolved = loaded.resolve_alias(name);
                if name.contains('/') {
                    return Ok(resolved);
                }
                let owner = loaded
                    .daemons
                    .values()
                    .find(|daemon| {
                        loaded.namespace_for(&daemon.root).is_some_and(|namespace| {
                            resolved == format!("{namespace}/{}", daemon.name)
                        })
                    })
                    .map(|daemon| daemon.root.as_path())
                    .unwrap_or(root);
                let previous = runtime::read_state(owner)?;
                // Bare names still address registered daemons after a namespace
                // edit, so users can stop them before the next start migrates.
                // Explicit qualified IDs always retain their literal meaning.
                let namespace = if !install && !previous.namespace.is_empty() {
                    previous.namespace
                } else {
                    match loaded.namespace_for(owner) {
                        Some(namespace) => namespace.to_owned(),
                        None => runtime::namespace(owner)?,
                    }
                };
                let daemon_name = resolved.rsplit('/').next().unwrap_or(&resolved);
                Ok(format!("{namespace}/{daemon_name}"))
            })
            .collect::<Result<_>>()?;
        let mut root_ids = Vec::new();
        for root in &roots {
            let previous = runtime::read_state(root)?;
            let namespace = match loaded.namespace_for(root) {
                Some(namespace) => namespace.to_string(),
                None if previous.namespace.is_empty() => runtime::namespace(root)?,
                None => previous.namespace.clone(),
            };
            let mut ids = if install { Vec::new() } else { previous.ids };
            ids.extend(
                loaded
                    .for_root(root)
                    .daemons
                    .values()
                    .map(|daemon| format!("{namespace}/{}", daemon.name)),
            );
            root_ids.push(ids);
        }
        // Validate the entire request before any root installs tools or changes state.
        for (name, requested) in names.iter().zip(&requested_names) {
            if !root_ids.iter().flatten().any(|id| matches_name(id, name)) {
                bail!("no matching project daemons for {requested:?}");
            }
        }
        // Resolve dependencies against each owner's complete declarations, so
        // imported daemons can bring their own local dependencies with them.
        let mut owner_configs = std::collections::HashMap::new();
        let starting = if install {
            let mut candidates = daemons::DaemonSet::default();
            let mut index = 0;
            while index < roots.len() {
                let root = roots[index].clone();
                index += 1;
                let scoped = runtime::config_for_root(&config, &root).await?;
                let declarations = scoped.daemons()?;
                for dependency_root in declarations.roots() {
                    if !roots.contains(&dependency_root) {
                        roots.push(dependency_root);
                        root_ids.push(Vec::new());
                    }
                }
                let set = declarations.for_root(&root);
                for daemon in set.daemons.values() {
                    let namespace = set.namespace_for(&root).unwrap_or_default();
                    let id = format!("{namespace}/{}", daemon.name);
                    if let Some(other) = candidates.daemons.insert(id.clone(), daemon.clone())
                        && other.root != daemon.root
                    {
                        bail!(
                            "daemon {id} is declared in both {} and {}; give the projects distinct namespaces",
                            other.root.display(),
                            daemon.root.display()
                        );
                    }
                }
                candidates.namespaces.extend(set.namespaces);
                owner_configs.insert(root.clone(), scoped);
            }
            let requested = root_ids
                .iter()
                .flatten()
                .filter(|id| names.is_empty() || names.iter().any(|name| matches_name(id, name)))
                .cloned()
                .collect::<Vec<_>>();
            let starting = candidates.with_dependencies(&requested);
            // Dropping a dependency on an unresolved import keeps the generated
            // config valid, but starting the daemon anyway would run it without
            // something it declared it needs. Say which import is missing.
            if let Some((name, import)) = loaded
                .blocked
                .iter()
                .find(|(name, _)| starting.find(name).is_some())
            {
                bail!(
                    "daemon {name:?} depends on [daemons.{import}], which is unavailable: {}",
                    loaded.import_errors[import]
                );
            }
            starting
        } else {
            daemons::DaemonSet::default()
        };
        let mut pending = Vec::new();
        let mut rows = Vec::new();
        let mut matched = false;
        let mut root_entries: Vec<_> = roots.into_iter().zip(root_ids).collect();
        if install {
            // Startup holds project locks until execution. Acquire them in a
            // consistent order even when callers import the projects differently.
            root_entries.sort_by(|a, b| a.0.cmp(&b.0));
        }
        for (root, ids) in root_entries {
            if install {
                if starting.for_root(&root).daemons.is_empty()
                    && (!names.is_empty() || root != project_root)
                {
                    continue;
                }
            } else if !names.is_empty()
                && !ids
                    .iter()
                    .any(|id| names.iter().any(|name| matches_name(id, name)))
            {
                continue;
            }
            let scoped = match owner_configs.remove(&root) {
                Some(scoped) => scoped,
                None => runtime::config_for_root(&config, &root).await?,
            };
            // Reload from the root's own hierarchy so the definition and its
            // `mise x` environment come from the project that owns it. The
            // generated pitchfork config for a root is rewritten wholesale, so
            // this has to stay that project's complete set: registering only the
            // daemon this project imported would delete its siblings from the
            // configuration it shares, orphaning any that were running.
            let set = scoped.daemons()?.for_root(&root);
            // What this invocation may act on, which for another project's root
            // is only what it imported or inherited.
            let requested = loaded.for_root(&root);
            let foreign = root != project_root;
            // What this invocation may act on or display. Registration still
            // uses the complete set above; only visibility narrows here.
            let visible = if foreign {
                set.restricted_to(&requested)
            } else {
                set.clone()
            };
            let previous = runtime::read_state(&root)?;
            if set.daemons.is_empty() && previous.ids.is_empty() {
                continue;
            }
            let (scoped, ts) = runtime::toolset(&scoped, install).await?;
            let runtime = Runtime::from_toolset(&scoped, &ts, Some(&previous.bin)).await;
            if action == "ls" {
                let desired = set
                    .namespace_for(&root)
                    .unwrap_or(previous.namespace.as_str());
                // Keep active daemons visible under their registered IDs until
                // they can be stopped. Otherwise show only the new namespace.
                let active = if !previous.namespace.is_empty() && desired != previous.namespace {
                    match &runtime {
                        Ok(runtime) => runtime.active(&root, &previous).await?,
                        Err(_) => false,
                    }
                } else {
                    false
                };
                let listed = if active { &previous.namespace } else { desired };
                let mut ids: Vec<String> = previous
                    .ids
                    .iter()
                    .filter(|id| {
                        id.rsplit_once('/')
                            .is_some_and(|(namespace, _)| namespace == listed)
                            && (!foreign
                                || visible.find(id.rsplit('/').next().unwrap_or(id)).is_some())
                    })
                    .cloned()
                    .collect();
                for name in visible.daemons.values().map(|d| &d.name) {
                    // An existing registration already represents this daemon.
                    // New declarations always use the configured namespace,
                    // even while other daemons still run under the old one.
                    if ids.iter().any(|id| id.rsplit('/').next() == Some(name)) {
                        continue;
                    }
                    let id = if desired.is_empty() {
                        name.clone()
                    } else {
                        format!("{desired}/{name}")
                    };
                    ids.push(id);
                }
                for id in ids {
                    let name = id.rsplit('/').next().unwrap_or(&id);
                    let daemon = visible.find(name);
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
                // Validate what this invocation will start, plus whatever those
                // daemons depend on, since pitchfork starts dependencies with
                // them. An unrelated daemon is registered but not started, so a
                // missing tool or task reference of its own must not fail this
                // command.
                let starting = set.restricted_to(&starting);
                runtime::validate_tools(&starting, &scoped, &ts).await?;
                starting.validate_tasks(&scoped).await?;
            }
            let (state, _project_lock) = if install {
                let (state, lock) = runtime.prepare(&root, &set, true, !foreign).await?;
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
                selected.retain(|id| set.find(id.rsplit('/').next().unwrap_or(id)).is_some());
            }
            if foreign {
                // Registering another project's daemons does not mean starting
                // or stopping them; only the ones this project asked for.
                selected.retain(|id| {
                    requested
                        .find(id.rsplit('/').next().unwrap_or(id))
                        .is_some()
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
                if install {
                    pending.push((runtime, root, forwarded, _project_lock));
                } else {
                    runtime.exec(&root, forwarded).await?;
                }
            }
        }
        // Register and validate every dependency root before pitchfork starts
        // anything, regardless of the order projects appear in the config.
        for (runtime, root, forwarded, _project_lock) in pending {
            runtime.exec(&root, forwarded).await?;
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
