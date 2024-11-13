use std::fmt::{Display, Formatter};

#[derive(
    Debug,
    PartialEq,
    Eq,
    Hash,
    Clone,
    Copy,
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
    Go,
    Npm,
    Pipx,
    Spm,
    Ubi,
    Vfox,
    Unknown,
}

impl Display for BackendType {
    fn fmt(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "{}", format!("{:?}", self).to_lowercase())
    }
}

impl BackendType {
    pub fn guess(s: &str) -> BackendType {
        let s = s.split(':').next().unwrap_or(s);
        let s = s.split('-').next().unwrap_or(s);
        match s {
            "aqua" => BackendType::Aqua,
            "asdf" => BackendType::Asdf,
            "cargo" => BackendType::Cargo,
            "core" => BackendType::Core,
            "go" => BackendType::Go,
            "npm" => BackendType::Npm,
            "pipx" => BackendType::Pipx,
            "spm" => BackendType::Spm,
            "ubi" => BackendType::Ubi,
            "vfox" => BackendType::Vfox,
            _ => BackendType::Unknown,
        }
    }
}
