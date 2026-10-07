//! Resolves granted keys at spawn time. Values live only in `SourceMemo`, in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use eyre::{Result, bail, eyre};
use tokio::sync::{Mutex, OnceCell};

use super::grant::{
    GrantOrigin, Problem, ProblemKind, SecretGrant, SecretsDenied, Subject, collision_problem,
    exec_grant, grant_for_task, key_problems, sandbox_problem, static_problems,
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
    /// `mise x`
    Exec,
}

impl Grantee<'_> {
    fn task(&self) -> Option<&Task> {
        match self {
            Self::Task(task) => Some(task),
            Self::Exec => None,
        }
    }

    fn subject(&self) -> Subject<'_> {
        match self {
            Self::Task(task) => Subject::Task(&task.name),
            Self::Exec => Subject::Exec,
        }
    }
}

/// What an interactive fnox call is about to ask for.
#[derive(Clone)]
pub(crate) enum Pending {
    Keys(Vec<SecretName>),
    /// no key named: everything the source injects
    All,
}

impl Pending {
    fn selection(&self) -> KeySelection {
        match self {
            Self::Keys(keys) => KeySelection::Keys(keys.iter().cloned().collect()),
            Self::All => KeySelection::AllInScope,
        }
    }

    /// For "asking fnox for ...".
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Keys(keys) => list(keys),
            Self::All => "all secrets".to_string(),
        }
    }
}

/// The terminal lock for an interactive fnox call.
#[async_trait::async_trait]
pub(crate) trait TerminalAccess: Send + Sync {
    async fn acquire(
        &self,
        pending: &Pending,
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
    /// keys whose task-env `default` directive actually rendered
    pub(crate) rendered_defaults: &'a BTreeSet<String>,
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
    /// The one `AllInScope` call for this source: `Ok` once its filtered document is in the
    /// maps above, `Err` with the reason it failed. Nothing is asked again after either.
    all: Option<std::result::Result<(), Arc<str>>>,
}

/// Why a request cannot be answered without trying again (G15).
enum Earlier {
    Key(SecretName),
    All,
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
    /// What still has to be asked of the source, or the earlier failure that rules it out.
    fn pending_for(
        &self,
        keys: &BTreeSet<SecretName>,
        all: bool,
    ) -> std::result::Result<Option<Pending>, Earlier> {
        match &self.all {
            // the whole document is here; a key it did not return is missing
            Some(Ok(())) => return Ok(None),
            Some(Err(_)) if all => return Err(Earlier::All),
            _ => {}
        }
        if all {
            if let Some(key) = self.failed.keys().next() {
                return Err(Earlier::Key(key.clone()));
            }
            return Ok(Some(Pending::All));
        }
        let mut pending = vec![];
        for key in keys {
            if self.set.contains_key(key)
                || self.files.contains_key(key)
                || self.missing.contains(key)
            {
                continue;
            }
            if self.failed.contains_key(key) {
                return Err(Earlier::Key(key.clone()));
            }
            // the failed all-in-scope batch covered every key, so a key that is not already
            // here would need a new call, which is not made; keys already here stay usable
            if matches!(self.all, Some(Err(_))) {
                return Err(Earlier::All);
            }
            pending.push(key.clone());
        }
        Ok((!pending.is_empty()).then_some(Pending::Keys(pending)))
    }

    async fn resolve(
        &mut self,
        memo: &SourceMemo,
        catalog: &Catalog,
        what: Pending,
        interactive: bool,
        subject: &str,
    ) -> Result<()> {
        let selection = what.selection();
        let result = memo
            .source
            .resolve(&SourceCx { interactive }, &selection, catalog)
            .await;
        self.apply(result, memo, catalog, what, interactive, subject)
    }

    /// Asks the source's cache, without a terminal or a process, for what is pending. `true`
    /// when it answered (a rejection counts: it is the same answer the CLI would give), and
    /// `false` when the caller has to resolve. Never waits on a person, so the caller may hold
    /// the memo lock.
    async fn resolve_cached(
        &mut self,
        memo: &SourceMemo,
        catalog: &Catalog,
        what: &Pending,
        subject: &str,
    ) -> Result<bool> {
        let selection = what.selection();
        let result = match memo
            .source
            .resolve_cached(&SourceCx { interactive: true }, &selection, catalog)
            .await
        {
            Ok(None) => return Ok(false),
            Ok(Some(resolved)) => Ok(resolved),
            Err(err) => Err(err),
        };
        self.apply(result, memo, catalog, what.clone(), true, subject)?;
        Ok(true)
    }

    fn apply(
        &mut self,
        result: std::result::Result<Resolved, ResolveError>,
        memo: &SourceMemo,
        catalog: &Catalog,
        what: Pending,
        interactive: bool,
        subject: &str,
    ) -> Result<()> {
        match result {
            Ok(resolved) => {
                match what {
                    Pending::Keys(keys) => self.accept(resolved, &keys, subject),
                    Pending::All => {
                        self.accept_all(resolved.filter_for_all(catalog), subject);
                        self.all = Some(Ok(()));
                    }
                }
                Ok(())
            }
            Err(err) => {
                let (message, error) = resolve_error(&err, &what, memo, interactive, subject);
                let message: Arc<str> = Arc::from(message);
                match what {
                    Pending::Keys(keys) => {
                        // `invalid_keys` names the keys it rejects; the others were never
                        // tried and stay retryable. Any other failure marks the whole batch.
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
                        for key in keys {
                            if !named || rejected.contains(key.as_str()) {
                                self.failed.insert(key, message.clone());
                            }
                        }
                    }
                    Pending::All => self.all = Some(Err(message)),
                }
                Err(error)
            }
        }
    }

    fn accept(&mut self, resolved: Resolved, keys: &[SecretName], subject: &str) {
        let Resolved {
            set,
            files,
            remove,
            missing,
            leases: _,
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
                "fnox returned {key}, which its config marks as not injectable; it was not passed to {subject}"
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

    /// The document of an `AllInScope` call, already filtered against the catalog.
    fn accept_all(&mut self, resolved: Resolved, subject: &str) {
        let Resolved {
            set,
            files,
            remove,
            missing,
            leases: _,
            unrequested,
            not_injectable,
        } = resolved;
        if !unrequested.is_empty() {
            warn!(
                "fnox returned keys its describe output does not list ({}); they were ignored",
                unrequested.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        }
        for key in &not_injectable {
            warn!(
                "fnox returned {key}, which its config marks as not injectable; it was not passed to {subject}"
            );
        }
        register_redactions(set.iter().chain(files.iter()));
        self.set.extend(set);
        self.files.extend(files);
        self.remove.extend(remove);
        self.missing.extend(missing);
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

/// E: the references of composed values that fnox delivers as files. Needs the catalog only,
/// so the preflight reports it before any task runs.
pub(super) fn late_file_problems(
    subject: Subject<'_>,
    grant: &SecretGrant,
    catalog: &Catalog,
) -> Vec<Problem> {
    let mut problems = vec![];
    for late in &grant.late {
        for name in &late.refs {
            if catalog.entries.get(name).is_some_and(|e| e.as_file) {
                problems.push(Problem::new(
                    &subject.label(),
                    Some(name.as_str()),
                    ProblemKind::Template,
                    file_composed_text(&subject.text(), &late.key, name),
                ));
            }
        }
    }
    problems
}

/// E: a file secret cannot be composed into an env value. Value-free.
fn file_composed_text(who: &str, var: &str, name: &SecretName) -> String {
    format!(
        "{who}: env.{var} uses {{{{ secrets.{name} }}}}, which fnox delivers as a file (as_file = true); a file secret cannot be composed into an env value"
    )
}

/// Renders the task's env values that use `{{ secrets.X }}` from the resolved values, and
/// registers each composite with the redactor before it is returned. The values are in
/// `values` only for the caller to read; the composites are returned in order, so a later
/// overlay entry for the same name wins.
fn render_late(
    grant: &SecretGrant,
    values: &MemoValues,
    who: &str,
) -> Result<Vec<(SecretName, SecretValue)>> {
    let mut out = vec![];
    for late in &grant.late {
        let mut found = BTreeMap::new();
        for name in &late.refs {
            if values.files.contains_key(name) && !values.set.contains_key(name) {
                bail!("{}", file_composed_text(who, &late.key, name));
            }
            let Some(value) = values.set.get(name) else {
                bail!(
                    "{who}: env.{}: {{{{ secrets.{name} }}}} resolved to no value",
                    late.key
                );
            };
            found.insert(name.clone(), value.clone());
        }
        let Some(text) = super::template::render(&late.template, &found) else {
            bail!("{who}: env.{} could not be rendered from secrets", late.key);
        };
        let key = SecretName::new(&late.key).ok_or_else(|| {
            eyre!(
                "{who}: env.{} is not a valid environment variable name",
                late.key
            )
        })?;
        out.push((key, SecretValue::new(text)));
    }
    register_redactions(out.iter().map(|(k, v)| (k, v)));
    Ok(out)
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
    what: &Pending,
    memo: &SourceMemo,
    interactive: bool,
    subject: &str,
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
                lines.push(format!("{subject}: unknown secret {key}\n  {hint}"));
            }
            for key in not_injectable {
                lines.push(format!(
                    "{subject}: {key} cannot be injected\n  fnox config sets env = false for {key}, so fnox never hands it to processes."
                ));
            }
            if lines.is_empty() {
                lines.push(format!(
                    "{subject}: fnox rejected the request for {}",
                    what.describe()
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
                "{subject}: fnox could not resolve {}{profile}\n  {message}",
                what.describe()
            );
            if !interactive {
                text.push_str("\n  mise ran fnox without a terminal (CI or no TTY), so fnox could not prompt you to sign in. Sign in first (for example op signin) or give the provider's credentials to CI.");
            }
            text
        }
        ResolveError::Other(message) => format!("{subject}: {message}"),
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

fn g15(subject: &str, earlier: &Earlier) -> eyre::Report {
    let what = match earlier {
        Earlier::Key(key) => key.to_string(),
        Earlier::All => "the secrets".to_string(),
    };
    eyre::Report::new(ResolveFailed(format!(
        "{subject}: not retrying {what}; fnox failed to resolve {} earlier in this run (see above)",
        match earlier {
            Earlier::Key(_) => "it",
            Earlier::All => "them",
        }
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
            let Some(task) = grantee.task() else {
                continue;
            };
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
                        problems.extend(key_problems(
                            grantee.subject(),
                            grant,
                            &catalog,
                            &memo.source.label(),
                        ));
                        problems.extend(late_file_problems(grantee.subject(), grant, &catalog));
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
        let Some(task) = req.grantee.task() else {
            bail!("prepare_spawn is for tasks; use prepare_exec");
        };
        let (_, mut problems) = grant_for_task(task);
        // judged by what the task env preparation already resolved: no script runs again
        let env_view = super::EnvView::resolved(req.rendered_defaults);
        problems.extend(static_problems(task, req.grant, req.denied, &env_view));
        if problems.is_empty() {
            problems.extend(super::grant::age_read_problems(task, req.grant).await);
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
        let memo = self.memo_for_task(config, req.ctx_builder, task).await?;
        let refs = env_view.template_refs(task);
        let spawn = self.grant_values_with(&memo, &req, &refs).await?;
        spawn.ensure_settable(&req.grantee.subject().text())?;
        Ok(Some(spawn))
    }

    /// `mise x --secrets` and `--secrets-all`. The source is the current project's, chosen
    /// the way `mise secrets ls` chooses it. `Ok(None)` when nothing is left to grant (for
    /// example every injectable key is a file secret).
    pub(crate) async fn prepare_exec(
        &self,
        config: &Arc<Config>,
        keys: &[String],
        all: bool,
        req: SpawnRequest<'_>,
    ) -> Result<Option<SpawnSecrets>> {
        let mut grant = exec_grant(keys)?;
        if grant.is_empty() && !all {
            return Ok(None);
        }
        if let Some(denied) = req.denied {
            bail!(
                "mise x --secrets: it was started by {}; mise secrets are not available there in this version",
                denied.launcher()
            );
        }
        let selection = super::config::select_for_cwd(config)?;
        let Some(selected) = selection.source else {
            let mut msg = "mise x --secrets needs a secrets source; add [secrets.fnox] to the project's mise.toml (https://mise.jdx.dev/environments/secrets/fnox.html)".to_string();
            for file in &selection.ignored {
                msg.push('\n');
                msg.push_str(&super::ignored_line(file));
            }
            bail!("{msg}");
        };
        let ts = config.get_toolset().await?;
        let opened = super::open_source(config, &selected, ts, None).await?;
        let memo_key = (opened.id().clone(), opened.build_fingerprint());
        let memo = self
            .sources
            .entry(memo_key)
            .or_insert_with(|| SourceMemo::new(opened))
            .clone();
        if all {
            let catalog = memo.catalog().await?;
            let subject = req.grantee.subject();
            let no_refs = BTreeSet::new();
            let split = split_exec_all(&catalog, |key| skip_reason(&req, &no_refs, key));
            for (key, reason) in &split.skipped {
                warn_skipped(subject, key.as_str(), *reason);
            }
            if !split.files.is_empty() {
                warn!(
                    "mise x --secrets-all: skipping file secrets {}",
                    split.files.join(", ")
                );
            }
            for key in split.keys {
                grant.keys.insert(key, vec![GrantOrigin::CliAll]);
            }
            if grant.is_empty() {
                return Ok(None);
            }
        }
        let subject_text = req.grantee.subject().text();
        let req = SpawnRequest {
            grant: &grant,
            ..req
        };
        let spawn = self
            .grant_values_with(&memo, &req, &BTreeSet::new())
            .await?;
        spawn.ensure_settable(&subject_text)?;
        Ok(Some(spawn))
    }

    /// Everything after the source is known: catalog checks, sandbox and collision checks,
    /// resolution, files, `remove`.
    #[cfg(test)]
    async fn grant_values(
        &self,
        memo: &Arc<SourceMemo>,
        req: &SpawnRequest<'_>,
    ) -> Result<SpawnSecrets> {
        let refs = req
            .grantee
            .task()
            .map(|t| super::EnvView::default().template_refs(t))
            .unwrap_or_default();
        self.grant_values_with(memo, req, &refs).await
    }

    /// `template_refs`: env names the task's templates read (empty for `mise x`).
    pub(crate) async fn grant_values_with(
        &self,
        memo: &Arc<SourceMemo>,
        req: &SpawnRequest<'_>,
        template_refs: &BTreeSet<String>,
    ) -> Result<SpawnSecrets> {
        let subject = req.grantee.subject();
        let who = subject.text();
        let catalog = memo.catalog().await?;
        let mut problems = key_problems(subject, req.grant, &catalog, &memo.source.label());
        // a key that only an env value references is read, never exported or written to a file
        for key in req.grant.exported_keys() {
            if let Some(entry) = catalog.entries.get(key)
                && entry.as_file
                && req.file_dir.is_none()
            {
                problems.push(file_problem(subject, key.as_str()));
            }
        }
        problems.extend(late_file_problems(subject, req.grant, &catalog));
        // the sandbox decides before anything is resolved. A composite is exported under its
        // own name, so that name is the one checked.
        for key in req
            .grant
            .exported_keys()
            .map(|k| k.as_str())
            .chain(req.grant.late.iter().map(|l| l.key.as_str()))
        {
            if !req.sandbox.keeps_env_key(key) {
                problems.push(sandbox_problem(subject, key));
            }
        }
        // a value inherited from the shell is allowed (the secret wins); a value mise itself
        // sets is not. The same goes for a key an env value builds, and a key cannot be both
        // exported and built.
        let mut colliding: BTreeSet<&str> = BTreeSet::new();
        for key in req.grant.exported_keys().map(|k| k.as_str()) {
            if req.grant.is_late_key(key) || collides_for(req, key) {
                colliding.insert(key);
            }
        }
        for late in &req.grant.late {
            if collides_for(req, &late.key) {
                colliding.insert(&late.key);
            }
        }
        for key in colliding {
            problems.push(collision_problem(subject, key));
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
        // every key to resolve, and the ones that are handed over under their own names
        let resolve_keys: BTreeSet<SecretName> = req.grant.keys.keys().cloned().collect();
        let keys: BTreeSet<SecretName> = req.grant.exported_keys().cloned().collect();
        let all = req.grant.all.is_some();
        // `--secrets-all`: keys that would collide or be dropped are skipped, not errors
        let refs = template_refs;
        let mut skipped: BTreeSet<SecretName> = BTreeSet::new();
        if all {
            for (key, entry) in &catalog.entries {
                if keys.contains(key)
                    || !entry.injectable
                    || mise_util::env::is_reserved_secret_name(key.as_str())
                {
                    continue;
                }
                if let Some(reason) = skip_reason(req, refs, key.as_str()) {
                    warn_skipped(subject, key.as_str(), reason);
                    skipped.insert(key.clone());
                }
            }
        }
        self.ensure_resolved(memo, &catalog, &resolve_keys, all, req, &who)
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
                debug!("{key} resolved to nothing; leaving it unset for {who}");
            }
        }
        if all {
            // Keys a dynamic lease produced are not in the catalog, so they are checked now.
            let mut consider = |key: &SecretName| -> bool {
                if keys.contains(key) || mise_util::env::is_reserved_secret_name(key.as_str()) {
                    return false;
                }
                if skipped.contains(key) {
                    return false;
                }
                if !catalog.entries.contains_key(key)
                    && let Some(reason) = skip_reason(req, refs, key.as_str())
                {
                    warn_skipped(subject, key.as_str(), reason);
                    skipped.insert(key.clone());
                    return false;
                }
                true
            };
            for (key, value) in &v.set {
                if consider(key) {
                    env_values.insert(key.to_string(), value.clone());
                }
            }
            for (key, value) in &v.files {
                if consider(key) && req.file_dir.is_some() {
                    file_values.insert(key.clone(), value.clone());
                }
            }
        }
        let composites = render_late(req.grant, &v, &who)?;
        for (key, value) in composites {
            env_values.insert(key.to_string(), value);
        }
        let mut remove = removable(&v.remove);
        drop(v);
        // never delete a key mise itself sets for this child, nor one this spawn sets
        remove.retain(|k| {
            !keys.iter().any(|set| env_key_eq(set.as_str(), k))
                && !env_values.keys().any(|set| env_key_eq(set, k))
                && !file_values.keys().any(|set| env_key_eq(set.as_str(), k))
                && !skipped.iter().any(|set| env_key_eq(set.as_str(), k))
                && !collides_for(req, k)
        });
        let (files, file_env) = match (file_values.is_empty(), req.file_dir) {
            (true, _) => (TempSecretFiles::default(), BTreeMap::new()),
            (false, Some(dir)) => TempSecretFiles::create(dir, &file_values)?,
            (false, None) => bail!("{who}: secret files are not supported here"),
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
        all: bool,
        req: &SpawnRequest<'_>,
        who: &str,
    ) -> Result<()> {
        let pending;
        {
            let mut v = memo.values.lock().await;
            let Some(missing) = v.pending_for(keys, all).map_err(|e| g15(who, &e))? else {
                return Ok(());
            };
            if !req.interactive {
                return v.resolve(memo, catalog, missing, false, who).await;
            }
            // a cache answers without a terminal or a process, so it is asked before the
            // terminal is taken; it never waits on a person, so the memo lock is fine
            if v.resolve_cached(memo, catalog, &missing, who).await? {
                return Ok(());
            }
            pending = missing;
        }
        let _terminal = req.terminal.acquire(&pending).await;
        let _pause = MultiProgressReport::try_get().map(|r| r.pause_progress());
        let mut v = memo.values.lock().await;
        let Some(missing) = v.pending_for(keys, all).map_err(|e| g15(who, &e))? else {
            return Ok(());
        };
        v.resolve(memo, catalog, missing, true, who).await
    }
}

/// The names from fnox's `remove` list that mise may delete from a child's environment: not
/// reserved, and ones the OS can unset (`env::remove_var` panics on "", "A=B" or NUL).
fn removable(names: &BTreeSet<String>) -> BTreeSet<String> {
    names
        .iter()
        .filter(|k| {
            !mise_util::env::is_reserved_secret_name(k) && !k.is_empty() && !k.contains(['=', '\0'])
        })
        .cloned()
        .collect()
}

/// What `mise x --secrets-all` sends to fnox by name.
struct ExecAll {
    keys: Vec<SecretName>,
    /// file secrets, which `mise x` cannot clean up after `exec` (C2)
    files: Vec<String>,
    /// keys that mise sets itself or whose sandbox drops them (C4)
    skipped: Vec<(SecretName, Skip)>,
}

/// Every injectable key except files, minus the keys `skip` rules out. It names the keys,
/// so it never includes keys only a dynamic lease would produce.
fn split_exec_all(catalog: &Catalog, skip: impl Fn(&str) -> Option<Skip>) -> ExecAll {
    let mut out = ExecAll {
        keys: vec![],
        files: vec![],
        skipped: vec![],
    };
    for (key, entry) in &catalog.entries {
        if !entry.injectable || mise_util::env::is_reserved_secret_name(key.as_str()) {
            continue;
        }
        if entry.as_file {
            out.files.push(key.to_string());
        } else if let Some(reason) = skip(key.as_str()) {
            out.skipped.push((key.clone(), reason));
        } else {
            out.keys.push(key.clone());
        }
    }
    out
}

/// Why `--secrets-all` leaves a key out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Skip {
    /// the task's run or env reads it through a template, which renders before secrets exist
    Template,
    Sandbox,
    MiseSets,
}

fn collides_for(req: &SpawnRequest<'_>, key: &str) -> bool {
    collides(
        key,
        req.task_env_keys,
        req.mise_set_inherited,
        req.mise_env_keys,
        req.base_env,
        &env::PRISTINE_ENV,
    )
}

/// `template_refs` are the env names the grantee's templates read (a task's run and env).
fn skip_reason(
    req: &SpawnRequest<'_>,
    template_refs: &BTreeSet<String>,
    key: &str,
) -> Option<Skip> {
    if template_refs.contains(key) {
        Some(Skip::Template)
    } else if req.grant.is_late_key(key) {
        // the task builds this name itself, so `--secrets-all` leaves it alone
        Some(Skip::MiseSets)
    } else if !req.sandbox.keeps_env_key(key) {
        Some(Skip::Sandbox)
    } else if collides_for(req, key) {
        Some(Skip::MiseSets)
    } else {
        None
    }
}

/// C4.
fn warn_skipped(subject: Subject<'_>, key: &str, reason: Skip) {
    warn!("{}", skipped_text(subject, key, reason));
}

fn skipped_text(subject: Subject<'_>, key: &str, reason: Skip) -> String {
    let why = match (reason, subject) {
        (Skip::Template, _) => {
            format!("{{{{ env.{key} }}}} in its run or env cannot see the secret")
        }
        (Skip::Sandbox, Subject::Task(_)) => "its sandbox denies env vars".to_string(),
        (Skip::Sandbox, Subject::Exec) => "the sandbox denies env vars".to_string(),
        (Skip::MiseSets, Subject::Task(_)) => "mise sets it for this task".to_string(),
        (Skip::MiseSets, Subject::Exec) => "mise sets it for this command".to_string(),
    };
    format!(
        "--secrets-all: not granting {key} to {}: {why}",
        subject.text()
    )
}

/// C1 for `mise x`; a task always has a directory for its files.
fn file_problem(subject: Subject<'_>, key: &str) -> Problem {
    let text = match subject {
        Subject::Exec => format!(
            "mise x: {key} is a file secret (as_file = true), which mise x cannot delete after it hands the process over. Use a task (mise run) or `fnox exec -- <command>`."
        ),
        Subject::Task(_) => format!(
            "{}: {key} is delivered as a file (as_file = true), which is not supported here",
            subject.text()
        ),
    };
    Problem::new(&subject.label(), Some(key), ProblemKind::Source, text)
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
        /// answers `resolve_cached` for named keys
        cached: bool,
        cached_calls: AtomicUsize,
        /// catalog keys with `as_file = true` that resolve to a file
        file_keys: BTreeSet<String>,
        /// catalog keys that resolve without error but are left out of the document
        omit: BTreeSet<String>,
    }

    fn catalog() -> Catalog {
        let mut entries = IndexMap::new();
        for k in [
            "A",
            "B",
            "C",
            "DEPLOY_KEY",
            "PATH_LIKE",
            "SHORT",
            "HIDDEN_KEY",
            "LEASE",
        ] {
            entries.insert(
                SecretName::new(k).unwrap(),
                CatalogEntry {
                    kind: KeyKind::Secret,
                    mode: None,
                    as_file: false,
                    injectable: k != "HIDDEN_KEY",
                    description: None,
                },
            );
        }
        Catalog {
            entries,
            profile: vec![],
            dynamic_leases: vec![],
            tool_version: "1".into(),
            cache: None,
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
            let mut catalog = catalog();
            for k in self.file_keys.iter().chain(self.omit.iter()) {
                catalog.entries.insert(
                    SecretName::new(k).unwrap(),
                    CatalogEntry {
                        kind: KeyKind::Secret,
                        mode: None,
                        as_file: self.file_keys.contains(k),
                        injectable: true,
                        description: None,
                    },
                );
            }
            Ok(catalog)
        }
        fn build_fingerprint(&self) -> String {
            self.fingerprint.clone()
        }
        async fn resolve_cached(
            &self,
            _cx: &SourceCx,
            keys: &KeySelection,
            _catalog: &Catalog,
        ) -> std::result::Result<Option<Resolved>, ResolveError> {
            self.cached_calls.fetch_add(1, Ordering::SeqCst);
            let Some(named) = keys.names().filter(|_| self.cached) else {
                return Ok(None);
            };
            let mut out = Resolved::default();
            for k in named {
                out.set
                    .insert(k.clone(), SecretValue::new(format!("{k}-cached-s3cr3t")));
            }
            Ok(Some(out))
        }
        async fn resolve(
            &self,
            _cx: &SourceCx,
            keys: &KeySelection,
            _catalog: &Catalog,
        ) -> std::result::Result<Resolved, ResolveError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            let named = keys.names().cloned().unwrap_or_default();
            self.asked
                .lock()
                .unwrap()
                .push(named.iter().map(|k| k.to_string()).collect());
            let all = matches!(keys, KeySelection::AllInScope);
            if all && self.fail.contains("*") {
                return Err(ResolveError::Resolution("not signed in".into()));
            }
            let rejected: Vec<String> = named
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
            if named.iter().any(|k| self.fail.contains(k.as_str())) {
                return Err(ResolveError::Resolution("not signed in".into()));
            }
            let mut out = Resolved::default();
            let asked: Vec<SecretName> = if all {
                // everything the catalog lists, plus what a stale or mis-scoped source adds
                [
                    "A",
                    "B",
                    "C",
                    "DEPLOY_KEY",
                    "SHORT",
                    "HIDDEN_KEY",
                    "EXTRA_KEY",
                ]
                .iter()
                .map(|k| SecretName::new(k).unwrap())
                .collect()
            } else {
                named.iter().cloned().collect()
            };
            for k in asked {
                if self.omit.contains(k.as_str()) {
                    continue;
                }
                if self.file_keys.contains(k.as_str()) {
                    out.files
                        .insert(k.clone(), SecretValue::new("file-s3cr3t-contents"));
                    continue;
                }
                let value = match k.as_str() {
                    "SHORT" => "ab".to_string(),
                    "DEPLOY_KEY" => "line-one-s3cr3t\nline-two-s3cr3t \"q\"".to_string(),
                    other => format!("{other}-value-s3cr3t"),
                };
                out.set.insert(k.clone(), SecretValue::new(value));
            }
            out.remove = BTreeSet::from(["SCRUB".to_string(), "A".to_string()]);
            // like a lease credential: fnox leaves it out of `remove` only when requested
            if !named.iter().any(|k| k.as_str() == "LEASE") {
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
            _pending: &Pending,
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
        rendered: BTreeSet<String>,
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
                rendered: BTreeSet::new(),
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
                rendered_defaults: &self.rendered,
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

    fn composed_inputs(template: &str) -> Inputs {
        use crate::config::env_directive::EnvDirective;
        let mut inputs = Inputs::new("migrate", &[]);
        inputs.task.env = crate::config::config_file::mise_toml::EnvList(vec![EnvDirective::Val(
            "PGURL".into(),
            template.into(),
            Default::default(),
        )]);
        inputs
            .task
            .record_late_secret_env(Path::new("/p/mise.toml"))
            .unwrap();
        inputs.grant = grant_for_task(&inputs.task).0;
        inputs
    }

    fn fake_with(file_keys: &[&str], omit: &[&str]) -> (Arc<Fake>, Arc<SourceMemo>) {
        let f = Arc::new(Fake {
            id: Some(SourceId {
                kind: "fake",
                root: PathBuf::from("/p"),
                profile: None,
            }),
            file_keys: file_keys.iter().map(|s| s.to_string()).collect(),
            omit: omit.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        });
        let memo = SourceMemo::new(f.clone());
        (f, memo)
    }

    #[tokio::test]
    async fn a_composite_is_exported_and_its_references_are_not() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake(&[]);
        let inputs = composed_inputs("p://{{ secrets.B }}@{{ secrets.C }}");
        let spawn = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
        assert_eq!(spawn.marker_value(), "PGURL");
        assert_eq!(
            spawn.env["PGURL"].expose(),
            "p://B-value-s3cr3t@C-value-s3cr3t"
        );
    }

    #[test]
    fn all_skips_a_name_the_task_builds_itself() {
        let (_, _memo) = fake(&[]);
        let mut inputs = composed_inputs("p://{{ secrets.B }}@h");
        inputs.grant.all = Some(GrantOrigin::CliAll);
        let term = terminal();
        let req = inputs.req(&term, false);
        assert!(matches!(
            skip_reason(&req, &BTreeSet::new(), "PGURL"),
            Some(Skip::MiseSets)
        ));
        assert!(skip_reason(&req, &BTreeSet::new(), "B").is_none());
    }

    #[tokio::test]
    async fn a_file_secret_cannot_be_composed() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (fake, memo) = fake_with(&["FILE_KEY"], &[]);
        let inputs = composed_inputs("p://{{ secrets.FILE_KEY }}@h");
        let err = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("task migrate: env.PGURL uses {{ secrets.FILE_KEY }}, which fnox delivers as a file (as_file = true); a file secret cannot be composed into an env value"),
            "{err}"
        );
        assert!(!err.contains("s3cr3t"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        // and a document that put it in files anyway is never read as a value
        let mut values = MemoValues::default();
        values.files.insert(
            SecretName::new("FILE_KEY").unwrap(),
            SecretValue::new("file-s3cr3t-contents"),
        );
        let err = render_late(&inputs.grant, &values, "task migrate")
            .unwrap_err()
            .to_string();
        assert!(err.contains("a file secret cannot be composed"), "{err}");
        assert!(!err.contains("s3cr3t"), "{err}");
    }

    #[tokio::test]
    async fn a_reference_the_source_leaves_out_fails_without_producing_values() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake_with(&[], &["OMITTED"]);
        let inputs = composed_inputs("p://{{ secrets.B }}@{{ secrets.OMITTED }}");
        let err = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("task migrate: env.PGURL: {{ secrets.OMITTED }} resolved to no value"),
            "{err}"
        );
        assert!(!err.contains("s3cr3t"), "{err}");
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
    async fn a_cache_hit_needs_no_terminal_and_no_cli_call() {
        let broker = SecretBroker::default();
        let f = Arc::new(Fake {
            id: Some(SourceId {
                kind: "fake",
                root: PathBuf::from("/p"),
                profile: None,
            }),
            cached: true,
            ..Default::default()
        });
        let memo = SourceMemo::new(f.clone());
        let term = terminal();
        let one = Inputs::new("one", &["A"]);
        let spawn = broker
            .grant_values(&memo, &one.req(&term, true))
            .await
            .unwrap();
        assert_eq!(term.acquired.load(Ordering::SeqCst), 0);
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.cached_calls.load(Ordering::SeqCst), 1);
        assert!(format!("{spawn:?}").contains('A'));
        // memoized: the cache is not asked again
        broker
            .grant_values(&memo, &one.req(&term, true))
            .await
            .unwrap();
        assert_eq!(f.cached_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_cache_miss_falls_through_to_the_terminal_and_the_cli() {
        let broker = SecretBroker::default();
        let (f, memo) = fake(&[]);
        let term = terminal();
        let one = Inputs::new("one", &["A"]);
        broker
            .grant_values(&memo, &one.req(&term, true))
            .await
            .unwrap();
        assert_eq!(f.cached_calls.load(Ordering::SeqCst), 1);
        assert_eq!(term.acquired.load(Ordering::SeqCst), 1);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_interactive_runs_never_ask_the_cache() {
        let broker = SecretBroker::default();
        let (f, memo) = fake(&[]);
        let term = terminal();
        let one = Inputs::new("one", &["A"]);
        broker
            .grant_values(&memo, &one.req(&term, false))
            .await
            .unwrap();
        assert_eq!(f.cached_calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
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

    fn with_all(mut inputs: Inputs) -> Inputs {
        inputs.grant.all = Some(GrantOrigin::CliAll);
        inputs
    }

    #[tokio::test]
    async fn all_in_scope_asks_once_without_keys_and_filters_the_document() {
        let (fake, memo) = fake(&[]);
        let broker = SecretBroker::default();
        let term = terminal();
        let one = with_all(Inputs::new("a", &[]));
        let two = with_all(Inputs::new("b", &[]));
        let (req1, req2) = (one.req(&term, false), two.req(&term, false));
        let (r1, r2) = tokio::join!(
            broker.grant_values(&memo, &req1),
            broker.grant_values(&memo, &req2)
        );
        let (s1, s2) = (r1.unwrap(), r2.unwrap());
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        assert!(fake.asked.lock().unwrap()[0].is_empty());
        // HIDDEN_KEY is not injectable and EXTRA_KEY is not in the catalog: neither comes back
        assert_eq!(s1.marker_value(), "A,B,C,DEPLOY_KEY,SHORT");
        assert_eq!(s2.marker_value(), "A,B,C,DEPLOY_KEY,SHORT");
        // later requests for named keys are answered from the memo, with no call
        let three = Inputs::new("c", &["B", "C"]);
        let s3 = broker
            .grant_values(&memo, &three.req(&term, false))
            .await
            .unwrap();
        assert_eq!(s3.marker_value(), "B,C");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn all_in_scope_failure_is_not_retried() {
        let (fake, memo) = fake(&["*"]);
        let broker = SecretBroker::default();
        let term = terminal();
        let one = with_all(Inputs::new("a", &[]));
        let err = broker
            .grant_values(&memo, &one.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("fnox could not resolve all secrets"), "{err}");
        let two = with_all(Inputs::new("b", &[]));
        let err = broker
            .grant_values(&memo, &two.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("task b: not retrying the secrets"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn named_keys_after_a_failed_all_in_scope_call_are_g15() {
        let (fake, memo) = fake(&["*"]);
        let broker = SecretBroker::default();
        let term = terminal();
        let one = with_all(Inputs::new("a", &[]));
        let _ = broker
            .grant_values(&memo, &one.req(&term, false))
            .await
            .unwrap_err();
        let two = Inputs::new("b", &["B"]);
        let err = broker
            .grant_values(&memo, &two.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(
                "task b: not retrying the secrets; fnox failed to resolve them earlier in this run (see above)"
            ),
            "{err}"
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn keys_resolved_before_a_failed_all_in_scope_call_stay_usable() {
        let (fake, memo) = fake(&["*"]);
        let broker = SecretBroker::default();
        let term = terminal();
        let first = Inputs::new("dep", &["B"]);
        broker
            .grant_values(&memo, &first.req(&term, false))
            .await
            .unwrap();
        let all = with_all(Inputs::new("a", &[]));
        let _ = broker
            .grant_values(&memo, &all.req(&term, false))
            .await
            .unwrap_err();
        // B is already memoized: no call, no error
        let again = Inputs::new("later", &["B"]);
        let spawn = broker
            .grant_values(&memo, &again.req(&term, false))
            .await
            .unwrap();
        assert_eq!(spawn.marker_value(), "B");
        // C would need a new call
        let other = Inputs::new("other", &["C"]);
        let err = broker
            .grant_values(&memo, &other.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not retrying the secrets"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn all_in_scope_skips_keys_the_templates_read() {
        let (_, memo) = fake(&[]);
        let broker = SecretBroker::default();
        let term = terminal();
        let mut inputs = with_all(Inputs::new("pay", &[]));
        inputs.task.run = vec![crate::task::RunEntry::Script("echo {{ env.A }}".into())];
        let spawn = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
        assert_eq!(spawn.marker_value(), "B,C,DEPLOY_KEY,SHORT");
    }

    #[tokio::test]
    async fn all_in_scope_after_a_key_failure_is_g15() {
        let (fake, memo) = fake(&["B"]);
        let broker = SecretBroker::default();
        let term = terminal();
        let one = Inputs::new("one", &["B"]);
        let _ = broker
            .grant_values(&memo, &one.req(&term, false))
            .await
            .unwrap_err();
        let two = with_all(Inputs::new("two", &[]));
        let err = broker
            .grant_values(&memo, &two.req(&term, false))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("task two: not retrying B"), "{err}");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn all_in_scope_skips_keys_mise_sets_or_the_sandbox_drops() {
        let (_, memo) = fake(&[]);
        let broker = SecretBroker::default();
        let term = terminal();
        let mut inputs = with_all(Inputs::new("deploy", &[]));
        inputs.base.insert("A".into(), "from-mise".into());
        inputs.task_env.insert("C".into());
        let spawn = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
        assert_eq!(spawn.marker_value(), "B,DEPLOY_KEY,SHORT");
        // the skipped keys are never taken out of the child's environment either
        assert!(!spawn.remove.contains("A"));

        let mut inputs = with_all(Inputs::new("deploy", &[]));
        inputs.sandbox.deny_env = true;
        inputs.sandbox.allow_env = vec!["SHORT".into()];
        let spawn = broker
            .grant_values(&memo, &inputs.req(&term, false))
            .await
            .unwrap();
        assert_eq!(spawn.marker_value(), "SHORT");
    }

    #[tokio::test]
    async fn exec_messages_name_mise_x() {
        let broker = SecretBroker::default();
        let term = terminal();
        let (_, memo) = fake(&[]);
        let inputs = Inputs::new("ignored", &["A", "NOPE"]);
        let mut req = inputs.req(&term, false);
        req.grantee = Grantee::Exec;
        let err = broker
            .grant_values(&memo, &req)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("mise x: unknown secret NOPE"), "{err}");

        let mut inputs = Inputs::new("ignored", &["A"]);
        inputs.base.insert("A".into(), "from-mise".into());
        inputs.sandbox.deny_env = true;
        let mut req = inputs.req(&term, false);
        req.grantee = Grantee::Exec;
        let err = broker
            .grant_values(&memo, &req)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(
                "mise x is granted A, but its sandbox denies env vars; pass --allow-env A"
            ),
            "{err}"
        );
        assert!(!err.contains("add allow_env"), "{err}");
        inputs.sandbox.deny_env = false;
        let mut req = inputs.req(&term, false);
        req.grantee = Grantee::Exec;
        let err = broker
            .grant_values(&memo, &req)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("mise x: A is both a secret and a mise env var"),
            "{err}"
        );
        assert!(err.contains("for this command"), "{err}");
    }

    #[test]
    fn remove_drops_names_the_os_cannot_unset() {
        let names = ["", "A=B", "SCRUB", "NUL\0X", "PATH"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(removable(&names), BTreeSet::from(["SCRUB".to_string()]));
    }

    #[test]
    fn exec_all_names_injectable_keys_and_splits_off_files_and_collisions() {
        let mut catalog = catalog();
        for (k, as_file, injectable) in [
            ("GCP_SA_JSON", true, true),
            ("SIGNING_KEY", false, false),
            ("PATH", false, true),
        ] {
            catalog.entries.insert(
                SecretName::new(k).unwrap(),
                CatalogEntry {
                    kind: KeyKind::Secret,
                    mode: None,
                    as_file,
                    injectable,
                    description: None,
                },
            );
        }
        let split = split_exec_all(&catalog, |k| match k {
            "A" => Some(Skip::MiseSets),
            "C" => Some(Skip::Sandbox),
            _ => None,
        });
        let names = |v: &[SecretName]| v.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(
            names(&split.keys),
            ["B", "DEPLOY_KEY", "PATH_LIKE", "SHORT", "LEASE"]
        );
        assert_eq!(split.files, ["GCP_SA_JSON"]);
        let text = |s, k, r| skipped_text(s, k, r);
        assert_eq!(
            text(Subject::Exec, "A", Skip::MiseSets),
            "--secrets-all: not granting A to mise x: mise sets it for this command"
        );
        assert_eq!(
            text(Subject::Exec, "C", Skip::Sandbox),
            "--secrets-all: not granting C to mise x: the sandbox denies env vars"
        );
        assert_eq!(
            text(Subject::Task("pay"), "A", Skip::MiseSets),
            "--secrets-all: not granting A to task pay: mise sets it for this task"
        );
        assert_eq!(
            text(Subject::Task("pay"), "C", Skip::Sandbox),
            "--secrets-all: not granting C to task pay: its sandbox denies env vars"
        );
        assert_eq!(
            text(Subject::Task("pay"), "STRIPE_KEY", Skip::Template),
            "--secrets-all: not granting STRIPE_KEY to task pay: {{ env.STRIPE_KEY }} in its run or env cannot see the secret"
        );
        assert_eq!(
            split
                .skipped
                .iter()
                .map(|(k, r)| (k.to_string(), *r))
                .collect::<Vec<_>>(),
            [
                ("A".to_string(), Skip::MiseSets),
                ("C".to_string(), Skip::Sandbox)
            ]
        );
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
