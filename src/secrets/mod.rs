//! mise secrets: spawn-time env sources (`[secrets.*]`). Not `crate::system::secrets`
//! (`[bootstrap.secrets]`).
//!
//! A project names a secrets source in its own `mise.toml`. This module only knows how to
//! find that source and list the names it can provide; it never resolves a value.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::settings::SettingsExt;
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::sandbox::SandboxConfig;

mod broker;
mod config;
mod fnox;
mod grant;
mod name;
mod source;
mod spawn;
pub(crate) mod template;

pub use broker::is_resolve_failure;
pub(crate) use broker::{Grantee, Pending, SecretBroker, SpawnRequest, TerminalAccess};
pub use grant::{CliSecretGrant, G7_TEXT, Problem, ProblemKind, SecretsDenied, TaskSecrets};
pub(crate) use grant::{
    DENIED_MARKER, EnvView, SecretGrant, Subject, age_read_problems, aggregate_error,
    denied_from_env, effective_grant, grant_for_task, sandbox_and_collision_problems,
    static_problems,
};
pub use name::SecretName;
pub use source::{Catalog, CatalogEntry, InjectMode, KeyKind};
pub use spawn::SpawnSecrets;
pub(crate) use template::{LateSecretEnv, TomlShape, check_toml_locations, may_name_secrets};

use source::SecretSource;

/// A secret's value. `Debug` prints `[redacted]`; there is no `Display`, `Serialize` or
/// `Deserialize`, so a value reaches output only through an explicit `expose()`.
#[derive(Clone)]
pub(crate) struct SecretValue(Arc<str>);

impl SecretValue {
    pub(crate) fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Whether fnox may prompt: a person at a terminal, not CI, not a usage completion.
pub(crate) fn is_interactive() -> bool {
    console::user_attended_stderr()
        && std::io::stdin().is_terminal()
        && !ci_info::is_ci()
        && crate::env::__USAGE.is_none()
}

/// Where the project's secrets come from.
pub struct SourceInfo {
    pub kind: &'static str,
    pub root: PathBuf,
    pub declared_in: Vec<PathBuf>,
    pub profile: Option<String>,
    pub tool_path: PathBuf,
    /// The source's cache, when it has one (`daemon: running (protocol 6)`)
    pub daemon: Option<String>,
}

/// A task that asks for secrets from this source.
pub struct InventoryTask {
    pub task: String,
    pub uses: Vec<InventoryUse>,
    pub file: PathBuf,
}

/// One key a task asks for, and how.
pub struct InventoryUse {
    pub key: String,
    /// `secrets = [...]`, or `{{ secrets.KEY }}` in an env value
    pub via: UseVia,
}

pub enum UseVia {
    List,
    /// the env var whose value references the key
    Template {
        var: String,
    },
}

pub struct Inventory {
    pub source: Option<SourceInfo>,
    pub catalog: Option<Arc<Catalog>>,
    pub ignored: Vec<PathBuf>,
    /// Tasks whose grants resolve to this source.
    pub tasks: Vec<InventoryTask>,
    /// G1/G2 for those grants.
    pub problems: Vec<Problem>,
}

/// S1: no source for `root`.
pub fn no_source_message(root: &std::path::Path, ignored: &[PathBuf]) -> String {
    let mut msg = format!(
        "no secrets source is configured for {}; add [secrets.fnox] to the project's mise.toml (https://mise.jdx.dev/environments/secrets/fnox.html)",
        display_path(root)
    );
    for file in ignored {
        msg.push('\n');
        msg.push_str(&ignored_line(file));
    }
    msg
}

pub fn ignored_line(file: &std::path::Path) -> String {
    format!(
        "  [secrets.fnox] in {} is ignored: secrets sources are allowed only in project config (not global or system config, or files in or above your home directory).",
        display_path(file)
    )
}

/// The only place that picks a source implementation.
pub(crate) async fn open_source(
    config: &Arc<Config>,
    selected: &config::SelectedSource,
    ts: &crate::toolset::Toolset,
    config_env: Option<crate::task::task_context_builder::SourceConfigEnv>,
) -> eyre::Result<Arc<dyn SecretSource>> {
    Ok(Arc::new(
        fnox::FnoxSource::new(config, selected, ts, config_env).await?,
    ))
}

/// What `mise x` asks for.
pub struct ExecSecretsRequest<'a> {
    /// `--secrets`
    pub keys: &'a [String],
    /// `--secrets-all`
    pub all: bool,
    /// the env mise computed for the command, before secrets
    pub base_env: &'a EnvMap,
    pub sandbox: &'a SandboxConfig,
}

struct ExecTerminal;

#[async_trait::async_trait]
impl TerminalAccess for ExecTerminal {
    /// Nothing else runs concurrently, so there is no lock to take.
    async fn acquire(
        &self,
        pending: &Pending,
    ) -> Option<tokio::sync::RwLockWriteGuard<'static, ()>> {
        eprintln!("secrets: asking fnox for {}", pending.describe());
        None
    }
}

/// The values `mise x --secrets` and `--secrets-all` hand to the command. `Ok(None)` when
/// nothing is left to grant. Experimental, and refused in safe mode. `mise x` hands the
/// process over with `exec`, so these values are in mise's own environment until then.
pub async fn prepare_exec_secrets(
    config: &Arc<Config>,
    req: ExecSecretsRequest<'_>,
) -> eyre::Result<Option<SpawnSecrets>> {
    if req.keys.is_empty() && !req.all {
        return Ok(None);
    }
    Settings::get().ensure_experimental("mise secrets")?;
    Settings::ensure_not_safe("mise secrets")?;
    let broker = SecretBroker::default();
    let ctx = crate::task::task_context_builder::TaskContextBuilder::new();
    let no_env_keys = Default::default();
    // `base_env` is what mise computed for the command, so mise sets every key in it, even one
    // whose value equals the shell's.
    let mise_env_keys: std::collections::BTreeSet<String> = req.base_env.keys().cloned().collect();
    broker
        .prepare_exec(
            config,
            req.keys,
            req.all,
            SpawnRequest {
                grantee: Grantee::Exec,
                grant: &SecretGrant::default(),
                base_env: req.base_env,
                task_env_keys: &no_env_keys,
                mise_set_inherited: &no_env_keys,
                mise_env_keys: &mise_env_keys,
                // mise x has no task env, so no default was rendered
                rendered_defaults: &no_env_keys,
                sandbox: req.sandbox,
                file_dir: None,
                terminal: &ExecTerminal,
                ctx_builder: &ctx,
                denied: denied_from_env(),
                interactive: is_interactive(),
            },
        )
        .await
}

/// Used by `mise secrets ls`. Gated (safe mode, trust). Spawns
/// `fnox ... env --json --describe` once. `probe_daemon` also asks the source about its cache,
/// for the human header only.
pub async fn inventory(config: &Arc<Config>, probe_daemon: bool) -> eyre::Result<Inventory> {
    let selection = config::select_for_cwd(config)?;
    let Some(selected) = selection.source else {
        return Ok(Inventory {
            source: None,
            catalog: None,
            ignored: selection.ignored,
            tasks: vec![],
            problems: vec![],
        });
    };
    let source =
        fnox::FnoxSource::new(config, &selected, config.get_toolset().await?, None).await?;
    debug!("describing secrets from {}", source.label());
    let catalog = source.describe().await?;
    let id = source.id();
    let (tasks, problems) = inventory_tasks(config, id, &catalog, &source.label()).await;
    let daemon = cache_status(&source, &catalog, probe_daemon).await;
    Ok(Inventory {
        source: Some(SourceInfo {
            kind: id.kind,
            root: id.root.clone(),
            declared_in: selected.declared_in,
            profile: id.profile.clone(),
            tool_path: source.tool_path().to_path_buf(),
            daemon,
        }),
        catalog: Some(Arc::new(catalog)),
        ignored: selection.ignored,
        tasks,
        problems,
    })
}

/// The source's cache line, asked for only when the caller shows it.
async fn cache_status(
    source: &dyn SecretSource,
    catalog: &Catalog,
    probe_daemon: bool,
) -> Option<String> {
    if probe_daemon {
        source.daemon_status(catalog).await
    } else {
        None
    }
}

/// Tasks whose own selection lands on `id`, and what is wrong with their grants. A task whose
/// source cannot be selected is skipped: `mise run` reports that.
async fn inventory_tasks(
    config: &Arc<Config>,
    id: &source::SourceId,
    catalog: &Catalog,
    label: &str,
) -> (Vec<InventoryTask>, Vec<Problem>) {
    let mut tasks = vec![];
    let mut problems = vec![];
    let Ok(all) = config.tasks().await else {
        return (tasks, problems);
    };
    let ctx = crate::task::task_context_builder::TaskContextBuilder::new();
    for task in all.values() {
        let (grant, _) = grant_for_task(task);
        if grant.is_empty() {
            continue;
        }
        let Ok(selection) = config::select_for_task_ungated(config, &ctx, task).await else {
            continue;
        };
        let Some(selected) = selection.source else {
            continue;
        };
        let same = dunce::canonicalize(&selected.root).is_ok_and(|r| r == id.root)
            && selected.profile == id.profile;
        if !same {
            continue;
        }
        problems.extend(grant::key_problems(
            Subject::Task(&task.name),
            &grant,
            catalog,
            label,
        ));
        tasks.push(InventoryTask {
            task: task.name.clone(),
            uses: grant.inventory_uses(),
            file: task.config_source.clone(),
        });
    }
    (tasks, problems)
}

/// What `mise tasks validate` reports about one task's secrets.
pub struct TaskSecretsCheck {
    pub problems: Vec<Problem>,
    /// The catalog was not checked (safe mode, or a declaring file is not trusted).
    pub catalog_skipped: bool,
    /// `[secrets.fnox]` is configured but the fnox CLI is not found.
    pub fnox_missing: bool,
}

/// Static checks always; the catalog check only when the source may be used.
/// G13 and G11 as far as the task and the config decide them: its own sandbox settings and the
/// `[env]` keys it declares. `mise run` flags (`--deny-env`, `--allow-env`) and the run's
/// resolved environment can still change the result there.
fn task_level_problems(
    view: &EnvView,
    task: &crate::task::Task,
    grant: &SecretGrant,
) -> Vec<Problem> {
    let task_sandbox = crate::sandbox::SandboxConfig {
        deny_env: task.deny_all || task.deny_env,
        allow_env: task.allow_env.clone(),
        pass_through_env: task.pass_through_env.clone(),
        cache_env: task
            .cache
            .iter()
            .filter(|c| c.enabled)
            .flat_map(|c| c.env.clone())
            .collect(),
        ..Default::default()
    };
    let sandbox = crate::sandbox::SandboxConfig::from_settings_and_cli(
        &Settings::get().sandbox,
        false,
        task_sandbox,
    );
    sandbox_and_collision_problems(task, grant, &sandbox, &view.declared_keys(task))
}

/// Shared by every `check_task_secrets` call of one `mise tasks validate`, so a source is
/// opened and described once however many tasks list keys from it.
#[derive(Default)]
pub struct TaskSecretsCache {
    broker: SecretBroker,
    ctx: crate::task::task_context_builder::TaskContextBuilder,
}

pub async fn check_task_secrets(
    config: &Arc<Config>,
    task: &crate::task::Task,
    cache: &TaskSecretsCache,
) -> TaskSecretsCheck {
    let (grant, mut problems) = grant_for_task(task);
    let view = if grant.is_empty() {
        EnvView::default()
    } else {
        EnvView::load(config)
            .await
            .for_task(config, &cache.ctx, task, true)
            .await
    };
    problems.extend(static_problems(task, &grant, None, &view));
    problems.extend(grant::age_read_problems(task, &grant).await);
    let mut check = TaskSecretsCheck {
        problems,
        catalog_skipped: false,
        fnox_missing: false,
    };
    if grant.is_empty() {
        return check;
    }
    check
        .problems
        .extend(task_level_problems(&view, task, &grant));
    let ctx = &cache.ctx;
    let selection = match config::select_for_task_ungated(config, ctx, task).await {
        Ok(selection) => selection,
        Err(err) => {
            check.problems.push(Problem::new(
                &task.name,
                None,
                ProblemKind::Source,
                format!("task {}: {err:#}", task.name),
            ));
            return check;
        }
    };
    let Some(selected) = selection.source else {
        check.problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::Source,
            format!(
                "task {}: {}",
                task.name,
                no_source_message(
                    &task.config_root.clone().unwrap_or_default(),
                    &selection.ignored
                )
            ),
        ));
        return check;
    };
    let usable = !Settings::safe_mode()
        && selected
            .declared_in
            .iter()
            .all(|f| crate::config::config_file::is_path_trusted(f));
    if !usable {
        check.catalog_skipped = true;
        return check;
    }
    match cache.broker.catalog_for(config, ctx, task, &selected).await {
        // found through the task's own toolset, as `mise run` finds it
        Err(err) if fnox::is_not_found_message(&format!("{err:#}")) => {
            check.fnox_missing = true;
        }
        Ok((catalog, label)) => {
            check.problems.extend(grant::key_problems(
                Subject::Task(&task.name),
                &grant,
                &catalog,
                &label,
            ));
            check.problems.extend(broker::late_file_problems(
                Subject::Task(&task.name),
                &grant,
                &catalog,
            ));
        }
        Err(err) => check.problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::Source,
            format!("task {}: {err:#}", task.name),
        )),
    }
    check
}

/// The deprecated plugin, by name (`mise-env-fnox = "./plugins/secrets"`) or by the repo name
/// ending its source (`https://github.com/x/mise-env-fnox.git`). A source that merely contains
/// the text, such as `mise-env-fnox-fork-tools`, is some other plugin.
fn is_env_fnox_plugin(name: &str, source: &str) -> bool {
    const PLUGIN: &str = "mise-env-fnox";
    // A pinned source (`...#main`, `...?ref=x`) still names the same repo.
    let source = source.split(['#', '?']).next().unwrap_or_default();
    let last = source
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or_default();
    name == PLUGIN || last.strip_suffix(".git").unwrap_or(last) == PLUGIN
}

/// Used by `mise doctor`. Never spawns fnox, never errors.
pub async fn doctor_warnings(config: &Arc<Config>) -> Vec<String> {
    let mut warnings = vec![];
    match config::select_for_cwd_ungated(config) {
        Ok(selection) => {
            for file in &selection.ignored {
                warnings.push(format!(
                    "[secrets.fnox] in {} is ignored: secrets sources are allowed only in project config",
                    display_path(file)
                ));
            }
            if let Some(selected) = &selection.source
                && fnox::find_binary(config).await.is_none()
            {
                warnings.push(format!(
                    "[secrets.fnox] is configured in {} but the fnox CLI was not found; add it with: mise use fnox",
                    display_path(&selected.declared_in[0])
                ));
            }
        }
        Err(err) => warnings.push(format!("{err:#}")),
    }
    for (path, cf) in config.config_files.iter() {
        if let Ok(plugins) = cf.plugins()
            && plugins
                .iter()
                .any(|(name, src)| is_env_fnox_plugin(name, src))
        {
            warnings.push(format!(
                "the mise-env-fnox plugin is deprecated (configured in {}); see https://mise.jdx.dev/environments/secrets/fnox.html#migrating",
                display_path(path)
            ));
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::is_env_fnox_plugin;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct Counting {
        id: source::SourceId,
        probes: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SecretSource for Counting {
        fn id(&self) -> &source::SourceId {
            &self.id
        }
        fn label(&self) -> String {
            "counting".into()
        }
        async fn describe(&self) -> eyre::Result<Catalog> {
            unreachable!()
        }
        async fn resolve(
            &self,
            _cx: &source::SourceCx,
            _keys: &source::KeySelection,
            _catalog: &Catalog,
        ) -> Result<source::Resolved, source::ResolveError> {
            unreachable!()
        }
        async fn daemon_status(&self, _catalog: &Catalog) -> Option<String> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            Some("daemon: running".into())
        }
        fn build_fingerprint(&self) -> String {
            String::new()
        }
    }

    #[tokio::test]
    async fn the_cache_is_probed_only_when_the_caller_shows_it() {
        let source = Counting {
            id: source::SourceId {
                kind: "fake",
                root: PathBuf::from("/p"),
                profile: None,
            },
            probes: AtomicUsize::new(0),
        };
        let catalog = Catalog {
            entries: Default::default(),
            profile: vec![],
            dynamic_leases: vec![],
            tool_version: "1".into(),
            cache: Some(true),
        };
        assert_eq!(cache_status(&source, &catalog, false).await, None);
        assert_eq!(source.probes.load(Ordering::SeqCst), 0);
        assert!(cache_status(&source, &catalog, true).await.is_some());
        assert_eq!(source.probes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn env_fnox_plugin_matches_by_name_or_repo() {
        assert!(is_env_fnox_plugin("mise-env-fnox", "./plugins/secrets"));
        assert!(is_env_fnox_plugin(
            "fnox-env",
            "https://github.com/jdx/mise-env-fnox"
        ));
        assert!(is_env_fnox_plugin(
            "fnox-env",
            "https://github.com/jdx/mise-env-fnox.git"
        ));
        assert!(is_env_fnox_plugin("x", "git@github.com:jdx/mise-env-fnox/"));
        assert!(is_env_fnox_plugin(
            "fnox-env",
            "https://github.com/jdx/mise-env-fnox.git#main"
        ));
        assert!(is_env_fnox_plugin(
            "fnox-env",
            "https://github.com/jdx/mise-env-fnox?ref=v1"
        ));
    }

    #[test]
    fn env_fnox_plugin_ignores_lookalikes() {
        assert!(!is_env_fnox_plugin(
            "other",
            "https://github.com/a/mise-env-fnox-tools"
        ));
        assert!(!is_env_fnox_plugin("other", "./mise-env-fnox/other-plugin"));
        assert!(!is_env_fnox_plugin(
            "other",
            "https://github.com/a/not-mise-env-fnox"
        ));
    }
}
