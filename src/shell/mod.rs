use std::fmt::{Display, Formatter};
use std::path::Path;

use crate::env;

mod bash;
mod fish;
mod zsh;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ShellType {
    Bash,
    Fish,
    Zsh,
}

impl ShellType {
    pub fn load() -> Option<ShellType> {
        let shell = env::var("SHELL").ok()?;
        if shell.ends_with("bash") {
            Some(ShellType::Bash)
        } else if shell.ends_with("fish") {
            Some(ShellType::Fish)
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
            Self::Zsh => write!(f, "zsh"),
        }
    }
}

pub trait Shell {
    fn activate(&self, exe: &Path) -> String;
    fn deactivate(&self) -> String;
    fn set_env(&self, k: &str, v: &str) -> String;
    fn unset_env(&self, k: &str) -> String;
}

pub fn get_shell(shell: Option<ShellType>) -> Box<dyn Shell> {
    match shell.or_else(ShellType::load) {
        Some(ShellType::Bash) => Box::<bash::Bash>::default(),
        Some(ShellType::Zsh) => Box::<zsh::Zsh>::default(),
        Some(ShellType::Fish) => Box::<fish::Fish>::default(),
        _ => panic!("no shell provided, use `--shell=zsh`"),
    }
}

pub fn is_dir_in_path(dir: &Path) -> bool {
    let dir = dir.canonicalize().unwrap_or(dir.to_path_buf());
    env::PATH
        .clone()
        .into_iter()
        .any(|p| p.canonicalize().unwrap_or(p) == dir)
}
