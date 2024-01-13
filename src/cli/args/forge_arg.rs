use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::str::FromStr;

use crate::forge::unalias_forge;
use crate::forge::ForgeType;

#[derive(Clone, PartialOrd, Ord)]
pub struct ForgeArg {
    pub id: String,
    pub name: String,
    pub forge_type: ForgeType,
}

impl FromStr for ForgeArg {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((forge_type, name)) = s.split_once('-') {
            if let Ok(forge_type) = forge_type.parse() {
                return Ok(Self::new(forge_type, name));
            }
        }
        Ok(Self::new(ForgeType::Asdf, s))
    }
}

impl ForgeArg {
    pub fn new(forge_type: ForgeType, name: &str) -> Self {
        let name = unalias_forge(name).to_string();
        let id = match forge_type {
            ForgeType::Asdf => name.clone(),
            forge_type => format!("{}-{}", forge_type.as_ref(), name),
        };
        Self {
            name,
            forge_type,
            id,
        }
    }
}

impl Display for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl Debug for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"ForgeArg("{}")"#, self.id)
    }
}

impl PartialEq for ForgeArg {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ForgeArg {}

impl Hash for ForgeArg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
