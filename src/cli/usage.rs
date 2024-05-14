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
        let spec: usage::Spec = cli.into();
        let extra = include_str!("../assets/mise-extra.usage.kdl");
        println!("{spec}\n{extra}");
        Ok(())
    }
}
