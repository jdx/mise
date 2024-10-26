use clap::{ArgMatches, Command};
use eyre::Result;
use rayon::prelude::*;

use crate::backend;
use crate::cli::args::BackendArg;

pub fn commands() -> Vec<Command> {
    time!("commands start");

    let commands = backend::list()
        .into_par_iter()
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
        .collect();
    time!("commands done");

    commands
}

pub fn execute(fa: &BackendArg, args: &ArgMatches) -> Result<()> {
    if let Some(mut cmd) = commands()
        .into_iter()
        .find(|c| c.get_name() == fa.to_string())
    {
        if let Some((subcommand, matches)) = args.subcommand() {
            let backend = backend::get(fa);
            let args: Vec<String> = matches
                .get_raw("args")
                .unwrap_or_default()
                .map(|s| s.to_string_lossy().to_string())
                .collect();
            if let Some(p) = backend.plugin() {
                p.execute_external_command(subcommand, args)?;
            }
        } else {
            cmd.print_help().unwrap();
        }
    }

    Ok(())
}
