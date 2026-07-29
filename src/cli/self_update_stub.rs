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

/// Shown when mise cannot update itself and the packager shipped no instructions
/// file. Self-update is always unavailable in this build, so without the hint
/// telling the user their mise is out of date is a dead end. Kept neutral about
/// how mise was installed: a build without the `self_update` feature is usually
/// a distro package, but it can equally be a local `--no-default-features` build.
pub const SELF_UPDATE_DISABLED_HINT: &str =
    "self-update is disabled for this install, update mise the same way you installed it";

/// How to update mise: the packager's instructions when they shipped some,
/// otherwise the generic hint.
pub fn upgrade_instructions_or_hint() -> String {
    upgrade_instructions_text().unwrap_or_else(|| SELF_UPDATE_DISABLED_HINT.to_string())
}

pub fn append_self_update_instructions(mut message: String) -> String {
    message.push('\n');
    message.push_str(&upgrade_instructions_or_hint());
    message
}
