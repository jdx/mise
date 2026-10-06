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
use crate::file::display_path;

mod broker;
mod config;
mod fnox;
mod grant;
mod name;
mod source;
mod spawn;

pub use broker::is_resolve_failure;
pub(crate) use broker::{Grantee, SecretBroker, SpawnRequest, TerminalAccess};
pub(crate) use grant::{
    DENIED_MARKER, EnvView, SecretGrant, aggregate_error, collision_problem, grant_for_task,
    sandbox_problem, static_problems,
};
pub use grant::{G7_TEXT, Problem, ProblemKind, SecretsDenied, TaskSecrets};
pub use name::SecretName;
pub use source::{Catalog, CatalogEntry, InjectMode, KeyKind};
pub(crate) use spawn::SpawnSecrets;

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
}

/// A task that lists secrets from this source.
pub struct InventoryTask {
    pub task: String,
    pub keys: Vec<String>,
    pub file: PathBuf,
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

/// Used by `mise secrets ls`. Gated (safe mode, trust). Spawns
/// `fnox ... env --json --describe` once.
pub async fn inventory(config: &Arc<Config>) -> eyre::Result<Inventory> {
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
    Ok(Inventory {
        source: Some(SourceInfo {
            kind: id.kind,
            root: id.root.clone(),
            declared_in: selected.declared_in,
            profile: id.profile.clone(),
            tool_path: source.tool_path().to_path_buf(),
        }),
        catalog: Some(Arc::new(catalog)),
        ignored: selection.ignored,
        tasks,
        problems,
    })
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
        problems.extend(grant::key_problems(task, &grant, catalog, label));
        tasks.push(InventoryTask {
            task: task.name.clone(),
            keys: grant.keys.keys().map(|k| k.to_string()).collect(),
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
    let declared = view.declared_keys(task);
    let mut problems = vec![];
    for key in grant.keys.keys() {
        if !sandbox.keeps_env_key(key.as_str()) {
            problems.push(sandbox_problem(&task.name, key.as_str()));
        }
        if declared
            .iter()
            .any(|d| mise_util::env::env_key_eq(d, key.as_str()))
        {
            problems.push(collision_problem(&task.name, key.as_str()));
        }
    }
    problems
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
    let view = EnvView::load(config).await;
    problems.extend(static_problems(task, &grant, None, &view));
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
            check
                .problems
                .extend(grant::key_problems(task, &grant, &catalog, &label));
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
            && plugins.values().any(|v| v.contains("mise-env-fnox"))
        {
            warnings.push(format!(
                "the mise-env-fnox plugin is deprecated (configured in {}); see https://mise.jdx.dev/environments/secrets/fnox.html#migrating",
                display_path(path)
            ));
        }
    }
    warnings
}
