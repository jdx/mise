use clap::{ArgMatches, Command};
use miette::Result;
use rayon::prelude::*;

use crate::config::Config;

pub fn commands(config: &Config) -> Vec<Command> {
    config
        .list_plugins()
        .into_par_iter()
        .flat_map(|p| {
            p.external_commands().unwrap_or_else(|e| {
                let p = p.name();
                warn!("failed to load external commands for plugin {p}: {e:#}");
                vec![]
            })
        })
        .collect()
}

pub fn execute(plugin: &str, args: &ArgMatches) -> Result<()> {
    let config = Config::try_get()?;
    if let Some(mut cmd) = commands(&config)
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
