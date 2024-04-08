use std::fmt::{Display, Formatter};
use std::path::Path;

use crate::env;

mod bash;
mod fish;
mod nushell;
mod xonsh;
mod zsh;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ShellType {
    Bash,
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
}

impl Display for ShellType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bash => write!(f, "bash"),
            Self::Fish => write!(f, "fish"),
            Self::Nu => write!(f, "nu"),
            Self::Xonsh => write!(f, "xonsh"),
            Self::Zsh => write!(f, "zsh"),
        }
    }
}

pub trait Shell {
    fn activate(&self, exe: &Path, flags: String) -> String;
    fn deactivate(&self) -> String;
    fn set_env(&self, k: &str, v: &str) -> String;
    fn prepend_env(&self, k: &str, v: &str) -> String;
    fn unset_env(&self, k: &str) -> String;
}

pub fn get_shell(shell: Option<ShellType>) -> Option<Box<dyn Shell>> {
    match shell.or_else(ShellType::load) {
        Some(ShellType::Bash) => Some(Box::<bash::Bash>::default()),
        Some(ShellType::Fish) => Some(Box::<fish::Fish>::default()),
        Some(ShellType::Nu) => Some(Box::<nushell::Nushell>::default()),
        Some(ShellType::Xonsh) => Some(Box::<xonsh::Xonsh>::default()),
        Some(ShellType::Zsh) => Some(Box::<zsh::Zsh>::default()),
        _ => None,
    }
}
