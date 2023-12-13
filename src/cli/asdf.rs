use clap::ValueHint::CommandWithArguments;
use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::cli::Cli;
use crate::config::Config;

use crate::toolset::ToolsetBuilder;

/// [internal] simulates asdf for plugins that call "asdf" internally
#[derive(Debug, clap::Args)]
#[clap(hide = true, verbatim_doc_comment)]
pub struct Asdf {
    /// all arguments
    #[clap(allow_hyphen_values = true, value_hint = CommandWithArguments, trailing_var_arg = true)]
    args: Vec<String>,
}

impl Asdf {
    pub fn run(mut self, config: Config) -> Result<()> {
        let mut args = vec![String::from("rtx")];
        args.append(&mut self.args);

        match args.get(1).map(|s| s.as_str()) {
            Some("reshim") => Cli::new().run(config, &args),
            Some("list") => list_versions(config, &args),
            Some("install") => {
                if args.len() == 4 {
                    let version = args.pop().unwrap();
                    args[2] = format!("{}@{}", args[2], version);
                }
                Cli::new().run(config, &args)
            }
            _ => Cli::new().run(config, &args),
        }
    }
}

fn list_versions(config: Config, args: &Vec<String>) -> Result<()> {
    if args[2] == "all" {
        let mut new_args: Vec<String> = vec!["rtx".into(), "ls-remote".into()];
        if args.len() >= 3 {
            new_args.push(args[3].clone());
        }
        return Cli::new().run(config, &new_args);
    }
    let ts = ToolsetBuilder::new().build(&config)?;
    let mut versions = ts.list_installed_versions(&config)?;
    let plugin = match args.len() {
        3 => Some(&args[2]),
        _ => None,
    };
    if let Some(plugin) = plugin {
        versions.retain(|(_, v)| v.plugin_name.as_str() == plugin);
        for (_, version) in versions {
            rtxprintln!("{}", version.version);
        }
    } else {
        for (plugin, versions) in &versions
            .into_iter()
            .group_by(|(_, v)| v.plugin_name.to_string())
        {
            rtxprintln!("{}", plugin);
            for (_, tv) in versions {
                rtxprintln!("  {}", tv.version);
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
        assert_cli!("install", "tiny@1", "tiny@2");
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
