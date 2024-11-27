use clap::ValueHint::CommandWithArguments;
use eyre::Result;
use itertools::Itertools;

use crate::cli::ls_remote::LsRemote;
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
    pub fn run(mut self) -> Result<()> {
        let config = Config::try_get()?;
        let mut args = vec![String::from("mise")];
        args.append(&mut self.args);

        match args.get(1).map(|s| s.as_str()) {
            Some("reshim") => Cli::run(&args),
            Some("list") => list_versions(&config, &args),
            Some("install") => {
                if args.len() == 4 {
                    let version = args.pop().unwrap();
                    args[2] = format!("{}@{}", args[2], version);
                }
                Cli::run(&args)
            }
            _ => Cli::run(&args),
        }
    }
}

fn list_versions(config: &Config, args: &[String]) -> Result<()> {
    if args[2] == "all" {
        return LsRemote {
            prefix: None,
            all: false,
            plugin: args.get(3).map(|s| s.parse()).transpose()?,
        }
        .run();
    }
    let ts = ToolsetBuilder::new().build(config)?;
    let mut versions = ts.list_installed_versions()?;
    let plugin = match args.len() {
        3 => Some(&args[2]),
        _ => None,
    };
    if let Some(plugin) = plugin {
        versions.retain(|(_, v)| &v.ba().to_string() == plugin);
        for (_, version) in versions {
            miseprintln!("{}", version.version);
        }
    } else {
        for (plugin, versions) in &versions.into_iter().chunk_by(|(_, v)| v.ba().clone()) {
            miseprintln!("{}", plugin);
            for (_, tv) in versions {
                miseprintln!("  {}", tv.version);
            }
        }
    }

    Ok(())
}
