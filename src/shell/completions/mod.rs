use std::collections::HashSet;

use clap::Command;
use once_cell::sync::Lazy;

mod zsh_complete;

pub fn zsh_complete(cmd: &Command) -> eyre::Result<String> {
    let output = zsh_complete::render(cmd);
    // let result = cmd!("shfmt", "-s").stdin_bytes(output).read()?;
    Ok(output)
}

static BANNED_COMMANDS: Lazy<HashSet<&str>> = Lazy::new(|| ["render-mangen", "render-help"].into());

pub fn is_banned(cmd: &Command) -> bool {
    BANNED_COMMANDS.contains(&cmd.get_name())
}
