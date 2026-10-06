use std::path::{Path, PathBuf};

use eyre::{Result, bail};

use super::add::DotfilesAdd;
use crate::config::Config;
use crate::file;
use crate::path::PathExt;
use crate::system;
use crate::system::files::FileMode;
use crate::system::history::OperationScope;
use crate::system::history::tracked::{self, TrackedSet};
use crate::ui::prompt;

/// Edit a managed dotfile source
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise dot edit ~/.zshrc
mise dot edit --apply ~/.config/starship.toml"###
    )
)]
pub(crate) struct DotfilesEdit {
    /// Target to edit
    #[usage(value_name = "TARGET")]
    target: String,

    /// Apply this target after the editor exits
    #[usage(long)]
    apply: bool,

    /// Dotfile mode to use if the target is not yet managed
    #[usage(long, short)]
    mode: Option<String>,

    /// Source path to use if the target is not yet managed
    #[usage(long, short, value_name = "PATH")]
    source: Option<PathBuf>,

    /// Skip the confirmation prompt when adding an unmanaged target
    #[usage(long, short)]
    yes: bool,

    /// Dotfile group whose tree holds the target, when several groups
    /// deploy into its directory
    #[usage(long, value_name = "NAME", conflicts = ["source"])]
    group: Option<String>,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

impl DotfilesEdit {
    /// Open the managed source and optionally converge its target afterward.
    pub(crate) async fn run(self) -> Result<()> {
        // The editor itself changes the managed source, so the whole command
        // is one generation, not just the optional apply.
        OperationScope::wrap("bootstrap dotfiles edit", false, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let mut config = Config::get().await?;
        let target = system::files::resolve_target_arg(&self.target);
        if self.apply {
            let files = system::files::files_from_config(&config)?;
            system::files::validate_composed_file_footprints(&files)?;
        }

        let group = self.group.as_deref();
        if let Some(path) = source_for_target(&config, &target, &self.target, group)? {
            open_or_create(&path)?;
            crate::cli::editor::open_in_editor(&path)?;
            if self.apply {
                apply_target(&self.target, group, self.prompt_secrets).await?;
            }
            return Ok(());
        }

        if !self.yes && console::user_attended_stderr() {
            let ok = prompt::confirm(format!("dotfiles: add {}?", self.target))?.is_yes();
            if !ok {
                info!("dotfiles: skipped");
                return Ok(());
            }
        } else if !self.yes {
            bail!("{} is not managed by [dotfiles]", self.target);
        }

        DotfilesAdd {
            targets: vec![self.target.clone()],
            changed: false,
            mode: self.mode.clone(),
            source: self.source.clone(),
            global: true,
            local: false,
            path: None,
            dry_run: false,
            no_apply: true,
            force: false,
            yes: true,
            prompt_secrets: self.prompt_secrets,
            group: self.group.clone(),
        }
        .run()
        .await?;

        config = Config::reset().await?;
        let Some(path) = source_for_target(&config, &target, &self.target, group)? else {
            bail!("failed to add {}", self.target);
        };
        open_or_create(&path)?;
        crate::cli::editor::open_in_editor(&path)?;
        if self.apply {
            apply_target(&self.target, group, self.prompt_secrets).await?;
        }
        Ok(())
    }
}

fn source_for_target(
    config: &Config,
    target: &std::path::Path,
    raw: &str,
    group: Option<&str>,
) -> Result<Option<PathBuf>> {
    let all_files = system::files::files_from_config(config)?;
    let matching_files = all_files
        .iter()
        .filter(|req| {
            system::files::matches_target(&req.target, &req.target_raw, &[raw.to_string()])
        })
        .cloned()
        .collect::<Vec<_>>();
    // one target may be both tracked and deployed, and tracking composes
    // first. editing the source is what converges a deployed target — and
    // what `--apply` would otherwise write back over — so the deployment
    // entry wins; the tracked file is edited only when nothing deploys it.
    let deployments = matching_files
        .iter()
        .filter(|req| req.mode != FileMode::Track)
        .collect::<Vec<_>>();
    // composed roots may each deploy into one target (symlink-each entries
    // whose leaves do not collide), and nothing says which source the edit
    // means — name them instead of opening whichever composed first
    if deployments.len() > 1 {
        let sources = deployments
            .iter()
            .map(|req| req.source.display_user())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("{raw}: multiple [dotfiles] entries deploy this target; edit one of: {sources}");
    }
    let selected = deployments
        .into_iter()
        .next()
        .or_else(|| matching_files.first());
    if let Some(req) = selected {
        // a tracked file has no source: it stays where it is, so that is
        // what to edit. inline content lives in the config that declares
        // it, like an inline edit entry.
        return Ok(Some(match req.mode {
            FileMode::Track => {
                warn_if_the_edit_escapes_history(config, &req.target);
                req.target.clone()
            }
            // neither has a source file: the declaring config is what
            // changes them
            FileMode::Content | FileMode::Absent => req.origin.config.clone(),
            // mise manages only the permissions; the file itself is the
            // only copy of its content, and mise never creates it
            FileMode::Permissions => {
                if std::fs::symlink_metadata(&req.target).is_err() {
                    bail!(
                        "{raw}: only its permissions are managed and it does not exist; create it first"
                    );
                }
                req.target.clone()
            }
            _ => req.source.clone(),
        }));
    }
    let matching_edits = system::edits::edits_from_config(config)?
        .into_iter()
        .filter(|req| system::edits::matches_target(req, &[raw.to_string()]))
        .collect::<Vec<_>>();
    match matching_edits.as_slice() {
        [] => {}
        [req] => {
            return Ok(Some(
                req.op
                    .source_file()
                    .map_or_else(|| req.config_path.clone(), Path::to_path_buf),
            ));
        }
        edits => {
            let keys = edits
                .iter()
                .map(|req| req.config_key())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("{raw}: multiple [dotfiles] edit entries match; choose one of: {keys}");
        }
    }
    // a file inside a dotfile group's tree has no entry of its own, and
    // edit keys such as `~/.zshrc/activate` are checked before it; once its
    // source exists, that source is what to edit (before that, add captures
    // the live file into the group)
    if let Some(req) = group_route(&all_files, target, group)? {
        let source = group_file_source(req, target)?;
        if source.symlink_metadata().is_ok() {
            return Ok(Some(source));
        }
        return Ok(None);
    }
    if target.is_relative() {
        bail!("{raw}: target must be absolute or start with ~/");
    }
    Ok(None)
}

/// History captures a tracked symlink as a link, never its destination, so an
/// editor that follows the link writes to a file the surrounding checkpoint
/// does not hold. Say so rather than record a generation that misses the edit.
fn warn_if_the_edit_escapes_history(config: &Config, target: &std::path::Path) {
    if !file::is_symlink_or_junction(target) {
        return;
    }
    // resolve the chain the way an atomic write does, so a dangling or
    // unreadable link — which still writes through to its destination — is
    // reported rather than mistaken for a path that escapes nothing
    let destination = match file::atomic_write_target(target) {
        Ok(destination) => tracked::normalize_target(&destination),
        Err(err) => {
            // an unresolvable chain is one the editor cannot open either
            debug!(
                "dotfiles: could not resolve {}: {err:#}",
                target.display_user()
            );
            return;
        }
    };
    if destination == target {
        return;
    }
    match TrackedSet::from_config(config).and_then(|set| set.would_capture(&destination)) {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            debug!("dotfiles: could not resolve the tracked set: {err:#}");
        }
    }
    warn!(
        "{} is tracked as a symlink: this edits {}, which history does not capture; track that path too to save its contents",
        target.display_user(),
        destination.display_user()
    );
}

fn open_or_create(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write(path, "")?;
    }
    Ok(())
}

/// Apply a selected target after validating the complete composed footprint.
async fn apply_target(target: &str, group: Option<&str>, prompt_secrets: bool) -> Result<()> {
    let config = Config::reset().await?;
    let secrets = system::secrets::resolve(&config, prompt_secrets)?;
    let targets = vec![target.to_string()];
    let all_files = system::files::files_from_config(&config)?;
    system::files::validate_composed_file_footprints(&all_files)?;
    let resolved = system::files::resolve_target_arg(target);
    let mut files = all_files
        .iter()
        .filter(|req| system::files::matches_target(&req.target, &req.target_raw, &targets))
        .cloned()
        .collect::<Vec<_>>();
    // a file inside a group tree is applied on its own, never the whole
    // tree, which would overwrite changes to its other files without asking
    let edits = system::edits::edits_from_config(&config)?
        .into_iter()
        .filter(|req| system::edits::matches_target(req, &targets))
        .collect::<Vec<_>>();
    // edit keys such as `~/.zshrc/activate` win over a group tree, as when
    // opening the target
    if files.is_empty()
        && edits.is_empty()
        && let Some(req) = group_route(&all_files, &resolved, group)?
    {
        files.push(group_file_request(req, &resolved)?);
    }
    if !files.is_empty() {
        let opts = system::files::ApplyOpts {
            dry_run: false,
            verbose: false,
            force: false,
            force_hint: "use `mise dot apply --force`",
            yes: true,
        };
        system::files::apply(&config, &files, &opts, &secrets, &mut vec![])?;
    }
    if !edits.is_empty() {
        let opts = system::edits::ApplyOpts {
            part: "dotfiles",
            dry_run: false,
            verbose: false,
            yes: true,
        };
        system::edits::apply(&config, &edits, &opts, &mut vec![])?;
    }
    Ok(())
}

/// The dotfile group tree `target` lies in, when no entry names the target
/// itself.
fn group_route<'a>(
    files: &'a [system::files::FileRequest],
    target: &std::path::Path,
    group: Option<&str>,
) -> Result<Option<&'a system::files::FileRequest>> {
    if files.iter().any(|req| req.target == target) {
        return Ok(None);
    }
    system::dotfile_groups::route_add(files, target, group)
}

/// The source file in `req`'s group tree that deploys to `target`.
fn group_file_source(
    req: &system::files::FileRequest,
    target: &std::path::Path,
) -> Result<PathBuf> {
    Ok(req.source.join(system::dotfile_groups::source_rel_for(
        &req.source,
        target.strip_prefix(&req.target)?,
        req.dot_prefix,
    )))
}

/// `req`, a group tree, narrowed to the one file that deploys to `target`:
/// a link for a linked tree, a copy for a copied one. The file comes from
/// the tree's own walk, so `exclude`, `dot_prefix`, and a Git manifest
/// apply as they do to the whole tree; a directory, or a file the tree
/// would not deploy, is refused.
fn group_file_request(
    req: &system::files::FileRequest,
    target: &std::path::Path,
) -> Result<system::files::FileRequest> {
    let Some((source, _)) = system::files::directory_source_files(req)?
        .into_iter()
        .find(|(_, deployed)| deployed == target)
    else {
        bail!(
            "{}: dotfile group {} does not deploy this file (a directory, excluded, or outside its manifest); run `mise dot apply` to apply the group",
            target.display_user(),
            req.group.as_deref().unwrap_or_default()
        );
    };
    let mode = match req.mode {
        FileMode::SymlinkEach => FileMode::Symlink,
        mode => mode,
    };
    let mut origin = req.origin.clone();
    origin.source = Some(source.clone());
    Ok(system::files::FileRequest {
        target_raw: target.display_user(),
        target: target.to_path_buf(),
        source,
        mode,
        exclude: vec![],
        manifest: None,
        dot_prefix: false,
        origin,
        ..req.clone()
    })
}
