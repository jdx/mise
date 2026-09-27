//! Declarative desired states shared by bootstrap resource adapters.

use serde::Serialize;

/// Desired state of a local bootstrap account.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    #[default]
    Present,
    Absent,
}

/// Desired state of a managed bootstrap file or directory.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedState {
    #[default]
    Present,
    Absent,
}

/// When to apply a managed file relative to package installation.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedFilePhase {
    PrePackages,
    #[default]
    PostPackages,
}

impl ManagedFilePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrePackages => "pre-packages",
            Self::PostPackages => "post-packages",
        }
    }
}
