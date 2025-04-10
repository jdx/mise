use crate::cli::Cli;
use clap::CommandFactory;
use clap::builder::Resettable;
use eyre::Result;

/// Generate a usage CLI spec
///
/// See https://usage.jdx.dev for more information on this specification.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct Usage {}

impl Usage {
    pub fn run(self) -> Result<()> {
        let cli = Cli::command().version(Resettable::Reset);
        let mut spec: usage::Spec = cli.into();
        let run = spec.cmd.subcommands.get_mut("run").unwrap();
        run.args = vec![];
        run.mounts.push(usage::SpecMount {
            run: "mise tasks --usage".to_string(),
        });

        let tasks = spec.cmd.subcommands.get_mut("tasks").unwrap();
        let tasks_run = tasks.subcommands.get_mut("run").unwrap();
        tasks_run.mounts.push(usage::SpecMount {
            run: "mise tasks --usage".to_string(),
        });

        let min_version = r#"min_usage_version "1.3""#;
        let extra = include_str!("../assets/mise-extra.usage.kdl").trim();
        println!("{min_version}\n{}\n{extra}", spec.to_string().trim());
        Ok(())
    }
}
