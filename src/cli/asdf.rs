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
            Some("reshim") => {
                if config.settings.experimental && config.settings.shims_dir.is_some() {
                    // only reshim if experimental is enabled and shims_dir is set
                    // otherwise it would error
                    Cli::new().run(config, &args, out)
                } else {
                    Ok(())
                }
            }
            Some("list") => list_versions(config, out, &args),
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

fn list_versions(mut config: Config, out: &mut Output, args: &Vec<String>) -> Result<()> {
    if args[2] == "all" {
        let mut new_args: Vec<String> = vec!["rtx".into(), "ls-remote".into()];
        if args.len() >= 3 {
            new_args.push(args[3].clone());
        }
        return Cli::new().run(config, &new_args, out);
    }
    let ts = ToolsetBuilder::new().build(&mut config)?;
    let mut versions = ts.list_installed_versions(&config)?;
    let plugin = match args.len() {
        3 => Some(&args[2]),
        _ => None,
    };
    if let Some(plugin) = plugin {
        versions.retain(|(_, v)| v.plugin_name.as_str() == plugin);
        for (_, version) in versions {
            rtxprintln!(out, "{}", version.version);
        }
    } else {
        for (plugin, versions) in &versions
            .into_iter()
            .group_by(|(_, v)| v.plugin_name.to_string())
        {
            rtxprintln!(out, "{}", plugin);
            for (_, tv) in versions {
                rtxprintln!(out, "  {}", tv.version);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_fake_asdf_list() {
        assert_cli!("asdf", "install", "tiny");
        assert_cli_snapshot!("asdf", "list", "tiny");
    }

    #[test]
    fn test_fake_asdf_other() {
        assert_cli_snapshot!("asdf", "current", "tiny");
    }

    #[test]
    fn test_fake_asdf_reshim() {
        assert_cli_snapshot!("asdf", "reshim");
    }

    #[test]
    fn test_fake_asdf_install() {
        assert_cli_snapshot!("asdf", "install", "tiny");
    }
}
