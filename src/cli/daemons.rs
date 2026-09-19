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
    Urls(UrlsArgs),
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

/// Show each project daemon's port and its proxy hostname URL.
///
/// Hostnames do not move between git worktrees, so an HTTP service can be
/// addressed by URL while concurrent checkouts keep separate ports. A daemon
/// with no port, or with proxy = false, is listed with its port alone.
#[derive(Debug, Default, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct UrlsArgs {
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
            Some(Commands::Urls(list)) => ("urls", vec![], list.json),
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
        let (requested_names, groups, flags) = split_args(action, &args)?;
        // An import that could not be resolved is fatal only when this command
        // names it. Someone whose sibling checkout is missing can still list and
        // stop their own daemons; they are told what is unavailable and why.
        // Fatal only when this command names it, and only when the name means
        // that failure to this project: a group or a working import declared
        // nearer answers for the word instead. A qualified request is literal,
        // and an unresolved import never got an ID to be qualified with.
        if let Some((name, err)) = requested_names.iter().find_map(|requested| {
            match loaded.resolve_bare(&project_root, requested) {
                Some(daemons::BareName::Unresolved(err)) if !requested.contains('/') => {
                    Some((requested, err))
                }
                _ => None,
            }
        }) {
            bail!("cannot resolve [daemons.{name}]: {err}");
        }
        for ((_, name), err) in &loaded.import_errors {
            warn!("[daemons.{name}] is unavailable: {err}");
        }
        let install = matches!(action, "start" | "restart");
        let proxy = daemons::urls::proxy_settings();
        let selectors: Vec<Selector> = requested_names
            .iter()
            .map(|name| {
                let resolved = loaded.resolve_alias(name);
                if name.contains('/') {
                    return Ok(Selector::Name(resolved));
                }
                // Ask this project what the word means, nearest declaration
                // first. An import becomes the ID it answers to; a group stays
                // bare so `selects` expands it against the project that
                // declares it, which is also how a group in an unrelated
                // project keeps its own meaning.
                match loaded.resolve_bare(&project_root, name) {
                    Some(daemons::BareName::Import(id)) => {
                        return Ok(Selector::Name(id.to_string()));
                    }
                    // A group stays unqualified: pinning it to one namespace
                    // would stop each project resolving it against its own
                    // [daemon_groups].
                    Some(daemons::BareName::Group) => return Ok(Selector::Group(name.clone())),
                    // An unresolved import has no ID; the check above already
                    // refused it, so this only keeps the name intact.
                    Some(daemons::BareName::Unresolved(_)) => {
                        return Ok(Selector::Name(name.clone()));
                    }
                    None => {}
                }
                // A group no project in this tree declares can still belong to
                // one of the other loaded roots, which resolves it itself.
                if loaded.groups.iter().any(|group| group.name == *name) {
                    return Ok(Selector::Group(name.clone()));
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
                Ok(Selector::Name(format!("{namespace}/{daemon_name}")))
            })
            .chain(groups.iter().cloned().map(|g| Ok(Selector::Group(g))))
            .collect::<Result<_>>()?;
        let mut root_ids = Vec::new();
        let mut root_sets = Vec::new();
        for root in &roots {
            let previous = runtime::read_state(root)?;
            let namespace = match loaded.namespace_for(root) {
                Some(namespace) => namespace.to_string(),
                None if previous.namespace.is_empty() => runtime::namespace(root)?,
                None => previous.namespace.clone(),
            };
            let set = loaded.for_root(root);
            let mut ids = if install { Vec::new() } else { previous.ids };
            // An imported daemon is keyed by qualified ID, so take the name
            // from the daemon rather than the map key.
            ids.extend(
                set.daemons
                    .values()
                    .map(|daemon| format!("{namespace}/{}", daemon.name)),
            );
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
                if project_root.starts_with(&root) {
                    scoped.seed_daemons(loaded.for_root(&root));
                }
                let declarations = scoped.daemons()?;
                for dependency_root in declarations.roots() {
                    if !roots.contains(&dependency_root) {
                        // Every root travels with its ids and its set; the three
                        // are zipped below and a short list drops the tail.
                        root_sets.push(declarations.for_root(&dependency_root));
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
            // Expanded the same way selection expands them, so starting a
            // group gathers the daemons it names rather than nothing.
            let requested = root_ids
                .iter()
                .zip(&root_sets)
                .flat_map(|(ids, set)| {
                    let root_selectors = effective_selectors(&selectors, set, action);
                    ids.iter()
                        .filter(move |id| {
                            root_selectors.is_empty()
                                || root_selectors.iter().any(|s| selects(set, id, s))
                        })
                        .cloned()
                })
                .collect::<Vec<_>>();
            let starting = candidates.with_dependencies(&requested);
            // Dropping a dependency on an unresolved import keeps the generated
            // config valid, but starting the daemon anyway would run it without
            // something it declared it needs. Say which import is missing.
            daemons::ensure_not_blocked(loaded, &starting, None)?;
            starting
        } else {
            daemons::DaemonSet::default()
        };
        let mut pending = Vec::new();
        let mut rows = Vec::new();
        // The hostname components each listed root contributes, for the stack
        // and project pages `mise daemons urls` prints alongside the daemons.
        let mut listed_roots: Vec<(PathBuf, Option<daemons::urls::RootLabels>)> = Vec::new();
        let mut matched = false;
        let mut root_entries: Vec<_> = roots
            .into_iter()
            .zip(root_ids)
            .zip(root_sets)
            .map(|((root, ids), root_set)| (root, ids, root_set))
            .collect();
        if install {
            // Startup holds project locks until execution. Acquire them in a
            // consistent order even when callers import the projects differently.
            root_entries.sort_by(|a, b| a.0.cmp(&b.0));
        }
        for (root, ids, root_set) in root_entries {
            // What this root contributes to the run: for a start that is the
            // dependency closure, which already accounts for daemons in other
            // projects that nothing named directly.
            let in_closure = install && !starting.for_root(&root).daemons.is_empty();
            if install && !in_closure && (!requested_names.is_empty() || root != project_root) {
                continue;
            }
            // Each root resolves the request against its own groups, so a `default`
            // group in one project never suppresses another project's daemons. This
            // is the one place selectors are resolved, from the set the request was
            // validated against rather than the per-root reload used for tools and
            // the generated configuration.
            let root_selectors = effective_selectors(&selectors, &root_set, action);
            if !in_closure
                && !root_selectors.is_empty()
                && !ids
                    .iter()
                    .any(|id| root_selectors.iter().any(|s| selects(&root_set, id, s)))
            {
                continue;
            }
            // The per-root configuration supplies this project's tools and env.
            let scoped = match owner_configs.remove(&root) {
                Some(scoped) => scoped,
                None => runtime::config_for_root(&config, &root).await?,
            };
            // What this invocation may act on, which for another project's root
            // is only what it imported.
            let requested = loaded.for_root(&root);
            // Another project's root, reached only because a daemon was imported
            // from it. An ancestor of this project is not that: its daemons are
            // declared in this project's own hierarchy, and the merged view is
            // what decides which of them a nearer config has taken over.
            let foreign = !requested.daemons.is_empty()
                && requested.daemons.values().all(|daemon| daemon.imported);
            // Which daemons a root owns comes from the merged view, the same way
            // the auto lifecycle and task-required daemons resolve them, so a
            // name a nearer project redefines is registered and started once.
            //
            // Another project's root is the exception. This project knows only
            // the daemon it imported, and the generated configuration is
            // rewritten whole, so registering that alone would delete the
            // siblings sharing that file. Its own hierarchy is the complete set.
            let set = if foreign {
                scoped.daemons()?.for_root(&root)
            } else {
                // Seeded before the toolset is built, so this project installs
                // tools for the daemons it registers and not for a name a nearer
                // project took over.
                scoped.seed_daemons(root_set.clone());
                root_set.clone()
            };
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
            if matches!(action, "ls" | "urls") {
                // Every listed root, labels or not. A root kept only by its
                // recorded ids declares nothing now and so contributes no
                // labels, and skipping it here would drop its daemons from the
                // listing entirely rather than showing them without URLs.
                listed_roots.push((root.clone(), set.labels.get(&root).cloned()));
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
                    // Fall back to the last recorded allocation for a daemon
                    // that is no longer declared but may still be running.
                    let claim = daemon
                        .and_then(|d| d.port)
                        .or_else(|| previous.ports.get(name).copied());
                    let status = if let Ok(runtime) = &runtime
                        && !previous.namespace.is_empty()
                    {
                        runtime.status(&root, &id).await.ok()
                    } else {
                        None
                    };
                    let host = daemon.and_then(|d| d.host.as_deref());
                    rows.push(serde_json::json!({ "id": id, "name": name, "root": root, "source": daemon.map(|d| &d.source), "preset": daemon.and_then(|d| d.preset.as_ref()), "status": status.as_ref().and_then(|s| s["status"].as_str()).unwrap_or("available"), "pid": status.as_ref().and_then(|s| s["pid"].as_u64()), "port": claim.map(|c| c.port), "port_auto": claim.map(|c| c.is_auto()), "host": host, "url": host.map(|h| proxy.url(h)), "proxy": daemon.map(proxy_mode) }));
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
                // This root's own configuration, which the check above cannot
                // see: a referenced project declares its own imports.
                daemons::ensure_not_blocked(&set, &starting, Some(&root))?;
            }
            let launching = starting_names(&set, &ids, &root_selectors);
            let (state, _project_lock) = if install {
                let (state, lock) = runtime
                    .prepare(&root, &set, true, !foreign, &launching)
                    .await?;
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
                    if install {
                        // The closure already answered this, including
                        // dependencies in projects nothing named directly.
                        set.find(id.rsplit('/').next().unwrap_or(id))
                            .is_some_and(|daemon| starting.contains(daemon))
                    } else {
                        root_selectors.is_empty()
                            || root_selectors.iter().any(|s| selects(&set, id, s))
                    }
                })
                .cloned()
                .collect();
            if foreign && !install {
                // Registering another project's daemons does not mean stopping
                // them; only the ones this project asked for.
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
        if matches!(action, "ls" | "urls") {
            if json {
                miseprintln!("{}", serde_json::to_string_pretty(&rows)?);
            } else if action == "ls" {
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
            } else {
                print_urls(&rows, &listed_roots, proxy)?;
            }
        } else if !matched {
            bail!("no matching project daemons; define [daemons] in mise.toml");
        }
        Ok(())
    }
}

/// Print every daemon's stable hostname next to the port it actually binds,
/// grouped by project root, followed by the pages pitchfork serves for the
/// whole stack. A daemon with `proxy = false` is listed with its port alone, so
/// a database is visible here rather than looking absent.
fn print_urls(
    rows: &[serde_json::Value],
    roots: &[(PathBuf, Option<daemons::urls::RootLabels>)],
    proxy: &daemons::urls::ProxySettings,
) -> Result<()> {
    for (root, labels) in roots {
        let display = crate::file::display_path(root);
        miseprintln!("{display}");
        let mut table =
            crate::ui::table::MiseTable::new(false, &["Daemon", "URL", "Port", "Proxy", "Status"]);
        for row in rows
            .iter()
            .filter(|row| row["root"].as_str().map(std::path::Path::new) == Some(root.as_path()))
        {
            table.add_row(vec![
                comfy_table::Cell::new(row["id"].as_str().unwrap_or_default()),
                comfy_table::Cell::new(row["url"].as_str().unwrap_or("-")),
                comfy_table::Cell::new(
                    row["port"]
                        .as_u64()
                        .map(|port| port.to_string())
                        .unwrap_or_else(|| "-".into()),
                ),
                comfy_table::Cell::new(row["proxy"].as_str().unwrap_or("off")),
                comfy_table::Cell::new(row["status"].as_str().unwrap_or_default()),
            ]);
        }
        table.print()?;
        // A root that declares nothing now has no labels and so no pages. The
        // primary checkout has no stack page of its own either; its stack is
        // the project, so only a worktree prints both.
        let Some(labels) = labels else {
            continue;
        };
        if let Some(stack) = proxy.stack_url(labels) {
            miseprintln!("  stack:   {stack}");
        }
        if let Some(project) = proxy.project_url(labels) {
            miseprintln!("  project: {project}");
        }
    }
    Ok(())
}

/// How the proxy treats a daemon: `off` when it opted out, otherwise what it
/// does with TLS. Pitchfork terminates TLS unless the daemon asked it not to,
/// so an unset `proxy_tls` reports that default rather than nothing.
fn proxy_mode(daemon: &daemons::Daemon) -> &str {
    if daemon.host.is_none() {
        return "off";
    }
    daemon
        .table
        .get("proxy_tls")
        .and_then(toml::Value::as_str)
        .unwrap_or("terminate")
}

/// Daemon names this invocation will launch, so only their ports are conflict
/// checked. Selectors are matched against qualified ids, exactly as the daemon
/// selection does: a bare name never matches a selector written as
/// `<namespace>/<name>`, which would drop that daemon from the check while it
/// still started.
fn starting_names(set: &daemons::DaemonSet, ids: &[String], selectors: &[Selector]) -> Vec<String> {
    let named: Vec<String> = ids
        .iter()
        .filter(|id| selectors.is_empty() || selectors.iter().any(|s| selects(set, id, s)))
        .filter_map(|id| {
            let name = id.rsplit('/').next().unwrap_or(id);
            set.daemons.contains_key(name).then(|| name.to_string())
        })
        .collect();
    // Pitchfork starts a daemon's dependencies with it, so their ports are
    // about to be bound too and belong in the check. Naming only what the
    // selectors matched would let a dependency collide with another project
    // and say nothing.
    set.with_dependencies(&named)
        .daemons
        .values()
        .map(|daemon| daemon.name.clone())
        .collect()
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
    /// The word the user typed, for a message that has to name it back.
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
    fn qualified_selectors_still_reach_the_port_check() {
        let set = daemons::load(&files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'server'\n[daemons.web]\nrun = 'web'\n",
        )]))
        .unwrap();
        let ids = ["ns/api".to_string(), "ns/web".to_string()];

        // Selecting by qualified id must still reach the conflict check. A bare
        // name never matches such a selector, so filtering on names would have
        // returned nothing here while the daemon was still started.
        let qualified = [Selector::Name("ns/api".to_string())];
        assert_eq!(starting_names(&set, &ids, &qualified), ["api"]);

        // The short spelling selects the same daemon, and only that one.
        let bare = [Selector::Name("api".to_string())];
        assert_eq!(starting_names(&set, &ids, &bare), ["api"]);

        // No selectors means everything this root would launch.
        assert_eq!(starting_names(&set, &ids, &[]), ["api", "web"]);

        // An id with no matching daemon contributes nothing.
        let stale = ["ns/gone".to_string()];
        assert!(starting_names(&set, &stale, &[]).is_empty());

        // Pitchfork starts a daemon's dependencies with it, so their ports are
        // bound by this same command and have to reach the conflict check.
        // Naming only what the selector matched would let one collide with
        // another project in silence.
        let set = daemons::load(&files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'server'\nport = 3000\ndepends = ['db']\n\
             [daemons.db]\nrun = 'db'\nport = 5432\n",
        )]))
        .unwrap();
        let ids = ["ns/api".to_string(), "ns/db".to_string()];
        let mut launching = starting_names(&set, &ids, &[Selector::Name("ns/api".to_string())]);
        launching.sort();
        assert_eq!(launching, ["api", "db"]);
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
