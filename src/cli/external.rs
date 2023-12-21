use clap::{ArgMatches, Command};
use eyre::Result;
use rayon::prelude::*;

use crate::config::Config;

pub fn commands(config: &Config) -> Vec<Command> {
    config
        .list_plugins()
        .into_par_iter()
        .flat_map(|p| {
            p.external_commands().unwrap_or_else(|e| {
                let p = p.name();
                rtxwarn!("failed to load external commands for plugin {p}: {e:#}");
                vec![]
            })
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
            let plugin = config.get_or_create_plugin(plugin);
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
