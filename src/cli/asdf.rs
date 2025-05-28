use std::sync::Arc;

use clap::ValueHint::CommandWithArguments;
use eyre::Result;
use itertools::Itertools;

use crate::cli::Cli;
use crate::cli::ls_remote::LsRemote;
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
    pub async fn run(mut self) -> Result<()> {
        let config = Config::get().await?;
        let mut args = vec![String::from("mise")];
        args.append(&mut self.args);

        match args.get(1).map(|s| s.as_str()) {
            Some("reshim") => Box::pin(Cli::run(&args)).await,
            Some("list") => list_versions(&config, &args).await,
            Some("install") => {
                if args.len() == 4 {
                    let version = args.pop().unwrap();
                    args[2] = format!("{}@{}", args[2], version);
                }
                Box::pin(Cli::run(&args)).await
            }
            _ => Box::pin(Cli::run(&args)).await,
        }
    }
}

async fn list_versions(config: &Arc<Config>, args: &[String]) -> Result<()> {
    if args[2] == "all" {
        return LsRemote {
            prefix: None,
            all: false,
            plugin: args.get(3).map(|s| s.parse()).transpose()?,
        }
        .run()
        .await;
    }
    let ts = ToolsetBuilder::new().build(config).await?;
    let mut versions = ts.list_installed_versions(config).await?;
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
