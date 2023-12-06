use clap::{ArgMatches, Command};
use color_eyre::eyre::Result;
use itertools::Itertools;
use rayon::prelude::*;

use crate::config::Config;

pub fn commands(config: &Config) -> Vec<Command> {
    config
        .plugins
        .values()
        .collect_vec()
        .into_par_iter()
        .flat_map(|p| match p.external_commands() {
            Ok(commands) => commands,
            Err(e) => {
                warn!(
                    "failed to load external commands for plugin {}: {:#}",
                    p.name(),
                    e
                );
                vec![]
            }
        })
        .collect()
}

pub fn execute(
    config: &Config,
    plugin: &str,
    args: &ArgMatches,
    external_commands: Vec<Command>,
) -> Result<()> {
    if let Some(mut cmd) = external_commands
        .into_iter()
        .find(|c| c.get_name() == plugin)
    {
        if let Some((subcommand, matches)) = args.subcommand() {
            let plugin = config.plugins.get(&plugin.to_string()).unwrap();
            let args: Vec<String> = matches
                .get_raw("args")
                .unwrap_or_default()
                .map(|s| s.to_string_lossy().to_string())
                .collect();
            plugin.execute_external_command(config, subcommand, args)?;
        } else {
            cmd.print_help().unwrap();
        }
    }

    Ok(())
}
