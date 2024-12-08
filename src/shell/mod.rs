use std::fmt::{Display, Formatter};
use std::path::Path;

use crate::env;

mod bash;
mod elvish;
mod fish;
mod nushell;
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
}

impl ShellType {
    pub fn load() -> Option<ShellType> {
        let shell = env::var("MISE_SHELL").or(env::var("SHELL")).ok()?;
        if shell.ends_with("bash") {
            Some(ShellType::Bash)
        } else if shell.ends_with("elvish") {
            Some(ShellType::Elvish)
        } else if shell.ends_with("fish") {
            Some(ShellType::Fish)
        } else if shell.ends_with("nu") {
            Some(ShellType::Nu)
        } else if shell.ends_with("xonsh") {
            Some(ShellType::Xonsh)
        } else if shell.ends_with("zsh") {
            Some(ShellType::Zsh)
        } else {
            None
        }
    }

    pub fn as_shell(&self) -> Box<dyn Shell> {
        match self {
            Self::Bash => Box::<bash::Bash>::default(),
            Self::Elvish => Box::<elvish::Elvish>::default(),
            Self::Fish => Box::<fish::Fish>::default(),
            Self::Nu => Box::<nushell::Nushell>::default(),
            Self::Xonsh => Box::<xonsh::Xonsh>::default(),
            Self::Zsh => Box::<zsh::Zsh>::default(),
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
