use serde::{Deserialize, Serialize};

use crate::{cli::args::ForgeArg, dirs, file};

use super::ForgeType;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ForgeMeta {
    pub id: String,
    pub name: String,
    pub forge_type: String,
}

pub const FORGE_META_FILENAME: &str = ".mise.forge.json";

impl ForgeMeta {
    pub fn read(dirname: &str) -> ForgeMeta {
        let meta_path = &dirs::INSTALLS.join(dirname).join(FORGE_META_FILENAME);
        let json = file::read_to_string(meta_path).unwrap_or_default();
        serde_json::from_str(&json).unwrap_or(Self::default_meta(dirname))
    }

    pub fn write(fa: &ForgeArg) -> eyre::Result<()> {
        if fa.forge_type == ForgeType::Asdf {
            return Ok(());
        }
        let meta = ForgeMeta {
            id: fa.id.clone(),
            name: fa.name.clone(),
            forge_type: fa.forge_type.as_ref().to_string(),
        };

        let json = serde_json::to_string(&meta).expect("Could not encode JSON value");
        let meta_path = fa.installs_path.join(FORGE_META_FILENAME);
        file::write(meta_path, json)?;
        Ok(())
    }

    // Returns a ForgeMeta derived from the dirname for forges without a meta file
    fn default_meta(dirname: &str) -> ForgeMeta {
        let id = dirname.replacen('-', ":", 1);
        match id.split_once(':') {
            Some((forge_type, name)) => {
                let name = Self::name_for_type(name.to_string(), forge_type);
                let id = format!("{}:{}", forge_type, name);
                ForgeMeta {
                    id,
                    name,
                    forge_type: forge_type.to_string(),
                }
            }
            None => ForgeMeta {
                id: id.to_string(),
                name: id.to_string(),
                forge_type: ForgeType::Asdf.as_ref().to_string(),
            },
        }
    }

    // TODO: remove this when backends come out of experimental
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
