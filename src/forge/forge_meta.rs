use serde::{Deserialize, Serialize};

use crate::{cli::args::ForgeArg, dirs, file};

use super::ForgeType;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ForgeMeta {
    pub id: String,
    pub name: String,
}

const FORGE_META_FILENAME: &str = ".mise.forge.toml";

impl ForgeMeta {
    pub fn read(dirname: &str) -> ForgeMeta {
        let meta_path = &dirs::INSTALLS.join(dirname).join(FORGE_META_FILENAME);
        let toml = file::read_to_string(meta_path).unwrap_or_default();
        toml::from_str(&toml).unwrap_or(Self::default_meta(dirname))
    }

    pub fn write(fa: &ForgeArg) -> eyre::Result<()> {
        if fa.forge_type == ForgeType::Asdf {
            return Ok(());
        }
        let meta = ForgeMeta {
            id: fa.id.clone(),
            name: fa.name.clone(),
        };
        let toml = toml::to_string(&meta).expect("Could not encode TOML value");
        let meta_path = fa.installs_path.join(FORGE_META_FILENAME);
        file::write(meta_path, toml)?;
        Ok(())
    }

    // Returns a ForgeMeta with id and name derived from the dirname for backends without a .mise.forge.toml file
    fn default_meta(dirname: &str) -> ForgeMeta {
        let id = dirname.replacen('-', ":", 1);
        match id.split_once(':') {
            Some((forge_type, name)) => {
                let name = Self::name_for_type(name.to_string(), forge_type);
                let id = format!("{}:{}", forge_type, name);
                ForgeMeta { id, name }
            }
            None => ForgeMeta {
                id: id.to_string(),
                name: id.to_string(),
            },
        }
    }

    fn name_for_type(name: String, forge_type: &str) -> String {
        match forge_type {
            "go" => name.replace('-', "/"),
            "npm" => {
                if name.contains('@') {
                    return name.replacen('-', "/", 1).to_string();
                }
                name.to_string()
            }
            _ => name.to_string(),
        }
    }
}
