use eyre::Result;
use std::collections::HashMap;
use std::sync::LazyLock as Lazy;

use crate::backend;
use crate::cli::args::BackendArg;

pub static COMMANDS: Lazy<HashMap<String, crate::plugins::ExternalCommand>> = Lazy::new(|| {
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
        .map(|cmd| (cmd.topic.clone(), cmd))
        .collect()
});

pub fn execute(
    ba: &BackendArg,
    cmd: crate::plugins::ExternalCommand,
    args: Vec<String>,
) -> Result<()> {
    if let Some(subcommand) = args
        .first()
        .filter(|name| cmd.subcommands.contains(name))
        .cloned()
    {
        let backend = ba.backend()?;
        if let Some(p) = backend.plugin() {
            p.execute_external_command(&subcommand, args)?;
        }
    } else {
        eprintln!("Commands provided by {} plugin:", cmd.topic);
        for subcommand in cmd.subcommands {
            eprintln!("  {subcommand}");
        }
    }

    Ok(())
}
