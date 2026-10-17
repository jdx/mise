/// The host scope is part of a preference's identity.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HostScope {
    #[default]
    Any,
    Current,
}
