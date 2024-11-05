use serde::{Deserialize, Serialize};

use crate::cli::args::BackendArg;
use crate::{dirs, file};

use super::BackendType;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BackendMeta {
    pub short: String,
    pub id: String,
    pub name: String,
    pub backend_type: String,
}

pub const FORGE_META_FILENAME: &str = ".mise.backend.json";

impl BackendMeta {
    pub fn read(dirname: &str) -> BackendMeta {
        let meta_path = &dirs::INSTALLS.join(dirname).join(FORGE_META_FILENAME);
        if meta_path.exists() {
            let json = file::read_to_string(meta_path).unwrap_or_default();
            if let Ok(meta) = serde_json::from_str(&json) {
                return meta;
            }
        }
        Self::default_meta(dirname)
    }

    pub fn write(fa: &BackendArg) -> eyre::Result<()> {
        if fa.backend_type == BackendType::Asdf {
            return Ok(());
        }
        let meta = BackendMeta {
            short: fa.short.clone(),
            id: fa.full.clone(),
            name: fa.name.clone(),
            backend_type: fa.backend_type.as_ref().to_string(),
        };

        let json = serde_json::to_string(&meta).expect("Could not encode JSON value");
        let meta_path = fa.installs_path.join(FORGE_META_FILENAME);
        file::write(meta_path, json)?;
        Ok(())
    }

    // Returns a BackendMeta derived from the dirname for backends without a meta file
    fn default_meta(dirname: &str) -> BackendMeta {
        let id = dirname.replacen('-', ":", 1);
        match id.split_once(':') {
            Some((backend_type, name)) => {
                let name = Self::name_for_type(name.to_string(), backend_type);
                let id = format!("{}:{}", backend_type, name);
                BackendMeta {
                    short: id.clone(),
                    id,
                    name,
                    backend_type: backend_type.to_string(),
                }
            }
            None => BackendMeta {
                short: id.clone(),
                id: id.to_string(),
                name: id.to_string(),
                backend_type: BackendType::Asdf.as_ref().to_string(),
            },
        }
    }

    // TODO: remove this when backends come out of experimental
    fn name_for_type(name: String, backend_type: &str) -> String {
        match backend_type {
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
