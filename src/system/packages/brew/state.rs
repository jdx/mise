//! Ledger of what mise itself installed into the Homebrew prefix, so
//! upgrades/uninstalls never touch kegs the user installed by other means.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::result::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub pkg_version: String,
    pub on_request: bool,
    pub installed_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub kegs: BTreeMap<String, LedgerEntry>,
}

fn ledger_path() -> PathBuf {
    crate::dirs::STATE.join("system").join("brew.json")
}

impl Ledger {
    pub fn load() -> Self {
        let path = ledger_path();
        if let Ok(content) = crate::file::read_to_string(&path)
            && let Ok(ledger) = serde_json::from_str(&content)
        {
            return ledger;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = ledger_path();
        let parent = path.parent().unwrap();
        crate::file::create_dir_all(parent)?;
        // atomic write so an interrupted save can't corrupt the ledger
        let tmp = parent.join("brew.json.tmp");
        crate::file::write(&tmp, serde_json::to_string_pretty(self)?)?;
        crate::file::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn record(&mut self, name: &str, pkg_version: &str, on_request: bool) {
        let on_request = on_request || self.kegs.get(name).map(|e| e.on_request).unwrap_or(false);
        self.kegs.insert(
            name.to_string(),
            LedgerEntry {
                pkg_version: pkg_version.to_string(),
                on_request,
                installed_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            },
        );
    }

    pub fn remove(&mut self, name: &str) {
        self.kegs.remove(name);
    }
}
