use eyre::Result;
use std::collections::HashMap;
use std::sync::LazyLock as Lazy;

use crate::backend;
use crate::cli::args::BackendArg;

pub(super) static COMMANDS: Lazy<HashMap<String, crate::plugins::ExternalCommand>> =
    Lazy::new(|| {
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

fn match_subcommand(subcommands: &[String], args: &[String]) -> Option<String> {
    (1..=args.len()).rev().find_map(|word_count| {
        let flattened = args[..word_count].join("-");
        subcommands.contains(&flattened).then_some(flattened)
    })
}

pub(super) fn execute(
    ba: &BackendArg,
    cmd: crate::plugins::ExternalCommand,
    args: Vec<String>,
) -> Result<()> {
    let subcommand = match_subcommand(&cmd.subcommands, &args);
    if let Some(subcommand) = subcommand {
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

#[cfg(test)]
mod tests {
    #[test]
    fn nested_words_match_a_flattened_asdf_command() {
        let args = ["foo".to_string(), "bar".to_string(), "--flag".to_string()];
        let commands = ["foo".to_string(), "foo-bar".to_string()];
        let matched = super::match_subcommand(&commands, &args);
        assert_eq!(matched.as_deref(), Some("foo-bar"));
    }
}
