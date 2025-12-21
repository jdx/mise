use std::fmt::{Display, Formatter};

#[derive(
    Debug,
    PartialEq,
    Eq,
    Hash,
    Clone,
    strum::EnumString,
    strum::EnumIter,
    strum::AsRefStr,
    Ord,
    PartialOrd,
)]
#[strum(serialize_all = "snake_case")]
pub enum BackendType {
    Aqua,
    Asdf,
    Cargo,
    Conda,
    Core,
    Dotnet,
    Forgejo,
    Gem,
    Github,
    Gitlab,
    Go,
    Npm,
    Pipx,
    Spm,
    Http,
    Ubi,
    Vfox,
    VfoxBackend(String),
    Unknown,
}

impl Display for BackendType {
    fn fmt(&self, formatter: &mut Formatter) -> std::fmt::Result {
        match self {
            BackendType::VfoxBackend(plugin_name) => write!(formatter, "{plugin_name}"),
            _ => write!(formatter, "{}", format!("{self:?}").to_lowercase()),
        }
    }
}

impl BackendType {
    pub fn guess(s: &str) -> BackendType {
        let prefix = s.split(':').next().unwrap_or(s);

        match prefix {
            "aqua" => BackendType::Aqua,
            "asdf" => BackendType::Asdf,
            "cargo" => BackendType::Cargo,
            "conda" => BackendType::Conda,
            "core" => BackendType::Core,
            "dotnet" => BackendType::Dotnet,
            "forgejo" => BackendType::Forgejo,
            "gem" => BackendType::Gem,
            "github" => BackendType::Github,
            "gitlab" => BackendType::Gitlab,
            "go" => BackendType::Go,
            "npm" => BackendType::Npm,
            "pipx" => BackendType::Pipx,
            "spm" => BackendType::Spm,
            "http" => BackendType::Http,
            "ubi" => BackendType::Ubi,
            "vfox" => BackendType::Vfox,
            _ => BackendType::Unknown,
        }
    }

    /// Returns true if this backend requires experimental mode to be enabled
    pub fn is_experimental(&self) -> bool {
        use super::{conda, dotnet, spm};
        match self {
            BackendType::Conda => conda::EXPERIMENTAL,
            BackendType::Spm => spm::EXPERIMENTAL,
            BackendType::Dotnet => dotnet::EXPERIMENTAL,
            _ => false,
        }
    }
}
