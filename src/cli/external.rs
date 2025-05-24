use clap::Command;
use eyre::Result;
use std::collections::HashMap;
use std::sync::LazyLock as Lazy;

use crate::backend;
use crate::cli::args::BackendArg;

pub static COMMANDS: Lazy<HashMap<String, Command>> = Lazy::new(|| {
    backend::list()
        .into_iter()
        .flat_map(|b| {
            if let Some(p) = b.plugin() {
                return p.external_commands().unwrap_or_else(|e| {
                    let p = p.name();
                    warn!("failed to load external commands for plugin {p}: {e:#}");
                    vec![]
                });
            }
            vec![]
        })
        .map(|cmd| (cmd.get_name().to_string(), cmd))
        .collect()
});

pub fn execute(ba: &BackendArg, mut cmd: Command, args: Vec<String>) -> Result<()> {
    if let Some(subcommand) = cmd.find_subcommand(&args[0]) {
        let backend = ba.backend()?;
        if let Some(p) = backend.plugin() {
            p.execute_external_command(subcommand.get_name(), args)?;
        }
    } else {
        cmd.print_help().unwrap();
    }

    Ok(())
}
