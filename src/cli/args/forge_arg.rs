use std::fmt::Display;
use std::str::FromStr;

use crate::forge::unalias_forge;
use crate::forge::ForgeType;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ForgeArg {
    pub name: String,
    pub forge_type: ForgeType,
}

impl FromStr for ForgeArg {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (forge_type, plugin) = s.split_once(':').unwrap_or(("external", s));
        let name = unalias_forge(plugin).to_string();
        Ok(Self {
            name,
            forge_type: ForgeType::try_from(forge_type)?,
        })
    }
}

impl Display for ForgeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}
