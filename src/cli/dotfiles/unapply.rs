use eyre::Result;

use crate::config::{Config, Settings};
use crate::system;
use crate::system::history::OperationScope;
use crate::ui::prompt;

/// Remove dotfiles applied from `[dotfiles]`
///
/// Removes configured whole-file entries and edits while preserving files
/// mise cannot identify as managed. Modified copies, templates, and plain-line
/// edits require `--force`. Source files and configuration entries are retained.
/// Run this before deleting a declaration so mise can still identify its targets.
///
/// With `--group`, only that dotfile group's files are removed, using what
/// mise recorded when it applied them, so this works even after the group
/// is deselected or deleted from the config.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise dot unapply
mise dot unapply ~/.zshrc
mise dot unapply --group work
mise dot unapply --dry-run
mise dot unapply --force --yes"###
    )
)]
pub(crate) struct DotfilesUnapply {
    /// Only unapply these targets
    #[usage(value_name = "TARGET")]
    targets: Vec<String>,

    /// Remove modified or otherwise ambiguous managed files and lines
    #[usage(long, short)]
    force: bool,

    /// Print the actions that would run without writing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Only unapply the files of this dotfile group, even one that is no
    /// longer selected or declared
    #[usage(long, value_name = "NAME")]
    group: Vec<String>,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

impl DotfilesUnapply {
    pub(crate) async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap dotfiles unapply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let all_files = system::files::files_from_config(&config)?;
        let in_groups = |req: &system::files::FileRequest| {
            self.group.is_empty()
                || req
                    .group
                    .as_ref()
                    .is_some_and(|group| self.group.contains(group))
        };
        let files = all_files
            .iter()
            .filter(|req| {
                in_groups(req)
                    && system::files::matches_target(&req.target, &req.target_raw, &self.targets)
            })
            .cloned()
            .collect::<Vec<_>>();
        let all_edits = system::edits::edits_from_config(&config)?;
        // edits belong to no group
        let edits = all_edits
            .iter()
            .filter(|req| {
                self.group.is_empty() && system::edits::matches_target(req, &self.targets)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut records = vec![];
        for group in &self.group {
            system::dotfile_groups::validate_group_name(group)?;
            match system::dotfile_groups::load_record(group) {
                Some(record) => records.push(record),
                None if files.iter().any(|req| req.group.as_ref() == Some(group)) => {}
                None => eyre::bail!(
                    "dotfile group {group:?} is not configured and has no recorded files"
                ),
            }
        }
        if !self.group.is_empty() && !self.targets.is_empty() {
            eyre::bail!("--group cannot be combined with target arguments");
        }
        // a group's record holds what mise wrote, which is the ownership
        // evidence to remove by; the current source may have changed since.
        // Only a group applied before it had a record goes through the
        // ordinary planner.
        let files = files
            .into_iter()
            .filter(|req| {
                !records
                    .iter()
                    .any(|record| req.group.as_ref() == Some(&record.group))
            })
            .collect::<Vec<_>>();
        if files.is_empty()
            && edits.is_empty()
            && !self.targets.is_empty()
            && (!all_files.is_empty() || !all_edits.is_empty())
        {
            eyre::bail!(
                "no dotfiles matched target filter: {}",
                self.targets.join(", ")
            );
        }
        if files.is_empty() && edits.is_empty() && records.is_empty() {
            super::warn_if_dotfiles_ignored();
            info!("no dotfiles configured in [dotfiles]");
            return Ok(());
        }

        let mut edit_opts = system::edits::UnapplyOpts {
            dry_run: self.dry_run,
            verbose: Settings::get().verbose,
            force: self.force,
            yes: self.yes,
        };
        let mut file_opts = system::files::UnapplyOpts {
            dry_run: self.dry_run,
            verbose: Settings::get().verbose,
            force: self.force,
            yes: self.yes,
        };

        // Validate and plan both domains before either mutates the filesystem.
        // Apply writes whole files before edits, so execute the inverse order.
        let edit_plan = system::edits::plan_unapply(&edits, &edit_opts)?;
        let mut file_plan = system::files::plan_unapply(&files, &file_opts)?;
        let recorded = records
            .iter()
            .map(|record| system::dotfile_groups::without_claimed(record, &all_files))
            .flat_map(|record| record.paths)
            .filter(|path| path.target.symlink_metadata().is_ok())
            .filter(|path| !files.iter().any(|req| path.target.starts_with(&req.target)))
            .count();
        if !self.dry_run
            && !self.yes
            && console::user_attended_stderr()
            && (!edit_plan.is_empty() || !file_plan.is_empty() || recorded > 0)
            && !prompt::confirm(format!(
                "dotfiles: unapply {} file(s) and {} edit(s)?",
                file_plan.len() + recorded,
                edit_plan.len()
            ))?
            .is_yes()
        {
            info!("dotfiles: skipped");
            return Ok(());
        }
        system::files::resolve_unapply(&config, &mut file_plan, &file_opts, &secrets)?;
        system::edits::validate_unapply(&edit_plan)?;
        // Confirmation covers the complete validated plan. Suppress the
        // per-domain prompts so declining cannot leave a partial unapply.
        edit_opts.yes = true;
        file_opts.yes = true;
        if !edits.is_empty() {
            system::edits::execute_unapply(&edit_plan, &edit_opts)?;
        }
        if !files.is_empty() {
            system::files::execute_unapply(&config, &file_plan, &file_opts)?;
        }
        // what a group deployed but no longer declares, or what it deployed
        // before it was deselected
        let remove_opts = system::dotfile_groups::RemoveOpts {
            dry_run: self.dry_run,
            force: self.force,
        };
        for record in &records {
            let record = system::dotfile_groups::without_claimed(record, &all_files);
            system::dotfile_groups::remove_recorded(&record, &remove_opts)?;
        }
        Ok(())
    }
}
