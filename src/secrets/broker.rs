//! Resolves granted keys at spawn time. Values live only in `SourceMemo`, in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use eyre::{Result, bail, eyre};
use tokio::sync::{Mutex, OnceCell};

use super::grant::{
    Problem, ProblemKind, SecretGrant, SecretsDenied, collision_problem, grant_for_task,
    key_problems, sandbox_problem, static_problems,
};
use super::source::{
    Catalog, KeySelection, ResolveError, Resolved, SecretSource, SourceCx, SourceId,
};
use super::spawn::{SpawnSecrets, TempSecretFiles};
use super::{SecretName, SecretValue};
use crate::config::settings::SettingsExt;
use crate::config::{Config, Settings};
use crate::env;
use crate::env_diff::EnvMap;
use crate::sandbox::SandboxConfig;
use crate::task::Task;
use crate::task::task_context_builder::TaskContextBuilder;
use crate::toolset::Toolset;
use crate::ui::multi_progress_report::MultiProgressReport;
use mise_util::env::env_key_eq;

pub(crate) enum Grantee<'a> {
    Task(&'a Task),
}

impl Grantee<'_> {
    fn task(&self) -> &Task {
        let Self::Task(task) = self;
        task
    }

    fn label(&self) -> &str {
        &self.task().name
    }
}

/// The terminal lock for an interactive fnox call.
#[async_trait::async_trait]
pub(crate) trait TerminalAccess: Send + Sync {
    async fn acquire(
        &self,
        keys: &[SecretName],
    ) -> Option<tokio::sync::RwLockWriteGuard<'static, ()>>;
}

pub(crate) struct SpawnRequest<'a> {
    pub(crate) grantee: Grantee<'a>,
    pub(crate) grant: &'a SecretGrant,
    /// child env before secrets
    pub(crate) base_env: &'a EnvMap,
    /// keys the task's own env sets
    pub(crate) task_env_keys: &'a BTreeSet<String>,
    /// inherited secret keys that mise's own env sets for this child; M2 strips them from
    /// `base_env`, so they are carried here
    pub(crate) mise_set_inherited: &'a BTreeSet<String>,
    /// every key mise itself sets for this child, whatever its value
    pub(crate) mise_env_keys: &'a BTreeSet<String>,
    pub(crate) sandbox: &'a SandboxConfig,
    /// `None` refuses `as_file` keys
    pub(crate) file_dir: Option<&'a Path>,
    pub(crate) terminal: &'a dyn TerminalAccess,
    pub(crate) ctx_builder: &'a TaskContextBuilder,
    pub(crate) denied: Option<SecretsDenied>,
    pub(crate) interactive: bool,
}

type Opened = std::result::Result<Arc<dyn SecretSource>, Arc<str>>;

#[derive(Hash, PartialEq, Eq, Clone)]
struct OpenKey {
    root: PathBuf,
    profile: Option<String>,
    /// the config file whose hierarchy built a monorepo task's toolset
    toolset: Option<PathBuf>,
}

pub(crate) struct SourceMemo {
    source: Arc<dyn SecretSource>,
    catalog: OnceCell<std::result::Result<Arc<Catalog>, Arc<str>>>,
    values: Mutex<MemoValues>,
}

#[derive(Default)]
struct MemoValues {
    set: BTreeMap<SecretName, SecretValue>,
    files: BTreeMap<SecretName, SecretValue>,
    remove: BTreeSet<String>,
    missing: BTreeSet<SecretName>,
    failed: BTreeMap<SecretName, Arc<str>>,
}

#[derive(Default)]
pub(crate) struct SecretBroker {
    opened: DashMap<OpenKey, Arc<OnceCell<Opened>>>,
    sources: DashMap<(SourceId, String), Arc<SourceMemo>>,
}

impl SourceMemo {
    pub(crate) fn new(source: Arc<dyn SecretSource>) -> Arc<Self> {
        Arc::new(Self {
            source,
            catalog: OnceCell::new(),
            values: Mutex::new(MemoValues::default()),
        })
    }

    async fn catalog(&self) -> Result<Arc<Catalog>> {
        self.catalog
            .get_or_init(|| async {
                self.source
                    .describe()
                    .await
                    .map(Arc::new)
                    .map_err(|e| Arc::from(format!("{e:#}")))
            })
            .await
            .clone()
            .map_err(|m| eyre!("{m}"))
    }
}

impl MemoValues {
    /// Keys still to resolve, or the first key that failed earlier in this process.
    fn missing_for(
        &self,
        keys: &BTreeSet<SecretName>,
    ) -> std::result::Result<Vec<SecretName>, SecretName> {
        let mut pending = vec![];
        for key in keys {
            if self.set.contains_key(key)
                || self.files.contains_key(key)
                || self.missing.contains(key)
            {
                continue;
            }
            if self.failed.contains_key(key) {
                return Err(key.clone());
            }
            pending.push(key.clone());
        }
        Ok(pending)
    }

    async fn resolve(
        &mut self,
        memo: &SourceMemo,
        catalog: &Catalog,
        keys: Vec<SecretName>,
        interactive: bool,
        label: &str,
    ) -> Result<()> {
        let selection = KeySelection::Keys(keys.iter().cloned().collect());
        match memo
            .source
            .resolve(&SourceCx { interactive }, &selection, catalog)
            .await
        {
            Ok(resolved) => {
                self.accept(resolved, &keys, label);
                Ok(())
            }
            Err(err) => {
                let (message, error) = resolve_error(&err, &keys, memo, interactive, label);
                let message: Arc<str> = Arc::from(message);
                // `invalid_keys` names the keys it rejects; the others were never tried and
                // stay retryable. Any other failure marks the whole batch.
                let rejected: BTreeSet<&str> = match &err {
                    ResolveError::Invalid {
                        unknown,
                        not_injectable,
                        ..
                    } => unknown
                        .iter()
                        .chain(not_injectable)
                        .map(String::as_str)
                        .collect(),
                    _ => BTreeSet::new(),
                };
                let named = keys.iter().any(|k| rejected.contains(k.as_str()));
                for key in &keys {
                    if !named || rejected.contains(key.as_str()) {
                        self.failed.insert(key.clone(), message.clone());
                    }
                }
                Err(error)
            }
        }
    }

    fn accept(&mut self, resolved: Resolved, keys: &[SecretName], label: &str) {
        let Resolved {
            set,
            files,
            remove,
            missing,
            unrequested,
            not_injectable,
        } = resolved;
        if !unrequested.is_empty() {
            warn!(
                "fnox returned keys that were not requested ({}); they were ignored",
                unrequested.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        }
        for key in &not_injectable {
            warn!(
                "fnox returned {key}, which its config marks as not injectable; it was not passed to task {label}"
            );
        }
        register_redactions(set.iter().chain(files.iter()));
        self.set.extend(set);
        self.files.extend(files);
        self.remove.extend(remove);
        self.missing.extend(missing);
        for key in keys {
            if !self.set.contains_key(key) && !self.files.contains_key(key) {
                self.missing.insert(key.clone());
            }
        }
    }
}

/// Registers every value, its lines and its JSON-escaped form with the output redactor,
/// before anything is returned to a caller.
fn register_redactions<'a>(values: impl Iterator<Item = (&'a SecretName, &'a SecretValue)>) {
    let mut patterns = vec![];
    for (key, value) in values {
        if value.expose().len() < 4 {
            warn_once!(
                "secret {key} is shorter than 4 characters; mise redacts every occurrence of it in task output"
            );
        }
        patterns.extend(mise_util::redactions::secret_patterns(value.expose()));
    }
    crate::config::add_secret_redactions(patterns);
}

fn list(keys: &[SecretName]) -> String {
    keys.iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The user-facing error for a failed resolve, and the text to remember for G15.
fn resolve_error(
    err: &ResolveError,
    keys: &[SecretName],
    memo: &SourceMemo,
    interactive: bool,
    label: &str,
) -> (String, eyre::Report) {
    let source_label = memo.source.label();
    let text = match err {
        ResolveError::Invalid {
            unknown,
            suggestions,
            not_injectable,
        } => {
            let mut lines = vec![];
            for key in unknown {
                let mut hint = format!("{source_label} has no secret named {key}.");
                if let Some(s) = suggestions.get(key).and_then(|s| s.first()) {
                    hint.push_str(&format!(" Did you mean {s}?"));
                }
                lines.push(format!("task {label}: unknown secret {key}\n  {hint}"));
            }
            for key in not_injectable {
                lines.push(format!(
                    "task {label}: {key} cannot be injected\n  fnox config sets env = false for {key}, so fnox never hands it to processes."
                ));
            }
            if lines.is_empty() {
                lines.push(format!(
                    "task {label}: fnox rejected the requested keys ({})",
                    list(keys)
                ));
            }
            lines.push("See the available keys with `mise secrets ls`.".to_string());
            lines.join("\n")
        }
        ResolveError::Resolution(message) => {
            let profile = memo
                .source
                .id()
                .profile
                .as_ref()
                .map(|p| format!(" (profile {p})"))
                .unwrap_or_default();
            let mut text = format!(
                "task {label}: fnox could not resolve {}{profile}\n  {message}",
                list(keys)
            );
            if !interactive {
                text.push_str("\n  mise ran fnox without a terminal (CI or no TTY), so fnox could not prompt you to sign in. Sign in first (for example op signin) or give the provider's credentials to CI.");
            }
            text
        }
        ResolveError::Other(message) => format!("task {label}: {message}"),
    };
    (text.clone(), eyre::Report::new(ResolveFailed(text)))
}

/// G14 and G15. The executor prints these itself, so `mise run` never reprints them: a later
/// task's G15 is a consequence of an earlier failure and would otherwise be silent, while
/// the earlier G14 may itself have been dropped as collateral.
#[derive(Debug)]
pub(crate) struct ResolveFailed(String);

pub fn is_resolve_failure(err: &eyre::Report) -> bool {
    err.downcast_ref::<ResolveFailed>().is_some()
}

impl std::fmt::Display for ResolveFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ResolveFailed {}

fn g15(label: &str, key: &SecretName) -> eyre::Report {
    eyre::Report::new(ResolveFailed(format!(
        "task {label}: not retrying {key}; fnox failed to resolve it earlier in this run (see above)"
    )))
}

/// G11: mise itself sets `key` for this child. A key mise sets is a collision even when its
/// value equals the shell's (mise's env may already be exported into the shell). A value that
/// merely came from the shell, with mise setting nothing, is allowed, and the secret wins.
fn collides(
    key: &str,
    task_env_keys: &BTreeSet<String>,
    mise_set_inherited: &BTreeSet<String>,
    mise_env_keys: &BTreeSet<String>,
    base_env: &EnvMap,
    pristine: &EnvMap,
) -> bool {
    task_env_keys.iter().any(|t| env_key_eq(t, key))
        || mise_env_keys.iter().any(|t| env_key_eq(t, key))
        || mise_set_inherited.iter().any(|t| env_key_eq(t, key))
        || get_eq(base_env, key).is_some_and(|v| get_eq(pristine, key) != Some(v))
}

fn get_eq<'a>(env: &'a EnvMap, key: &str) -> Option<&'a String> {
    env.get(key)
        .or_else(|| env.iter().find(|(k, _)| env_key_eq(k, key)).map(|(_, v)| v))
}

impl SecretBroker {
    async fn open(
        &self,
        config: &Arc<Config>,
        ctx: &TaskContextBuilder,
        task: &Task,
        selected: &super::config::SelectedSource,
    ) -> Result<Arc<SourceMemo>> {
        let monorepo = task.cf.is_some() && !task.is_remote();
        let key = OpenKey {
            root: selected.root.clone(),
            profile: selected.profile.clone(),
            toolset: monorepo
                .then(|| task.cf.as_ref().map(|cf| cf.get_path().to_path_buf()))
                .flatten(),
        };
        let cell = self.opened.entry(key).or_default().clone();
        let opened = cell
            .get_or_init(|| async {
                let owned: Toolset;
                let mut config_env = None;
                let ts: &Toolset = if monorepo {
                    let task_cf = task.cf.as_ref().expect("monorepo task has a config file");
                    owned = ctx
                        .build_toolset_for_task(config, task, Some(task_cf), &[])
                        .await
                        .map_err(|e| Arc::<str>::from(format!("{e:#}")))?;
                    config_env = ctx
                        .config_env_for_source(config, task, &owned, false)
                        .await
                        .map_err(|e| Arc::<str>::from(format!("{e:#}")))?;
                    &owned
                } else {
                    config
                        .get_toolset()
                        .await
                        .map_err(|e| Arc::<str>::from(format!("{e:#}")))?
                };
                super::open_source(config, selected, ts, config_env)
                    .await
                    .map_err(|e| Arc::<str>::from(format!("{e:#}")))
            })
            .await
            .clone()
            .map_err(|m| eyre!("{m}"))?;
        Ok(self.memo_for(opened))
    }

    /// One memo per (id, build fingerprint): sources built from different envs must not
    /// share resolved values.
    fn memo_for(&self, opened: Arc<dyn SecretSource>) -> Arc<SourceMemo> {
        let memo_key = (opened.id().clone(), opened.build_fingerprint());
        self.sources
            .entry(memo_key)
            .or_insert_with(|| SourceMemo::new(opened))
            .clone()
    }

    /// Checks every grant against the source it would use, describing each distinct source
    /// once. Returns every problem; nothing is spawned.
    pub(crate) async fn preflight(
        &self,
        config: &Arc<Config>,
        ctx: &TaskContextBuilder,
        items: &[(Grantee<'_>, SecretGrant)],
    ) -> Result<Vec<Problem>> {
        let mut problems = vec![];
        // source-level failures are reported once, with every task they affect
        let mut source_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (grantee, grant) in items {
            if grant.is_empty() {
                continue;
            }
            let task = grantee.task();
            // a config that does not parse, or is not trusted, fails on its own
            let selected = match self.select(config, ctx, task).await? {
                Ok(selected) => selected,
                Err(no_source) => {
                    source_errors
                        .entry(no_source)
                        .or_default()
                        .push(task.name.clone());
                    continue;
                }
            };
            match self.open_selected(config, ctx, task, selected).await {
                Ok(memo) => match memo.catalog().await {
                    Ok(catalog) => {
                        problems.extend(key_problems(task, grant, &catalog, &memo.source.label()))
                    }
                    Err(e) => source_errors
                        .entry(format!("{e:#}"))
                        .or_default()
                        .push(task.name.clone()),
                },
                Err(e) => source_errors
                    .entry(format!("{e:#}"))
                    .or_default()
                    .push(task.name.clone()),
            }
        }
        for (message, mut tasks) in source_errors {
            tasks.sort();
            tasks.dedup();
            let first = tasks[0].clone();
            problems.push(Problem::new(
                &first,
                None,
                ProblemKind::Source,
                format!(
                    "{} {}: {}",
                    if tasks.len() == 1 { "task" } else { "tasks" },
                    tasks.join(", "),
                    message.replace('\n', "\n  ")
                ),
            ));
        }
        Ok(problems)
    }

    async fn select(
        &self,
        config: &Arc<Config>,
        ctx: &TaskContextBuilder,
        task: &Task,
    ) -> Result<std::result::Result<super::config::SelectedSource, String>> {
        let selection = super::config::select_for_task(config, ctx, task).await?;
        Ok(selection.source.ok_or_else(|| {
            super::no_source_message(
                &task.config_root.clone().unwrap_or_default(),
                &selection.ignored,
            )
        }))
    }

    /// The catalog and label of an already selected source, through the same memoized open
    /// that `mise run` uses.
    pub(crate) async fn catalog_for(
        &self,
        config: &Arc<Config>,
        ctx: &TaskContextBuilder,
        task: &Task,
        selected: &super::config::SelectedSource,
    ) -> Result<(Arc<Catalog>, String)> {
        let memo = self.open(config, ctx, task, selected).await?;
        let catalog = memo.catalog().await?;
        Ok((catalog, memo.source.label()))
    }

    async fn open_selected(
        &self,
        config: &Arc<Config>,
        ctx: &TaskContextBuilder,
        task: &Task,
        selected: super::config::SelectedSource,
    ) -> Result<Arc<SourceMemo>> {
        self.open(config, ctx, task, &selected).await
    }

    async fn memo_for_task(
        &self,
        config: &Arc<Config>,
        ctx: &TaskContextBuilder,
        task: &Task,
    ) -> Result<Arc<SourceMemo>> {
        let selection = super::config::select_for_task(config, ctx, task).await?;
        let Some(selected) = selection.source else {
            bail!(
                "{}",
                super::no_source_message(
                    &task.config_root.clone().unwrap_or_default(),
                    &selection.ignored
                )
            );
        };
        self.open(config, ctx, task, &selected).await
    }

    /// Ok(None) when the grant is empty: no source selected, no process run.
    pub(crate) async fn prepare_spawn(
        &self,
        config: &Arc<Config>,
        req: SpawnRequest<'_>,
    ) -> Result<Option<SpawnSecrets>> {
        if req.grant.is_empty() {
            return Ok(None);
        }
        Settings::get().ensure_experimental("mise secrets")?;
        Settings::ensure_not_safe("mise secrets")?;
        let task = req.grantee.task();
        let (_, mut problems) = grant_for_task(task);
        let env_view = super::EnvView::load(config)
            .await
            .for_task(config, req.ctx_builder, task)
            .await;
        problems.extend(static_problems(task, req.grant, req.denied, &env_view));
        if !problems.is_empty() {
            bail!(
                "{}",
                problems
                    .iter()
                    .map(Problem::render)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        let memo = self.memo_for_task(config, req.ctx_builder, task).await?;
        self.grant_values(&memo, &req).await.map(Some)
    }

    /// Everything after the source is known: catalog checks, sandbox and collision checks,
    /// resolution, files, `remove`.
    pub(crate) async fn grant_values(
        &self,
        memo: &Arc<SourceMemo>,
        req: &SpawnRequest<'_>,
    ) -> Result<SpawnSecrets> {
        let label = req.grantee.label().to_string();
        let task = req.grantee.task();
        let catalog = memo.catalog().await?;
        let mut problems = key_problems(task, req.grant, &catalog, &memo.source.label());
        for key in req.grant.keys.keys() {
            if let Some(entry) = catalog.entries.get(key)
                && entry.as_file
                && req.file_dir.is_none()
            {
                problems.push(Problem::new(
                    &label,
                    Some(key.as_str()),
                    ProblemKind::Source,
                    format!(
                        "task {label}: {key} is delivered as a file (as_file = true), which is not supported here"
                    ),
                ));
            }
        }
        // the sandbox decides before anything is resolved
        for key in req.grant.keys.keys() {
            if !req.sandbox.keeps_env_key(key.as_str()) {
                problems.push(sandbox_problem(&label, key.as_str()));
            }
        }
        // a value inherited from the shell is allowed (the secret wins); a value mise itself
        // sets is not
        for key in req.grant.keys.keys() {
            let k = key.as_str();
            let from_mise = collides(
                k,
                req.task_env_keys,
                req.mise_set_inherited,
                req.mise_env_keys,
                req.base_env,
                &env::PRISTINE_ENV,
            );
            if from_mise {
                problems.push(collision_problem(&label, k));
            }
        }
        if !problems.is_empty() {
            bail!(
                "{}",
                problems
                    .iter()
                    .map(Problem::render)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        let keys: BTreeSet<SecretName> = req.grant.keys.keys().cloned().collect();
        self.ensure_resolved(memo, &catalog, &keys, req, &label)
            .await?;

        let v = memo.values.lock().await;
        let mut env_values = BTreeMap::new();
        let mut file_values = BTreeMap::new();
        for key in &keys {
            if let Some(value) = v.set.get(key) {
                env_values.insert(key.to_string(), value.clone());
            } else if let Some(value) = v.files.get(key) {
                file_values.insert(key.clone(), value.clone());
            } else {
                debug!("{key} resolved to nothing; leaving it unset for task {label}");
            }
        }
        let mut remove: BTreeSet<String> = v
            .remove
            .iter()
            .filter(|k| !mise_util::env::is_reserved_secret_name(k))
            .cloned()
            .collect();
        drop(v);
        // never delete a key mise itself sets for this child, nor one this spawn sets
        remove.retain(|k| {
            !keys.iter().any(|set| env_key_eq(set.as_str(), k))
                && !env_values.keys().any(|set| env_key_eq(set, k))
                && !collides(
                    k,
                    req.task_env_keys,
                    req.mise_set_inherited,
                    req.mise_env_keys,
                    req.base_env,
                    &env::PRISTINE_ENV,
                )
        });
        let (files, file_env) = match (file_values.is_empty(), req.file_dir) {
            (true, _) => (TempSecretFiles::default(), BTreeMap::new()),
            (false, Some(dir)) => TempSecretFiles::create(dir, &file_values)?,
            (false, None) => bail!("task {label}: secret files are not supported here"),
        };
        Ok(SpawnSecrets::new(env_values, files, file_env, remove))
    }

    /// Lock order is terminal, then memo; nobody holds a memo lock while waiting for the
    /// terminal.
    async fn ensure_resolved(
        &self,
        memo: &Arc<SourceMemo>,
        catalog: &Catalog,
        keys: &BTreeSet<SecretName>,
        req: &SpawnRequest<'_>,
        label: &str,
    ) -> Result<()> {
        let pending;
        {
            let mut v = memo.values.lock().await;
            let missing = v.missing_for(keys).map_err(|k| g15(label, &k))?;
            if missing.is_empty() {
                return Ok(());
            }
            if !req.interactive {
                return v.resolve(memo, catalog, missing, false, label).await;
            }
            pending = missing;
        }
        let _terminal = req.terminal.acquire(&pending).await;
        let _pause = MultiProgressReport::try_get().map(|r| r.pause_progress());
        let mut v = memo.values.lock().await;
        let missing = v.missing_for(keys).map_err(|k| g15(label, &k))?;
        if missing.is_empty() {
            return Ok(());
        }
        v.resolve(memo, catalog, missing, true, label).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::secrets::source::CatalogEntry;
    use crate::secrets::source::KeyKind;

    #[derive(Debug, Default)]
    struct Fake {
        id: Option<SourceId>,
        calls: AtomicUsize,
        asked: StdMutex<Vec<Vec<String>>>,
        fail: BTreeSet<String>,
        reject: StdMutex<BTreeSet<String>>,
        fingerprint: String,
    }

    fn catalog() -> Catalog {
        let mut entries = IndexMap::new();
        for k in ["A", "B", "C", "DEPLOY_KEY", "PATH_LIKE", "SHORT", "LEASE"] {
            entries.insert(
                SecretName::new(k).unwrap(),
                CatalogEntry {
                    kind: KeyKind::Secret,
                    mode: None,
                    as_file: false,
                    injectable: true,
                    description: None,
                },
            );
        }
        Catalog {
            entries,
            profile: vec![],
            dynamic_leases: vec![],
            tool_version: "1".into(),
        }
    }

    #[async_trait::async_trait]
    impl SecretSource for Fake {
        fn id(&self) -> &SourceId {
            self.id.as_ref().unwrap()
        }
        fn label(&self) -> String {
            "fake in /p".into()
        }
        async fn describe(&self) -> Result<Catalog> {
            Ok(catalog())
        }
        fn build_fingerprint(&self) -> String {
            self.fingerprint.clone()
        }
        async fn resolve(
            &self,
            _cx: &SourceCx,
            keys: &KeySelection,
            _catalog: &Catalog,
        ) -> std::result::Result<Resolved, ResolveError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            self.asked
                .lock()
                .unwrap()
                .push(keys.keys().iter().map(|k| k.to_string()).collect());
            let rejected: Vec<String> = keys
                .keys()
                .iter()
                .filter(|k| self.reject.lock().unwrap().contains(k.as_str()))
                .map(|k| k.to_string())
                .collect();
            if !rejected.is_empty() {
                return Err(ResolveError::Invalid {
                    unknown: rejected,
                    suggestions: BTreeMap::new(),
                    not_injectable: vec![],
                });
            }
            if keys.keys().iter().any(|k| self.fail.contains(k.as_str())) {
                return Err(ResolveError::Resolution("not signed in".into()));
            }
            let mut out = Resolved::default();
            for k in keys.keys() {
                let value = match k.as_str() {
                    "SHORT" => "ab".to_string(),
                    "DEPLOY_KEY" => "line-one-s3cr3t\nline-two-s3cr3t \"q\"".to_string(),
                    other => format!("{other}-value-s3cr3t"),
                };
                out.set.insert(k.clone(), SecretValue::new(value));
            }
            out.remove = BTreeSet::from(["SCRUB".to_string(), "A".to_string()]);
            // like a lease credential: fnox leaves it out of `remove` only when requested
            if !keys.keys().iter().any(|k| k.as_str() == "LEASE") {
                out.remove.insert("LEASE".to_string());
            }
            Ok(out)
        }
    }

    fn fake(fail: &[&str]) -> (Arc<Fake>, Arc<SourceMemo>) {
        let f = Arc::new(Fake {
            id: Some(SourceId {
                kind: "fake",
                root: PathBuf::from("/p"),
                profile: None,
            }),
            fail: fail.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        });
        let memo = SourceMemo::new(f.clone());
        (f, memo)
    }

    struct Terminal {
        acquired: AtomicUsize,
        memo: StdMutex<Option<Arc<SourceMemo>>>,
        memo_locked_during_acquire: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl TerminalAccess for Terminal {
        async fn acquire(
            &self,
            _keys: &[SecretName],
        ) -> Option<tokio::sync::RwLockWriteGuard<'static, ()>> {
            self.acquired.fetch_add(1, Ordering::SeqCst);
            if let Some(memo) = self.memo.lock().unwrap().as_ref()
                && memo.values.try_lock().is_err()
            {
                self.memo_locked_during_acquire
                    .fetch_add(1, Ordering::SeqCst);
            }
            None
        }
    }

    fn terminal() -> Terminal {
        Terminal {
            acquired: AtomicUsize::new(0),
            memo: StdMutex::new(None),
            memo_locked_during_acquire: AtomicUsize::new(0),
        }
    }

    fn task(name: &str, keys: &[&str]) -> Task {
        Task {
            name: name.into(),
            config_source: PathBuf::from("/p/mise.toml"),
            secrets: Some(super::super::TaskSecrets(
                keys.iter().map(|s| s.to_string()).collect(),
            )),
            ..Default::default()
        }
    }

    struct Inputs {
        task: Task,
        grant: SecretGrant,
        base: EnvMap,
        task_env: BTreeSet<String>,
        inherited: BTreeSet<String>,
        mise_env: BTreeSet<String>,
        sandbox: SandboxConfig,
        ctx: TaskContextBuilder,
    }

    impl Inputs {
        fn new(name: &str, keys: &[&str]) -> Self {
            let task = task(name, keys);
            let (grant, _) = grant_for_task(&task);
            Self {
                task,
                grant,
                base: EnvMap::new(),
                task_env: BTreeSet::new(),
                inherited: BTreeSet::new(),
                mise_env: BTreeSet::new(),
                sandbox: SandboxConfig::default(),
                ctx: TaskContextBuilder::new(),
            }
        }

        fn req<'a>(&'a self, term: &'a Terminal, interactive: bool) -> SpawnRequest<'a> {
            SpawnRequest {
                grantee: Grantee::Task(&self.task),
                grant: &self.grant,
                base_env: &self.base,
                task_env_keys: &self.task_env,
                mise_set_inherited: &self.inherited,
                mise_env_keys: &self.mise_env,
                sandbox: &self.sandbox,
                file_dir: None,
                terminal: term,
                ctx_builder: &self.ctx,
                denied: None,
                interactive,
            }
        }
    }

    #[tokio::test]
    async fn concurrent_spawns_resolve_once_then_only_missing_keys() {
        let (fake, memo) = fake(&[]);
        let broker = SecretBroker::default();
        let term = terminal();
        let one = Inputs::new("one", &["A", "B"]);
        let two = Inputs::new("two", &["A", "B"]);
        let (req1, req2) = (one.req(&term, false), two.req(&term, false));
        let (r1, r2) = tokio::join!(
            broker.grant_values(&memo, &req1),
            broker.grant_values(&memo, &req2)
        );
        let (s1, s2) = (r1.unwrap(), r2.unwrap());
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        assert_eq!(s1.marker_value(), "A,B");
        assert_eq!(s2.marker_value(), "A,B");
        let three = Inputs::new("three", &["A", "C"]);
        broker
            .grant_values(&memo, &three.req(&term, false))
            .await
            .unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        assert_eq!(fake.asked.lock().unwrap()[1], ["C"]);
        assert_eq!(term.acquired.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failures_are_memoized_and_not_retried() {
        let (fake, memo) = fake(&["B"]);
        let broker = SecretBroker::default();
        let term = terminal();
        let one = Inputs::new("e1", &["B"]);
        let err = broker
            .grant_values(&memo, &one.req(&term, false))
            .await
            .unwrap_err();
        assert!(crate::secrets::is_resolve_failure(&err));
        let err = err.to_string();
        assert!(err.contains("fnox could not resolve B"), "{err}");
        assert!(err.contains("without a terminal"), "{err}");
        let two = Inputs::new("e2", &["B"]);
        let err = broker
            .grant_values(&memo, &two.req(&term, false))
            .await
            .unwrap_err();
        assert!(crate::secrets::is_resolve_failure(&err));
        let err = err.to_string();
        assert!(err.contains("task e2: not retrying B"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        let mut c = Inputs::new("c", &["A"]);
        c.base.insert("A".into(), "from-mise".into());
        let err = broker
            .grant_values(&memo, &c.req(&term, false))
            .await
            .unwrap_err();
        assert!(!crate::secrets::is_resolve_failure(&err));
    }

    /// `remove` accumulates in the shared memo, but per spawn it never deletes a key the spawn
    /// sets, and a union-only entry is one fnox itself lists for the profile.
    #[tokio::test]
    async fn accumulated_remove_never_deletes_what_a_task_sets() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake(&[]);
        // task A is granted the "lease" key; its own response leaves LEASE out of remove
        let a = Inputs::new("a", &["A", "LEASE"]);
        let spawn_a = broker
            .grant_values(&memo, &a.req(&term, false))
            .await
            .unwrap();
        assert!(!spawn_a.remove.contains("LEASE"));
        // task B asks for other keys; fnox's response lists LEASE (B does not request it)
        let b = Inputs::new("b", &["C"]);
        let spawn_b = broker
            .grant_values(&memo, &b.req(&term, false))
            .await
            .unwrap();
        assert!(spawn_b.remove.contains("LEASE"));
        assert!(spawn_b.remove.contains("SCRUB"));
        // the union now holds LEASE, yet A (memoized, no new call) still keeps its own value
        let spawn_a = broker
            .grant_values(&memo, &a.req(&term, false))
            .await
            .unwrap();
        assert!(!spawn_a.remove.contains("LEASE"), "{:?}", spawn_a.remove);
        assert!(spawn_a.marker_value().contains("LEASE"));
        // and nothing either task sets is ever in its own remove list
        for (spawn, set) in [
            (&spawn_a, ["A", "LEASE"].as_slice()),
            (&spawn_b, ["C"].as_slice()),
        ] {
            for key in set {
                assert!(!spawn.remove.contains(*key), "{key}");
            }
        }
    }

    #[tokio::test]
    async fn invalid_keys_fail_only_the_rejected_key() {
        let (fake, memo) = fake(&[]);
        fake.reject.lock().unwrap().insert("B".to_string());
        let broker = SecretBroker::default();
        let term = terminal();
        let both = Inputs::new("t1", &["A", "B"]);
        let err = broker
            .grant_values(&memo, &both.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown secret B"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        // A was never tried: it is retried, not "not retrying"
        let a = Inputs::new("t2", &["A"]);
        broker
            .grant_values(&memo, &a.req(&term, false))
            .await
            .unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        // B stays failed
        let b = Inputs::new("t3", &["B"]);
        let err = broker
            .grant_values(&memo, &b.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not retrying B"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn sources_with_different_envs_get_different_memos() {
        let broker = SecretBroker::default();
        let term = terminal();
        let mk = |fp: &str| {
            Arc::new(Fake {
                id: Some(SourceId {
                    kind: "fake",
                    root: PathBuf::from("/p"),
                    profile: None,
                }),
                fingerprint: fp.into(),
                ..Default::default()
            })
        };
        let (staging, prod, staging2) = (mk("staging"), mk("prod"), mk("staging"));
        let m1 = broker.memo_for(staging.clone());
        let m2 = broker.memo_for(prod.clone());
        let m3 = broker.memo_for(staging2.clone());
        assert!(!Arc::ptr_eq(&m1, &m2));
        assert!(Arc::ptr_eq(&m1, &m3));
        let inputs = Inputs::new("t", &["A"]);
        for memo in [&m1, &m2, &m3] {
            broker
                .grant_values(memo, &inputs.req(&term, false))
                .await
                .unwrap();
        }
        assert_eq!(staging.calls.load(Ordering::SeqCst), 1);
        assert_eq!(prod.calls.load(Ordering::SeqCst), 1);
        assert_eq!(staging2.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn remove_never_names_a_key_mise_sets() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake(&[]);
        let mut inputs = Inputs::new("t", &["B"]);
        inputs.task_env.insert("SCRUB".into());
        inputs.base.insert("SCRUB".into(), "task-value".into());
        let spawn = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
        assert!(spawn.remove.is_empty() || !spawn.remove.contains("SCRUB"));
        let mut env = inputs.base.clone();
        let mut env_remove = BTreeSet::new();
        spawn.apply(&mut env, &mut env_remove);
        assert_eq!(env.get("SCRUB").map(String::as_str), Some("task-value"));
        // a key mise does not set is still scrubbed
        let other = Inputs::new("u", &["B"]);
        let spawn = broker
            .grant_values(&memo, &other.req(&term, false))
            .await
            .unwrap();
        assert!(spawn.remove.contains("SCRUB"));
    }

    #[test]
    fn collision_rule() {
        let none = BTreeSet::new();
        let pristine = EnvMap::from([("SHELL_SET".into(), "v".into())]);
        let base = EnvMap::from([
            ("SHELL_SET".into(), "v".into()),
            ("MISE_SET".into(), "x".into()),
            ("CHANGED".into(), "new".into()),
        ]);
        let pristine = {
            let mut p = pristine;
            p.insert("CHANGED".into(), "old".into());
            p
        };
        assert!(!collides(
            "SHELL_SET",
            &none,
            &none,
            &none,
            &base,
            &pristine
        ));
        assert!(collides("MISE_SET", &none, &none, &none, &base, &pristine));
        assert!(collides("CHANGED", &none, &none, &none, &base, &pristine));
        assert!(!collides("OTHER", &none, &none, &none, &base, &pristine));
        let own = BTreeSet::from(["OTHER".to_string()]);
        let own_shell = BTreeSet::from(["SHELL_SET".to_string()]);
        assert!(collides("OTHER", &own, &none, &none, &base, &pristine));
        assert!(collides("OTHER", &none, &own, &none, &base, &pristine));
        // mise sets it, to the very value the shell already has
        assert!(!collides(
            "SHELL_SET",
            &none,
            &none,
            &none,
            &base,
            &pristine
        ));
        assert!(collides(
            "SHELL_SET",
            &none,
            &none,
            &own_shell,
            &base,
            &pristine
        ));
    }

    #[tokio::test]
    async fn collisions_are_g11() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake(&[]);
        let mut inputs = Inputs::new("t", &["A"]);
        inputs.base.insert("A".into(), "from-mise".into());
        let err = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("A is both a secret and a mise env var"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn sandbox_denying_env_is_g13() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (fake, memo) = fake(&[]);
        let mut inputs = Inputs::new("deploy", &["A"]);
        inputs.sandbox.deny_env = true;
        let err = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("task deploy is granted A, but its sandbox denies env vars"),
            "{err}"
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        inputs.sandbox.allow_env = vec!["A".into()];
        broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn unknown_keys_are_g1_with_a_suggestion() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (fake, memo) = fake(&[]);
        let inputs = Inputs::new("deploy", &["DEPLOY_KYE"]);
        let err = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("task deploy: unknown secret DEPLOY_KYE"),
            "{err}"
        );
        assert!(err.contains("Did you mean DEPLOY_KEY?"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn registers_lines_and_json_forms_and_excludes_set_keys_from_remove() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake(&[]);
        let inputs = Inputs::new("deploy", &["A", "DEPLOY_KEY", "SHORT"]);
        let spawn = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
        // remove lists SCRUB but not A, which this spawn sets
        assert_eq!(
            spawn.remove,
            BTreeSet::from(["SCRUB".to_string(), "LEASE".to_string()])
        );
        for leaked in [
            "line-one-s3cr3t",
            "line-two-s3cr3t \"q\"",
            "line-one-s3cr3t\\nline-two-s3cr3t",
            "A-value-s3cr3t",
            "ab",
        ] {
            let out = mise_util::redactions::redact_global(&format!("x {leaked} y"));
            assert!(!out.contains(leaked), "{leaked}: {out}");
        }
        assert!(!format!("{spawn:?}").contains("s3cr3t"));
    }

    #[tokio::test]
    async fn terminal_is_acquired_only_when_interactive_and_never_under_the_memo_lock() {
        let broker = SecretBroker::default();
        let (_, memo) = fake(&[]);
        let term = terminal();
        *term.memo.lock().unwrap() = Some(memo.clone());
        let one = Inputs::new("one", &["A"]);
        broker
            .grant_values(&memo, &one.req(&term, true))
            .await
            .unwrap();
        assert_eq!(term.acquired.load(Ordering::SeqCst), 1);
        assert_eq!(term.memo_locked_during_acquire.load(Ordering::SeqCst), 0);
        // everything is memoized now: no terminal
        broker
            .grant_values(&memo, &one.req(&term, true))
            .await
            .unwrap();
        assert_eq!(term.acquired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resolved_debug_lists_names_only() {
        let mut r = Resolved::default();
        r.set
            .insert(SecretName::new("A").unwrap(), SecretValue::new("s3cr3t"));
        let text = format!("{r:?}");
        assert!(text.contains('A') && !text.contains("s3cr3t"), "{text}");
    }
}
