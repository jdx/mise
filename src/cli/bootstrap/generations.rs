use eyre::{Result, bail};

use crate::dirs;
use crate::file::display_path;
use crate::system::generations::journal;
use crate::system::generations::shadow::{DiffOpts, ShadowRepo};
use crate::system::generations::store::{self, Generation, GenerationStatus, Snapshot};
use crate::ui::table::MiseTable;

/// Inspect recorded bootstrap generations
///
/// Every mutating bootstrap command records a generation: what ran, a
/// snapshot of the global config directory and dotfiles root taken before
/// and after the run, the global lockfile, and a journal of what changed.
/// Without a subcommand this lists them, newest first.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct BootstrapGenerations {
    #[usage(subcommand)]
    command: Option<BootstrapGenerationsCommands>,

    #[usage(flatten)]
    ls: BootstrapGenerationsLs,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapGenerationsCommands {
    Diff(BootstrapGenerationsDiff),
    Ls(BootstrapGenerationsLs),
    Show(BootstrapGenerationsShow),
}

/// Diff the snapshotted config and dotfiles between generations
///
/// With one id, shows what that generation's run changed inside the
/// snapshot roots: its snapshot before the run against the one after.
/// With two ids, compares the states the two runs left behind, which is
/// how to see what changed by hand between runs. Paths are prefixed by
/// their root (`config/`, `dotfiles/`, `mise.lock`).
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapGenerationsDiff {
    /// Generation id, `latest`, or `latest~N`
    #[usage(value_name = "A")]
    a: String,

    /// Compare the state after `A` with the state after this generation
    #[usage(value_name = "B")]
    b: Option<String>,

    /// Print the full patch instead of a per-file summary
    #[usage(long, short)]
    patch: bool,

    /// Restrict to one snapshot root or a path inside it
    #[usage(long, value_name = "LABEL[/PATH]")]
    root: Option<String>,

    /// Exit 1 when the snapshots differ
    #[usage(long)]
    exit_code: bool,

    /// Skip the journal entries of the generations covered
    #[usage(long)]
    no_journal: bool,
}

/// List recorded generations, newest first
#[derive(Debug, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment)]
struct BootstrapGenerationsLs {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Show at most this many generations (0 for all)
    #[usage(long, short = 'n', default_value_t = 20, default = "20")]
    limit: usize,

    /// Only list generations whose run did not finish
    #[usage(long)]
    pending: bool,
}

/// Show one generation: what ran, its snapshot, and its journal
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapGenerationsShow {
    /// Generation id, `latest` (the default), or `latest~N`
    #[usage(value_name = "ID")]
    id: Option<String>,

    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// List every file in the snapshot taken after the run
    #[usage(long)]
    files: bool,
}

impl BootstrapGenerations {
    pub(crate) async fn run(self) -> Result<()> {
        match self.command {
            Some(BootstrapGenerationsCommands::Diff(cmd)) => cmd.run(),
            Some(BootstrapGenerationsCommands::Ls(cmd)) => cmd.run(),
            Some(BootstrapGenerationsCommands::Show(cmd)) => cmd.run(),
            None => self.ls.run(),
        }
    }
}

impl BootstrapGenerationsLs {
    fn run(self) -> Result<()> {
        let mut generations = store::list_in(&dirs::STATE)?;
        generations.reverse();
        if self.pending {
            generations.retain(|generation| generation.status == GenerationStatus::Pending);
        }
        if self.limit > 0 {
            generations.truncate(self.limit);
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&generations)?);
            return Ok(());
        }
        if generations.is_empty() {
            if self.pending {
                info!("no pending bootstrap generations");
            } else {
                info!("no bootstrap generations recorded");
            }
            return Ok(());
        }
        let mut table = MiseTable::new(
            false,
            &["ID", "Status", "When", "Command", "Parts", "Snapshot"],
        );
        for generation in &generations {
            table.add_row(vec![
                generation.id.to_string(),
                generation.status.as_str().to_string(),
                local_time(&generation.created_at),
                generation.command.clone(),
                parts(generation),
                snapshot_summary(generation),
            ]);
        }
        table.print()
    }
}

impl BootstrapGenerationsShow {
    fn run(self) -> Result<()> {
        let generations = store::list_in(&dirs::STATE)?;
        let spec = self.id.as_deref().unwrap_or("latest");
        let id = store::resolve_id(spec, &generations)?;
        let generation = store::load_in(&dirs::STATE, id)?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&generation)?);
            return Ok(());
        }
        let g = &generation;
        miseprintln!("Generation {} ({})", g.id, g.status.as_str());
        miseprintln!("  Command:    mise {}", g.command);
        let finished = g
            .finished_at
            .as_deref()
            .map(local_time)
            .unwrap_or_else(|| "(not finished)".into());
        miseprintln!(
            "  Recorded:   {} -> {}",
            local_time(&g.created_at),
            finished
        );
        miseprintln!("  Directory:  {}", display_path(&g.cwd));
        if let Some(user) = &g.user {
            miseprintln!("  User:       {user}");
        }
        miseprintln!("  mise:       {}", g.mise_version);
        if let Some(parent) = g.parent {
            miseprintln!("  Parent:     {parent}");
        }
        if let Some(of) = g.rollback_of {
            miseprintln!("  Rolled back to: {of}");
        }
        match &g.lockfile {
            Some(lock) => miseprintln!(
                "  Lockfile:   {} (sha256 {})",
                display_path(&lock.path),
                &lock.sha256[..12.min(lock.sha256.len())]
            ),
            None => miseprintln!("  Lockfile:   none"),
        }
        let snapshot = &g.snapshot;
        if snapshot.available {
            let before = snapshot.before.as_ref().map(|s| short(&s.commit));
            let after = snapshot.after.as_ref().map(|s| short(&s.commit));
            let unchanged = match snapshot.unchanged {
                Some(true) => " (unchanged)",
                _ => "",
            };
            miseprintln!(
                "  Snapshot:   before {} after {}{unchanged}",
                before.unwrap_or_else(|| "-".into()),
                after.unwrap_or_else(|| "-".into())
            );
            miseprintln!("  Repository: {}", display_path(&snapshot.repo));
        } else {
            miseprintln!(
                "  Snapshot:   unavailable ({})",
                snapshot.reason.as_deref().unwrap_or("unknown reason")
            );
        }
        if let Some(summary) = &g.summary {
            if !summary.parts.is_empty() {
                miseprintln!("  Parts:      {}", summary.parts.join(", "));
            }
            if let Some(message) = &summary.message {
                miseprintln!("  Note:       {message}");
            }
        }
        if let Some(error) = &g.error {
            miseprintln!("  Error:      {error}");
        }

        if let Some(roots) = snapshot.after.as_ref().or(snapshot.before.as_ref()) {
            miseprintln!("");
            let mut table = MiseTable::new(false, &["Root", "Path", "Tree", "Files", "Note"]);
            for root in &roots.roots {
                let note = if let Some(reason) = &root.skipped {
                    format!("skipped: {reason}")
                } else if let Some(label) = &root.alias_of {
                    format!("same directory as {label}")
                } else if let Some(label) = &root.contained_in {
                    format!(
                        "inside {label} at {}",
                        root.subpath
                            .as_deref()
                            .map(display_path)
                            .unwrap_or_default()
                    )
                } else if let Some(vcs) = &root.vcs {
                    format!(
                        "git checkout {}{}",
                        vcs.branch.as_deref().unwrap_or("(detached)"),
                        vcs.head
                            .as_deref()
                            .map(|head| format!(" @ {}", short(head)))
                            .unwrap_or_default()
                    )
                } else {
                    String::new()
                };
                table.add_row(vec![
                    root.label.clone(),
                    display_path(&root.path),
                    root.tree
                        .as_deref()
                        .map(short)
                        .unwrap_or_else(|| "-".into()),
                    root.tree
                        .as_ref()
                        .map(|_| root.files.to_string())
                        .unwrap_or_default(),
                    note,
                ]);
            }
            table.print()?;
            for warning in &roots.warnings {
                miseprintln!("  warning: {warning}");
            }
        }

        if !g.journal.is_empty() {
            miseprintln!("");
            miseprintln!("Journal:");
            for line in journal::render(&g.journal) {
                miseprintln!("  - {line}");
            }
        }

        if self.files {
            let (snapshot, phase) = match (&snapshot.after, &snapshot.before) {
                (Some(after), _) => (after, "after"),
                (None, Some(before)) => (before, "before"),
                (None, None) => bail!("generation {id} has no content snapshot"),
            };
            let Some(shadow) = ShadowRepo::open_or_init_in(&dirs::STATE)? else {
                bail!("listing snapshot files requires git");
            };
            for root in snapshot.roots.iter().filter(|root| root.tree.is_some()) {
                miseprintln!("");
                miseprintln!(
                    "Files in {} ({}) from the snapshot taken {phase} the run:",
                    root.label,
                    display_path(&root.path)
                );
                for entry in shadow.ls_tree(&snapshot.commit, &root.label)? {
                    let size = entry
                        .size
                        .map(|size| size.to_string())
                        .unwrap_or_else(|| "-".into());
                    miseprintln!(
                        "  {} {:>9} {} {}",
                        entry.mode,
                        size,
                        short(&entry.oid),
                        entry.path
                    );
                }
            }
            if let Some(blob) = g.lockfile.as_ref().and_then(|lock| lock.blob.as_deref()) {
                miseprintln!("");
                miseprintln!("Lockfile in the snapshot: mise.lock ({})", short(blob));
            }
        }
        Ok(())
    }
}

impl BootstrapGenerationsDiff {
    fn run(self) -> Result<()> {
        let generations = store::list_in(&dirs::STATE)?;
        let a_id = store::resolve_id(&self.a, &generations)?;
        let a = store::load_in(&dirs::STATE, a_id)?;
        let (from, to, covered, label) = match &self.b {
            Some(b) => {
                let b_id = store::resolve_id(b, &generations)?;
                let b = store::load_in(&dirs::STATE, b_id)?;
                let (lo, hi) = (a_id.min(b_id), a_id.max(b_id));
                let covered = generations
                    .iter()
                    .filter(|g| g.id > lo && g.id <= hi)
                    .cloned()
                    .collect::<Vec<_>>();
                (
                    final_snapshot(&a)?,
                    final_snapshot(&b)?,
                    covered,
                    format!("generation {a_id} -> {b_id}"),
                )
            }
            None => {
                if a.status == GenerationStatus::Pending {
                    bail!(
                        "generation {a_id} did not finish; there is no state after the run to compare"
                    );
                }
                let before = a.snapshot.before.clone().ok_or_else(|| no_snapshot(&a))?;
                let Some(after) = a.snapshot.after.clone() else {
                    bail!("generation {a_id} has no snapshot after the run to compare");
                };
                (before, after, vec![a.clone()], format!("generation {a_id}"))
            }
        };
        // resolved per side: a root may be an alias or nested in one snapshot
        // and stand alone in the other
        let paths = match &self.root {
            Some(root) => Some((
                resolve_root_path(&from, root)?,
                resolve_root_path(&to, root)?,
            )),
            None => None,
        };
        let Some(shadow) = ShadowRepo::open_or_init_in(&dirs::STATE)? else {
            bail!("comparing snapshots requires git");
        };
        let result = shadow.diff(
            &from.tree,
            &to.tree,
            &DiffOpts {
                patch: self.patch,
                color: console::colors_enabled(),
                paths,
            },
        )?;
        if result.changed {
            miseprint!("{}", String::from_utf8_lossy(&result.output))?;
        } else {
            info!("{label}: no differences");
        }
        if !self.no_journal {
            for generation in covered.iter().filter(|g| !g.journal.is_empty()) {
                miseprintln!("Journal (generation {}):", generation.id);
                for line in journal::render(&generation.journal) {
                    miseprintln!("  - {line}");
                }
            }
        }
        if self.exit_code && result.changed {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

/// The state a generation left behind: its `after` snapshot, or `before`
/// when the run never finished.
fn final_snapshot(generation: &Generation) -> Result<Snapshot> {
    generation
        .snapshot
        .after
        .clone()
        .or_else(|| generation.snapshot.before.clone())
        .ok_or_else(|| no_snapshot(generation))
}

fn no_snapshot(generation: &Generation) -> eyre::Report {
    eyre::eyre!(
        "generation {} has no content snapshot ({})",
        generation.id,
        generation
            .snapshot
            .reason
            .as_deref()
            .unwrap_or("not recorded")
    )
}

/// Turns `label` or `label/path` into a path inside the snapshot's top-level
/// tree, following roots that are aliases of or contained in another.
fn resolve_root_path(snapshot: &Snapshot, spec: &str) -> Result<String> {
    let (label, rest) = spec.split_once('/').unwrap_or((spec, ""));
    let root = snapshot
        .roots
        .iter()
        .find(|root| root.label == label)
        .ok_or_else(|| eyre::eyre!("no snapshot root named {label}"))?;
    if let Some(reason) = &root.skipped {
        bail!("snapshot root {label} was skipped ({reason})");
    }
    let mut base = if let Some(alias) = &root.alias_of {
        alias.clone()
    } else if let Some(outer) = &root.contained_in {
        match &root.subpath {
            // git tree paths always use `/`
            Some(subpath) => format!("{outer}/{}", subpath.to_string_lossy().replace('\\', "/")),
            None => outer.clone(),
        }
    } else {
        label.to_string()
    };
    if !rest.is_empty() {
        base.push('/');
        base.push_str(rest.trim_end_matches('/'));
    }
    Ok(base)
}

fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

fn parts(generation: &Generation) -> String {
    generation
        .summary
        .as_ref()
        .map(|summary| summary.parts.join(","))
        .filter(|parts| !parts.is_empty())
        .unwrap_or_else(|| "-".into())
}

fn snapshot_summary(generation: &Generation) -> String {
    let snapshot = &generation.snapshot;
    if !snapshot.available {
        return "unavailable".into();
    }
    match (&snapshot.before, &snapshot.after) {
        (_, Some(after)) if snapshot.unchanged == Some(true) => {
            format!("{} (unchanged)", short(&after.commit))
        }
        (_, Some(after)) => short(&after.commit),
        (Some(before), None) => format!("{} (before only)", short(&before.commit)),
        (None, None) => "-".into(),
    }
}

fn local_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.to_string())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap generations</bold>
    $ <bold>mise bootstrap generations --json | jq '.[0]'</bold>
    $ <bold>mise bootstrap generations show latest</bold>
    $ <bold>mise bootstrap generations show 12 --files</bold>
    $ <bold>mise bootstrap generations diff 11 12 --patch</bold>
"#
);
