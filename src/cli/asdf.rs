use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::cli::command::Command;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// [internal] simulates asdf for plugins that call "asdf" internally
#[derive(Debug, clap::Args)]
#[clap(hide = true, verbatim_doc_comment)]
pub struct Asdf {
    /// all arguments
    #[clap(allow_hyphen_values = true)]
    args: Vec<String>,
}

impl Command for Asdf {
    fn run(mut self, config: Config, out: &mut Output) -> Result<()> {
        let mut args = vec![String::from("rtx")];
        args.append(&mut self.args);

        match args.get(1).map(|s| s.as_str()) {
            Some("reshim") => Ok(()),
            Some("list") => list_versions(&config, out, &args),
            Some("install") => {
                if args.len() == 4 {
                    let version = args.pop().unwrap();
                    args[2] = format!("{}@{}", args[2], version);
                }
                Cli::new().run(config, &args, out)
            }
            _ => Cli::new().run(config, &args, out),
        }
    }
}

fn list_versions(config: &Config, out: &mut Output, args: &Vec<String>) -> Result<()> {
    let ts = ToolsetBuilder::new().build(config);
    let mut versions = ts.list_installed_versions()?;
    let plugin = match args.len() {
        3 => Some(&args[2]),
        _ => None,
    };
    if let Some(plugin) = plugin {
        versions.retain(|v| &v.plugin.name == plugin);
        for version in versions {
            rtxprintln!(out, "{}", version.version);
        }
    } else {
        for (plugin, versions) in &versions.into_iter().group_by(|v| v.plugin.name.clone()) {
            rtxprintln!(out, "{}", plugin);
            for version in versions {
                rtxprintln!(out, "  {}", version.version);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::cli::version::VERSION;
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_fake_asdf() {
        let stdout = assert_cli!("asdf", "version");
        assert_str_eq!(stdout, VERSION.to_string() + "\n");
    }

    #[test]
    fn test_fake_asdf_list() {
        assert_cli!("asdf", "install", "tiny");
        assert_cli_snapshot!("asdf", "list", "tiny");
    }
}
