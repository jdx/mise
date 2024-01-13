use crate::cli::args::ForgeArg;
use clap::{ArgMatches, Command};
use eyre::Result;
use rayon::prelude::*;

use crate::forge;

pub fn commands() -> Vec<Command> {
    forge::list()
        .into_par_iter()
        .flat_map(|p| {
            p.external_commands().unwrap_or_else(|e| {
                let p = p.id();
                warn!("failed to load external commands for plugin {p}: {e:#}");
                vec![]
            })
        })
        .collect()
}

pub fn execute(fa: &ForgeArg, args: &ArgMatches) -> Result<()> {
    if let Some(mut cmd) = commands()
        .into_iter()
        .find(|c| c.get_name() == fa.to_string())
    {
        if let Some((subcommand, matches)) = args.subcommand() {
            let plugin = forge::get(fa);
            let args: Vec<String> = matches
                .get_raw("args")
                .unwrap_or_default()
                .map(|s| s.to_string_lossy().to_string())
                .collect();
            plugin.execute_external_command(subcommand, args)?;
        } else {
            cmd.print_help().unwrap();
        }
    }

    Ok(())
}
