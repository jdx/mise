use clap::builder::Resettable;
use eyre::Result;

use crate::cli;

/// Generate a usage CLI spec
///
/// See https://usage.jdx.dev for more information
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Usage {}

impl Usage {
    pub fn run(self) -> Result<()> {
        let cli = cli::Cli::command().version(Resettable::Reset);
        let mut spec: usage::Spec = cli.into();
        let run = spec.cmd.subcommands.get_mut("run").unwrap();
        run.args = vec![];
        run.mounts.push(usage::SpecMount {
            run: "mise tasks --usage".to_string(),
        });

        let extra = include_str!("../assets/mise-extra.usage.kdl");
        println!("{spec}\n{extra}");
        Ok(())
    }
}
