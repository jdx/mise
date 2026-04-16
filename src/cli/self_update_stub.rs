use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::env;

#[derive(Debug, Default, clap::Args)]
pub struct SelfUpdate {
    /// Update to a specific version
    version: Option<String>,

    /// Update even if already up to date
    #[clap(long, short)]
    force: bool,

    /// Skip confirmation prompt
    #[clap(long, short)]
    yes: bool,

    /// Disable auto-updating plugins
    #[clap(long)]
    no_plugins: bool,
}

impl SelfUpdate {
    pub async fn run(self) -> crate::Result<()> {
        if let Some(instructions) = upgrade_instructions_text() {
            warn!("{}", instructions);
        }
        eyre::bail!("mise's self-update feature has been disabled at build time, cannot update");
    }
    pub fn is_available() -> bool {
        false
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct InstructionsToml {
    message: Option<String>,
    #[serde(flatten)]
    commands: BTreeMap<String, String>,
}

fn read_instructions_file(path: &PathBuf) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let parsed: InstructionsToml = toml::from_str(&body).ok()?;
    if let Some(msg) = parsed.message {
        return Some(msg);
    }
    if let Some((_k, v)) = parsed.commands.into_iter().next() {
        return Some(v);
    }
    None
}

pub fn upgrade_instructions_text() -> Option<String> {
    if let Some(path) = &*env::MISE_SELF_UPDATE_INSTRUCTIONS {
        if let Some(msg) = read_instructions_file(path) {
            return Some(msg);
        }
    }
    None
}

pub fn append_self_update_instructions(mut message: String) -> String {
    if let Some(instructions) = upgrade_instructions_text() {
        message.push('\n');
        message.push_str(&instructions);
    }
    message
}
