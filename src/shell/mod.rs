use crate::env;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::str::FromStr;

mod bash;
mod elvish;
mod fish;
mod nushell;
mod pwsh;
mod xonsh;
mod zsh;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ShellType {
    Bash,
    Elvish,
    Fish,
    Nu,
    Xonsh,
    Zsh,
    Pwsh,
}

impl ShellType {
    pub fn load() -> Option<ShellType> {
        env::var("MISE_SHELL")
            .or(env::var("SHELL"))
            .ok()?
            .parse()
            .ok()
    }

    pub fn as_shell(&self) -> Box<dyn Shell> {
        match self {
            Self::Bash => Box::<bash::Bash>::default(),
            Self::Elvish => Box::<elvish::Elvish>::default(),
            Self::Fish => Box::<fish::Fish>::default(),
            Self::Nu => Box::<nushell::Nushell>::default(),
            Self::Xonsh => Box::<xonsh::Xonsh>::default(),
            Self::Zsh => Box::<zsh::Zsh>::default(),
            Self::Pwsh => Box::<pwsh::Pwsh>::default(),
        }
    }
}

impl Display for ShellType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bash => write!(f, "bash"),
            Self::Elvish => write!(f, "elvish"),
            Self::Fish => write!(f, "fish"),
            Self::Nu => write!(f, "nu"),
            Self::Xonsh => write!(f, "xonsh"),
            Self::Zsh => write!(f, "zsh"),
            Self::Pwsh => write!(f, "pwsh"),
        }
    }
}

impl FromStr for ShellType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        let s = s.rsplit_once('/').map(|(_, s)| s).unwrap_or(&s);
        match s {
            "bash" | "sh" => Ok(Self::Bash),
            "elvish" => Ok(Self::Elvish),
            "fish" => Ok(Self::Fish),
            "nu" => Ok(Self::Nu),
            "xonsh" => Ok(Self::Xonsh),
            "zsh" => Ok(Self::Zsh),
            "pwsh" => Ok(Self::Pwsh),
            _ => Err(format!("unsupported shell type: {s}")),
        }
    }
}

pub trait Shell: Display {
    fn activate(&self, exe: &Path, flags: String) -> String;
    fn deactivate(&self) -> String;
    fn set_env(&self, k: &str, v: &str) -> String;
    fn prepend_env(&self, k: &str, v: &str) -> String;
    fn unset_env(&self, k: &str) -> String;
}

pub fn get_shell(shell: Option<ShellType>) -> Option<Box<dyn Shell>> {
    shell.or_else(ShellType::load).map(|st| st.as_shell())
}
