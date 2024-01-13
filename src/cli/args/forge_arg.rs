use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::str::FromStr;

use crate::forge;
use crate::forge::unalias_forge;
use crate::forge::{AForge, ForgeType};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ForgeArg {
    pub name: String,
    pub forge_type: ForgeType,
}

impl FromStr for ForgeArg {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (forge_type, name) = s.split_once(':').unwrap_or(("asdf", s));
        let forge_type = forge_type.parse()?;
        Ok(Self::new(forge_type, name))
    }
}

impl ForgeArg {
    pub fn new(forge_type: ForgeType, name: &str) -> Self {
        let name = unalias_forge(name).to_string();
        Self { name, forge_type }
    }
    pub fn get_forge(&self) -> AForge {
        forge::get(self)
    }
    pub fn pathname(&self) -> String {
        match self.forge_type {
            ForgeType::Asdf => self.name.to_string(),
            forge_type => format!("{}-{}", forge_type.as_ref(), self.name),
        }
    }
}

impl Display for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.forge_type {
            ForgeType::Asdf => write!(f, "{}", self.name),
            ft => write!(f, "{}:{}", ft.as_ref(), self.name),
        }
    }
}

impl Debug for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.forge_type {
            ForgeType::Asdf => write!(f, r#"ForgeArg("{}")"#, self.name),
            ft => write!(f, r#"ForgeArg("{}:{}")"#, ft.as_ref(), self.name),
        }
    }
}
