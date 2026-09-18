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
        let (names, groups, flags) = split_args(action, &args)?;
        let selectors: Vec<Selector> = names
            .into_iter()
            .map(Selector::Name)
            .chain(groups.iter().cloned().map(Selector::Group))
            .collect();
        let install = matches!(action, "start" | "restart");
        let mut root_ids = Vec::new();
        let mut root_sets = Vec::new();
        for root in &roots {
            let previous = runtime::read_state(root)?;
            let namespace = if previous.namespace.is_empty() {
                runtime::namespace(root)?
            } else {
                previous.namespace.clone()
            };
            let set = loaded.for_root(root);
            let mut ids = if install { Vec::new() } else { previous.ids };
            ids.extend(set.daemons.keys().map(|name| format!("{namespace}/{name}")));
            root_ids.push(ids);
            root_sets.push(set);
        }
        // A pitchfork group can name daemons outside the project, so only groups
        // declared in [daemon_groups] are accepted here.
        for group in &groups {
            if !root_sets.iter().any(|set| set.group(group).is_some()) {
                // A group is an alias in the configuration, not persisted state, so a
                // removed one cannot be expanded. Daemons it started are still tracked
                // by name, which is the way back to them.
                let hint = if install {
                    "declare the group in [daemon_groups] or use pitchfork directly for its own groups"
                } else {
                    "declare the group in [daemon_groups], or run `mise daemons ls` to name daemons a removed group started"
                };
                bail!("no [daemon_groups] entry named {group:?}; {hint}");
            }
        }
        // Validate the entire request before any root installs tools or changes state.
        for selector in &selectors {
            if !root_ids
                .iter()
                .zip(&root_sets)
                .any(|(ids, set)| ids.iter().any(|id| selects(set, id, selector)))
            {
                bail!("no matching project daemons for {:?}", selector.name());
            }
        }
        let mut rows = Vec::new();
        let mut matched = false;
        for ((root, ids), root_set) in roots.into_iter().zip(root_ids).zip(root_sets) {
            // Each root resolves the request against its own groups, so a `default`
            // group in one project never suppresses another project's daemons. This
            // is the one place selectors are resolved, from the set the request was
            // validated against rather than the per-root reload used for tools and
            // the generated configuration.
            let root_selectors = effective_selectors(&selectors, &root_set, action);
            if !root_selectors.is_empty()
                && !ids
                    .iter()
                    .any(|id| root_selectors.iter().any(|s| selects(&root_set, id, s)))
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
                set.validate_tasks(&scoped).await?;
            }
            let (state, _project_lock) = if install {
                let (state, lock) = runtime.prepare(&root, &set, true).await?;
                (state, Some(lock))
            } else {
                (previous, None)
            };
            // `root_selectors` came from the same set the request was validated
            // against, so selection cannot disagree with that validation.
            let mut selected: Vec<_> = state
                .ids
                .iter()
                .filter(|id| {
                    root_selectors.is_empty()
                        || root_selectors.iter().any(|s| selects(&root_set, id, s))
                })
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

fn matches_name(id: &str, name: &str) -> bool {
    id == name || id.rsplit('/').next() == Some(name)
}

/// What the user asked for. A positional argument may name a daemon or a group and
/// is resolved per project root; `--group` only ever names a group, so it can never
/// fall back to a same-named daemon in an unrelated project.
#[derive(Debug, Clone, PartialEq)]
enum Selector {
    Name(String),
    Group(String),
}

impl Selector {
    fn name(&self) -> &str {
        match self {
            Selector::Name(name) | Selector::Group(name) => name,
        }
    }
}

/// Whether `id`, a daemon of `set`'s project, is selected. Groups are looked up in
/// that same project, so membership never crosses a project boundary.
fn selects(set: &daemons::DaemonSet, id: &str, selector: &Selector) -> bool {
    match selector {
        Selector::Group(name) => set
            .expand(name)
            .is_some_and(|members| members.iter().any(|member| matches_name(id, member))),
        Selector::Name(name) => match set.expand(name) {
            Some(members) => members.iter().any(|member| matches_name(id, member)),
            None => matches_name(id, name),
        },
    }
}

/// The selectors to apply to one project. A bare `start` or `restart` uses that
/// project's own `default` group; a project without one still covers all of its
/// daemons. `restart` is included because it starts daemons, and would otherwise
/// start the ones a `default` group deliberately leaves out.
fn effective_selectors(
    selectors: &[Selector],
    set: &daemons::DaemonSet,
    action: &str,
) -> Vec<Selector> {
    if selectors.is_empty()
        && matches!(action, "start" | "restart")
        && set.group("default").is_some()
    {
        return vec![Selector::Group("default".into())];
    }
    selectors.to_vec()
}

/// Separate positional IDs, `--group` values, and pitchfork options before matching
/// project roots. Value-taking options must retain their values even when a value is
/// a daemon name.
fn split_args(action: &str, args: &[String]) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let mut names = Vec::new();
    let mut groups = Vec::new();
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
        if arg == "--group" || arg.starts_with("--group=") {
            let value = match arg.strip_prefix("--group=") {
                Some(value) => value.to_string(),
                None => args
                    .next()
                    .ok_or_else(|| eyre::eyre!("--group requires a value"))?
                    .clone(),
            };
            if value.is_empty() {
                bail!("--group requires a value");
            }
            groups.push(value);
            continue;
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
    Ok((names, groups, flags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_file::ConfigFile;
    use crate::config::config_file::mise_toml::MiseToml;
    use std::sync::Arc;

    fn files(entries: &[(&str, &str)]) -> crate::config::ConfigMap {
        entries
            .iter()
            .map(|(path, body)| {
                let path = PathBuf::from(path);
                let cf: Arc<dyn ConfigFile> = Arc::new(MiseToml::from_str(body, &path).unwrap());
                (path, cf)
            })
            .collect()
    }

    #[test]
    fn names_are_independent_of_flag_order_and_values() {
        for args in [vec!["--force", "missing"], vec!["missing", "--force"]] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            let (names, groups, flags) = split_args("start", &args).unwrap();
            assert_eq!(names, ["missing"]);
            assert!(groups.is_empty());
            assert_eq!(flags, ["--force"]);
        }
        let args = ["--grep", "api", "--since=5m", "web", "-n", "20"].map(String::from);
        let (names, _, flags) = split_args("logs", &args).unwrap();
        assert_eq!(names, ["web"]);
        assert_eq!(flags, ["--grep", "api", "--since=5m", "-n", "20"]);
        let (names, _, flags) =
            split_args("logs", &["-fn".into(), "20".into(), "web".into()]).unwrap();
        assert_eq!(names, ["web"]);
        assert_eq!(flags, ["-fn", "20"]);
        assert!(split_args("logs", &["--grep".into()]).is_err());
        let (names, _, _) = split_args("start", &["--".into(), "missing".into()]).unwrap();
        assert_eq!(names, ["missing"]);
    }

    #[test]
    fn group_flags_are_collected_without_reaching_pitchfork() {
        for action in ["start", "stop", "restart", "status", "logs"] {
            let args = ["api", "--group", "web", "--group=db"].map(String::from);
            let (names, groups, flags) = split_args(action, &args).unwrap();
            assert_eq!(names, ["api"]);
            assert_eq!(groups, ["web", "db"]);
            assert!(flags.is_empty());
        }
        assert!(split_args("start", &["--group".into()]).is_err());
        assert!(split_args("start", &["--group=".into()]).is_err());
    }

    #[test]
    fn group_names_select_every_member() {
        let set = daemons::load(&files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'api'\n[daemons.worker]\nrun = 'worker'\n[daemons.web]\nrun = 'web'\n[daemon_groups]\nbackend = ['api', 'worker']\n",
        )]))
        .unwrap();
        let group = Selector::Group("backend".into());
        assert!(selects(&set, "proj/api", &group));
        assert!(selects(&set, "proj/worker", &group));
        assert!(!selects(&set, "proj/web", &group));
        // A positional argument resolves to the group of that name.
        assert!(selects(&set, "proj/api", &Selector::Name("backend".into())));
        // Daemon names keep working alongside groups.
        assert!(selects(&set, "proj/web", &Selector::Name("web".into())));
        assert!(selects(
            &set,
            "proj/web",
            &Selector::Name("proj/web".into())
        ));
        assert!(!selects(
            &set,
            "proj/web",
            &Selector::Name("missing".into())
        ));
    }

    #[test]
    fn a_group_selector_never_matches_a_same_named_daemon() {
        // One project declares the group; an unrelated project declares a daemon
        // that happens to share its name.
        let loaded = daemons::load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nops = ['api']\n",
            ),
            ("/parent/mise.toml", "[daemons.ops]\nrun = 'ops'\n"),
        ]))
        .unwrap();
        // Roots are absolutized, so derive them instead of hardcoding a path.
        let other = loaded.for_root(&loaded.daemons["ops"].root.clone());
        assert!(!selects(
            &other,
            "parent/ops",
            &Selector::Group("ops".into())
        ));
        // The same word given positionally still selects that project's daemon.
        assert!(selects(&other, "parent/ops", &Selector::Name("ops".into())));
        let owner = loaded.for_root(&loaded.daemons["api"].root.clone());
        assert!(selects(&owner, "child/api", &Selector::Group("ops".into())));
    }

    #[test]
    fn a_positional_name_resolves_in_each_project_separately() {
        // `web` is a group in the child and a daemon in the parent.
        let loaded = daemons::load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemons.worker]\nrun = 'worker'\n[daemon_groups]\nweb = ['api']\n",
            ),
            ("/parent/mise.toml", "[daemons.web]\nrun = 'web'\n"),
        ]))
        .unwrap();
        let child = loaded.for_root(&loaded.daemons["api"].root.clone());
        let parent = loaded.for_root(&loaded.daemons["web"].root.clone());
        let positional = Selector::Name("web".into());
        // In the child it is the group, so it reaches the member and not the rest.
        assert!(selects(&child, "child/api", &positional));
        assert!(!selects(&child, "child/worker", &positional));
        // In the parent the same word is the daemon of that name.
        assert!(selects(&parent, "parent/web", &positional));
        // --group stays a group everywhere, so it never reaches the parent daemon.
        let group = Selector::Group("web".into());
        assert!(selects(&child, "child/api", &group));
        assert!(!selects(&parent, "parent/web", &group));
    }

    #[test]
    fn a_default_group_applies_only_to_the_project_declaring_it() {
        let loaded = daemons::load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.web]\nrun = 'web'\n[daemons.extra]\nrun = 'extra'\n[daemon_groups]\ndefault = ['web']\n",
            ),
            ("/parent/mise.toml", "[daemons.inherited]\nrun = 'x'\n"),
        ]))
        .unwrap();
        let child = loaded.for_root(&loaded.daemons["web"].root.clone());
        let parent = loaded.for_root(&loaded.daemons["inherited"].root.clone());
        assert_eq!(
            effective_selectors(&[], &child, "start"),
            [Selector::Group("default".into())]
        );
        // The parent declares no default, so a bare start keeps every daemon.
        assert!(effective_selectors(&[], &parent, "start").is_empty());
        // restart starts daemons, so it uses the group too; stop does not.
        assert_eq!(
            effective_selectors(&[], &child, "restart"),
            [Selector::Group("default".into())]
        );
        assert!(effective_selectors(&[], &child, "stop").is_empty());
        assert!(effective_selectors(&[], &child, "logs").is_empty());
        // An explicit request is never replaced by the default group.
        assert_eq!(
            effective_selectors(&[Selector::Name("extra".into())], &child, "start"),
            [Selector::Name("extra".into())]
        );
    }
}
