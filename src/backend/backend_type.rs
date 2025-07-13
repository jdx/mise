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
    Core,
    Dotnet,
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

        // Handle vfox-backend prefix for backend plugins
        if prefix == "vfox-backend" {
            // For vfox-backend:plugin-name format, we need to extract the plugin name from the full string
            if let Some((_, plugin_name)) = s.split_once(':') {
                return BackendType::VfoxBackend(plugin_name.to_string());
            } else {
                // If no colon is found, this is not a valid vfox-backend format
                return BackendType::Unknown;
            }
        }

        let s = prefix.split('-').next().unwrap_or(prefix);
        match s {
            "aqua" => BackendType::Aqua,
            "asdf" => BackendType::Asdf,
            "cargo" => BackendType::Cargo,
            "core" => BackendType::Core,
            "dotnet" => BackendType::Dotnet,
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
}
