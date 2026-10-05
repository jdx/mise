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
    /// Identifies the executable and environment this source was built with.
    fn build_fingerprint(&self) -> String;
}

pub(crate) struct SourceCx {
    pub(crate) interactive: bool,
}

pub(crate) enum KeySelection {
    Keys(BTreeSet<SecretName>),
}

impl KeySelection {
    pub(crate) fn keys(&self) -> &BTreeSet<SecretName> {
        let Self::Keys(keys) = self;
        keys
    }
}

/// What a source hands back. `Debug` lists key names only.
#[derive(Default)]
pub(crate) struct Resolved {
    pub(crate) set: BTreeMap<SecretName, SecretValue>,
    pub(crate) files: BTreeMap<SecretName, SecretValue>,
    pub(crate) remove: BTreeSet<String>,
    pub(crate) missing: BTreeSet<SecretName>,
    /// Keys the source returned that were not requested (G17); dropped.
    pub(crate) unrequested: BTreeSet<String>,
    /// Requested keys the source returned although it marks them not injectable (G19); dropped.
    pub(crate) not_injectable: BTreeSet<SecretName>,
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
