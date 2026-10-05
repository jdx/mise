use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use indexmap::IndexMap;

use super::{SecretName, SecretValue};

/// What a source allows for a key, from its `env` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectMode {
    /// `env = true`
    Shell,
    /// `env = "exec"`
    Exec,
    /// `env = false`, or a value this mise does not know
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Secret,
    Lease { name: String },
}

#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub kind: KeyKind,
    /// `None` for lease-only keys
    pub mode: Option<InjectMode>,
    pub as_file: bool,
    /// The source's `injectable.exec`
    pub injectable: bool,
    /// ASCII control characters stripped
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    pub entries: IndexMap<SecretName, CatalogEntry>,
    pub profile: Vec<String>,
    pub dynamic_leases: Vec<String>,
    pub tool_version: String,
    /// The source's own cache (fnox: its daemon) is enabled for this project and env; `None`
    /// when the source did not say.
    pub cache: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SourceId {
    pub(crate) kind: &'static str,
    /// canonicalized
    pub(crate) root: PathBuf,
    pub(crate) profile: Option<String>,
}

#[async_trait::async_trait]
pub(crate) trait SecretSource: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &SourceId;
    /// e.g. "fnox (profile dev) in ~/src/app"
    fn label(&self) -> String;
    /// Names and metadata only: never resolves, prompts, contacts or starts a daemon.
    async fn describe(&self) -> eyre::Result<Catalog>;
    /// Resolves exactly the selected keys. `interactive` lets the source prompt on the
    /// terminal (it is given the stdin that mise has).
    async fn resolve(
        &self,
        cx: &SourceCx,
        keys: &KeySelection,
        catalog: &Catalog,
    ) -> Result<Resolved, ResolveError>;
    /// Answers from a cache without prompting or spawning a process. `Ok(None)` means not
    /// available: use `resolve`. Only called for an interactive run.
    async fn resolve_cached(
        &self,
        _cx: &SourceCx,
        _keys: &KeySelection,
        _catalog: &Catalog,
    ) -> Result<Option<Resolved>, ResolveError> {
        Ok(None)
    }
    /// One line for `mise secrets ls` about the source's cache, if it has one. Never starts
    /// anything.
    async fn daemon_status(&self, _catalog: &Catalog) -> Option<String> {
        None
    }
    /// Identifies the executable and environment this source was built with.
    fn build_fingerprint(&self) -> String;
}

pub(crate) struct SourceCx {
    pub(crate) interactive: bool,
}

pub(crate) enum KeySelection {
    Keys(BTreeSet<SecretName>),
    /// Everything the source injects, asked for without naming keys, so keys that only exist
    /// once a dynamic lease runs come back too. Files are allowed.
    AllInScope,
}

impl KeySelection {
    /// The keys asked for by name; `None` for `AllInScope`.
    pub(crate) fn names(&self) -> Option<&BTreeSet<SecretName>> {
        match self {
            Self::Keys(keys) => Some(keys),
            Self::AllInScope => None,
        }
    }
}

/// What a source hands back. `Debug` lists key names only.
#[derive(Default)]
pub(crate) struct Resolved {
    pub(crate) set: BTreeMap<SecretName, SecretValue>,
    pub(crate) files: BTreeMap<SecretName, SecretValue>,
    pub(crate) remove: BTreeSet<String>,
    pub(crate) missing: BTreeSet<SecretName>,
    /// Leases the source ran for this document.
    pub(crate) leases: BTreeSet<String>,
    /// Keys the source returned that were not requested (G17); dropped.
    pub(crate) unrequested: BTreeSet<String>,
    /// Requested keys the source returned although it marks them not injectable (G19); dropped.
    pub(crate) not_injectable: BTreeSet<SecretName>,
}

impl Resolved {
    /// What an `AllInScope` document may contribute, whoever produced it: never a key the
    /// catalog marks not injectable (G19), and a key the catalog does not list only when a
    /// dynamic lease the catalog knows ran, since that lease is what produced it.
    pub(crate) fn filter_for_all(mut self, catalog: &Catalog) -> Self {
        let lease_ran = self
            .leases
            .iter()
            .any(|lease| catalog.dynamic_leases.contains(lease));
        let mut unlisted = BTreeSet::new();
        let mut hidden = BTreeSet::new();
        let mut keep = |map: &mut BTreeMap<SecretName, SecretValue>| {
            map.retain(|name, _| match catalog.entries.get(name) {
                Some(entry) if !entry.injectable => {
                    hidden.insert(name.clone());
                    false
                }
                Some(_) => true,
                None if lease_ran => true,
                None => {
                    unlisted.insert(name.to_string());
                    false
                }
            });
        };
        keep(&mut self.set);
        keep(&mut self.files);
        self.not_injectable.extend(hidden);
        self.unrequested.extend(unlisted);
        self
    }
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved")
            .field("set", &self.set.keys().collect::<Vec<_>>())
            .field("files", &self.files.keys().collect::<Vec<_>>())
            .field("remove", &self.remove)
            .field("missing", &self.missing)
            .finish()
    }
}

/// Why a resolve produced no values.
#[derive(Debug)]
pub(crate) enum ResolveError {
    /// The source no longer knows the keys it described (config edited since preflight): G1/G2.
    Invalid {
        unknown: Vec<String>,
        suggestions: BTreeMap<String, Vec<String>>,
        not_injectable: Vec<String>,
    },
    /// The provider failed (G14). Carries the source's own message.
    Resolution(String),
    /// Anything else, already worded for the user (S3, S5, S6, ...).
    Other(String),
}
