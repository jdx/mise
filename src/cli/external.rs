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
        .map(|p| match p.external_commands() {
            Ok(commands) => commands,
            Err(e) => {
                warn!(
                    "failed to load external commands for plugin {}: {}",
                    p.name, e
                );
                vec![]
            }
        })
        .collect::<Vec<Vec<Vec<String>>>>()
        .into_iter()
        .filter(|commands| !commands.is_empty())
        .filter(|commands| commands[0][0] != "direnv")
        .map(|commands| {
            clap::Command::new(commands[0][0].to_string()).subcommands(commands.into_iter().map(
                |cmd| {
                    clap::Command::new(cmd[1..].join("-")).arg(
                        clap::Arg::new("args")
                            .num_args(1..)
                            .allow_hyphen_values(true)
                            .trailing_var_arg(true),
                    )
                },
            ))
        })
        .collect()
}

pub fn execute(
    config: &Config,
    plugin: &str,
    args: &ArgMatches,
    external_commands: Vec<Command>,
) -> Result<()> {
    if let Some(_cmd) = external_commands.iter().find(|c| c.get_name() == plugin) {
        if let Some((subcommand, matches)) = args.subcommand() {
            let plugin = config.plugins.get(&plugin.to_string()).unwrap();
            let args: Vec<String> = matches
                .get_raw("args")
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect();
            plugin.execute_external_command(subcommand, args)?;
        }
    }

    Ok(())
}
