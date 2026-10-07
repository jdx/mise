//! fnox `env --json` schema 1. No `deny_unknown_fields`: fnox may add fields.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Head {
    pub(super) schema: u32,
    #[serde(default)]
    pub(super) fnox_version: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct DescribeDocument {
    #[serde(default)]
    pub(super) fnox_version: String,
    #[serde(default)]
    pub(super) profile: Vec<String>,
    #[serde(default)]
    pub(super) keys: Vec<WireKey>,
    #[serde(default)]
    pub(super) dynamic_leases: Vec<String>,
    /// fnox's own decision about its daemon; absent from an older fnox
    #[serde(default)]
    pub(super) daemon_enabled: Option<bool>,
}

#[derive(Deserialize)]
pub(super) struct WireKey {
    pub(super) key: String,
    #[serde(default)]
    pub(super) kind: String,
    /// `true`, `"exec"` or `false` for secrets; absent for leases
    #[serde(default)]
    pub(super) env: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) as_file: bool,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) lease: Option<String>,
    #[serde(default)]
    pub(super) injectable: WireInjectable,
}

#[derive(Deserialize, Default)]
pub(super) struct WireInjectable {
    #[serde(default)]
    pub(super) exec: bool,
}

#[derive(Deserialize)]
pub(super) struct ErrorDocument {
    pub(super) error: WireError,
}

#[derive(Deserialize)]
pub(super) struct WireError {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) message: String,
    /// `invalid_keys` only
    #[serde(default)]
    pub(super) unknown: Vec<String>,
    #[serde(default)]
    pub(super) suggestions: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(super) not_injectable: Vec<WireNotInjectable>,
}

#[derive(Deserialize)]
pub(super) struct WireNotInjectable {
    pub(super) key: String,
}
