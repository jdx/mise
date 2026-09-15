use eyre::Result;

mod ls;
mod sync;

/// Agent skills the active tools ship, from their packslips
///
/// A tool installed with the `packslip:` backend may declare an agent skill: a
/// directory holding `SKILL.md` and whatever it references, in the Agent Skills
/// format. mise knows which version of each tool is active here, so it can hand
/// an agent the skill for exactly that version.
#[derive(Debug, usage_rs::Args)]
#[usage(alias = "skill", verbatim_doc_comment)]
pub(crate) struct Skills {
    #[usage(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Ls(ls::SkillsLs),
    Sync(sync::SkillsSync),
}

impl Commands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run().await,
            Self::Sync(cmd) => cmd.run().await,
        }
    }
}

impl Skills {
    pub(crate) async fn run(self) -> Result<()> {
        self.command
            .unwrap_or(Commands::Ls(ls::SkillsLs::default()))
            .run()
            .await
    }
}
