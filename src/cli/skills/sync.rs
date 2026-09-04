use std::path::PathBuf;

use eyre::Result;

use crate::config::Config;
use crate::dirs;

/// Link the active tools' skills where an agent looks for them
///
/// Writes one symlink per skill into DIR, named after the skill and pointing
/// at the directory of the version that is active here. DIR defaults to
/// `.claude/skills` under the project root: the directory of the nearest mise
/// config. Run it again after `mise use` changes a version and the links follow.
///
/// Only links mise made, which point into its installs directory, are ever
/// replaced or, with --prune, removed. A real directory or a link of your own at
/// a skill's name is left alone and reported.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, effect = "write")]
pub(super) struct SkillsSync {
    /// The directory to link skills into
    #[usage(long, value_hint = usage_rs::ValueHint::DirPath)]
    dir: Option<PathBuf>,

    /// Link into ~/.claude/skills instead of the project's directory
    #[usage(long, short, conflicts = "dir")]
    global: bool,

    /// Remove links mise made for skills that are no longer active
    #[usage(long)]
    prune: bool,
}

impl SkillsSync {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let skills = crate::packslip::active_skills(&config).await?;
        let dir = match (&self.dir, self.global) {
            (Some(dir), _) => dir.clone(),
            (None, true) => dirs::HOME.join(".claude").join("skills"),
            (None, false) => config
                .project_root
                .clone()
                .map(Ok)
                .unwrap_or_else(std::env::current_dir)?
                .join(".claude")
                .join("skills"),
        };
        if skills.is_empty() && !self.prune {
            miseprintln!("no skills declared by the active tools; nothing to link");
            return Ok(());
        }
        let report = crate::packslip::sync_skills(&dir, &skills, &dirs::INSTALLS, self.prune)?;
        for name in &report.linked {
            let skill = skills.iter().find(|s| &s.name == name);
            miseprintln!(
                "linked {} -> {}",
                dir.join(name).display(),
                skill
                    .map(|s| s.path.display().to_string())
                    .unwrap_or_default()
            );
        }
        for name in &report.pruned {
            miseprintln!("removed {}", dir.join(name).display());
        }
        for (name, why) in &report.skipped {
            warn!("skipped {name}: {why}");
        }
        if report.linked.is_empty() && report.pruned.is_empty() {
            miseprintln!(
                "{} skill(s) already linked in {}",
                report.unchanged.len(),
                dir.display()
            );
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # into the project's .claude/skills
    $ <bold>mise skills sync</bold>

    # somewhere else, and drop links for skills that are no longer active
    $ <bold>mise skills sync --dir .agents/skills --prune</bold>
"#
);
