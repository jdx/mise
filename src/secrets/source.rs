use std::path::PathBuf;

use indexmap::IndexMap;

use super::SecretName;

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
}
