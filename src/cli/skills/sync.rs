use std::path::PathBuf;

use eyre::Result;

use crate::config::{Config, Settings};
use crate::dirs;

/// Link the active tools' skills where an agent looks for them
///
/// Writes one symlink per skill into DIR, named after the skill and pointing
/// at the directory of the version that is active here. DIR defaults to the
/// `skills.dir` setting under the project root: the directory of the nearest
/// mise config. Unset, that is `.agents/skills` if it exists, then
/// `.claude/skills` if it exists; with neither, mise creates `.agents/skills`
/// and a `.claude/skills` symlink to it. Run it again after `mise use` changes
/// a version and the links follow, or set `skills.auto_sync` to have mise do
/// that after every install and `mise use`.
///
/// mise replaces only the links it made, which point into its installs
/// directory, and removes them only with --prune or the `skills.prune` setting.
/// A real directory or a link of your own at a skill's name is left alone and
/// reported.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise skills sync"###,
        help = r###"Link skills into the project's skills.dir (default .agents/skills)"###
    ),
    example(
        r###"mise skills sync --dir .agents/skills --prune"###,
        help = r###"Link into .agents/skills and remove links for skills that are no longer active"###
    ),
    effect = "write"
)]
pub(super) struct SkillsSync {
    /// The directory to link skills into
    #[usage(long, value_hint = usage_rs::ValueHint::DirPath)]
    dir: Option<PathBuf>,

    /// Link into the skills.dir directory under your home directory instead of the project's
    ///
    /// With no skills.dir set, that follows the same rule as in a project.
    #[usage(long, short, conflicts = "dir")]
    global: bool,

    /// Remove links mise made for skills that are no longer active
    #[usage(long)]
    prune: bool,
}

impl SkillsSync {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let target = match (&self.dir, self.global) {
            (Some(dir), _) => crate::packslip::SkillsTarget::at(dir.clone()),
            (None, true) => crate::packslip::skills_target(&dirs::HOME),
            (None, false) => crate::packslip::skills_target(
                &config
                    .project_root
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(std::env::current_dir)?,
            ),
        };
        // Without --dir, the project's directory takes the project's own
        // tools, and the global ones go to --global, as auto_sync does. Where
        // the two would be the same directory, whether a project root is
        // loaded or not, it takes both.
        let skills = if self.global {
            crate::packslip::global_skills(&config).await?
        } else if self.dir.is_none()
            && !crate::packslip::same_dir(
                &target.dir,
                &crate::packslip::skills_target(&dirs::HOME).dir,
            )
        {
            crate::packslip::project_skills(&config).await?
        } else {
            crate::packslip::active_skills(&config).await?
        };
        for missing in &skills.missing {
            warn!("{missing}");
        }
        let nothing_declared = skills.missing.is_empty();
        let skills = skills.found;
        let settings = Settings::get();
        let dir = target.dir.clone();
        let prune = self.prune || settings.skills.prune;
        if skills.is_empty() && !prune {
            if nothing_declared {
                miseprintln!("no skills declared by the active tools; nothing to link");
            } else {
                miseprintln!("no skills installed; nothing to link");
            }
            return Ok(());
        }
        let report = crate::packslip::sync_skills(
            &dir,
            &skills,
            &[*dirs::INSTALLS, *dirs::INSTALL_STORE],
            prune,
        )?;
        if let Err(err) = target.link_alias() {
            warn!("could not update .claude/skills: {err}");
        }
        for name in &report.linked {
            let skill = skills.iter().find(|s| &s.name == name);
            miseprintln!(
                "linked {} -> {}",
                crate::file::display_path(dir.join(name)),
                skill
                    .map(|s| crate::file::display_path(&s.path))
                    .unwrap_or_default()
            );
        }
        for name in &report.pruned {
            miseprintln!("removed {}", crate::file::display_path(dir.join(name)));
        }
        for (name, why) in &report.skipped {
            warn!("skipped {name}: {why}");
        }
        if report.linked.is_empty() && report.pruned.is_empty() {
            miseprintln!(
                "{} skill(s) already linked in {}",
                report.unchanged.len(),
                crate::file::display_path(&dir)
            );
        }
        Ok(())
    }
}
