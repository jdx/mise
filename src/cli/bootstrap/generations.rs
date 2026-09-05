//! `mise bootstrap generations`: the operations `mise bootstrap` recorded,
//! as a view over `mise history`.

use eyre::{Result, bail};

use crate::cli::history::{local_time, short};
use crate::file::display_path;
use crate::system::history::journal;
use crate::system::history::shadow::DiffOpts;
use crate::system::history::store::{Entry, OperationStatus};
use crate::system::history::tracked::{display_to_tree_path, global_config_dir};
use crate::ui::table::MiseTable;

/// Inspect the operations bootstrap recorded
///
/// Every mutating bootstrap command records a pair of history checkpoints:
/// the tracked files before the run and after it, with a journal of what
/// the run changed. This lists those operations, newest first; `mise
/// history` lists every checkpoint.
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

/// Diff the tracked files between operations
///
/// With one id, shows what that operation's run changed: the checkpoint
/// before the run against the one after. With two ids, compares the states
/// the two runs left behind, which is how to see what changed by hand
/// between runs.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapGenerationsDiff {
    /// Checkpoint id, `latest`, or `latest~N` (among operations)
    #[usage(value_name = "A")]
    a: String,

    /// Compare the state after `A` with the state after this operation
    #[usage(value_name = "B")]
    b: Option<String>,

    /// Print the full patch instead of a per-file summary
    #[usage(long, short)]
    patch: bool,

    /// Restrict to `config` or `dotfiles` (the config directory or dotfiles
    /// root), a path inside one (`config/dotfiles/zshrc`), or any tracked path
    #[usage(long, value_name = "LABEL[/PATH]")]
    root: Option<String>,

    /// Exit 1 when the snapshots differ
    #[usage(long)]
    exit_code: bool,

    /// Skip the journal entries of the operations covered
    #[usage(long)]
    no_journal: bool,
}

/// List recorded operations, newest first
#[derive(Debug, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment)]
struct BootstrapGenerationsLs {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Show at most this many operations (0 for all)
    #[usage(long, short = 'n', default_value_t = 20, default = "20")]
    limit: usize,

    /// Only list operations whose run did not finish
    #[usage(long)]
    pending: bool,
}

/// Show one operation: what ran, its snapshots, and its journal
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapGenerationsShow {
    /// Checkpoint id, `latest` (the default), or `latest~N` (among operations)
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
            Some(BootstrapGenerationsCommands::Diff(cmd)) => cmd.run().await,
            Some(BootstrapGenerationsCommands::Ls(cmd)) => cmd.run().await,
            Some(BootstrapGenerationsCommands::Show(cmd)) => cmd.run().await,
            None => self.ls.run().await,
        }
    }
}

/// The operation outcomes among all checkpoints, oldest first.
fn operations(entries: &[Entry]) -> Vec<Entry> {
    entries
        .iter()
        .filter(|entry| entry.checkpoint.operation.is_some())
        .cloned()
        .collect()
}

fn before_of<'a>(entries: &'a [Entry], entry: &Entry) -> Option<&'a Entry> {
    let before = entry.checkpoint.operation.as_ref()?.before.as_deref()?;
    entries
        .iter()
        .find(|candidate| candidate.checkpoint.uuid == before)
}

impl BootstrapGenerationsLs {
    async fn run(self) -> Result<()> {
        let (_store, _tracked, entries) = crate::cli::history::open().await?;
        let mut operations = operations(&entries);
        operations.reverse();
        if self.pending {
            operations.retain(|entry| entry.checkpoint.status() == Some(OperationStatus::Pending));
        }
        if self.limit > 0 {
            operations.truncate(self.limit);
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&operations)?);
            return Ok(());
        }
        if operations.is_empty() {
            if self.pending {
                info!("no pending bootstrap operations");
            } else {
                info!("no bootstrap operations recorded");
            }
            return Ok(());
        }
        let mut table = MiseTable::new(
            false,
            &[
                "ID", "Status", "When", "Command", "Parts", "Before", "Changes",
            ],
        );
        for entry in &operations {
            let operation = entry.checkpoint.operation.as_ref().expect("filtered");
            table.add_row(vec![
                entry.id.to_string(),
                operation.status.as_str().to_string(),
                local_time(&entry.checkpoint.created_at),
                operation.command.clone(),
                if operation.parts.is_empty() {
                    "-".to_string()
                } else {
                    operation.parts.join(",")
                },
                before_of(&entries, entry)
                    .map(|before| before.id.to_string())
                    .unwrap_or_else(|| "-".into()),
                if entry.checkpoint.tree.available {
                    entry.checkpoint.changes.len().to_string()
                } else {
                    "unavailable".into()
                },
            ]);
        }
        table.print()
    }
}

fn resolve_operation(spec: &str, entries: &[Entry]) -> Result<Entry> {
    let operations = operations(entries);
    let id = if spec.starts_with("latest") {
        crate::system::history::store::resolve_ref(spec, &operations)?
    } else {
        crate::system::history::store::resolve_ref(spec, entries)?
    };
    entries
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or_else(|| eyre::eyre!("no bootstrap generation {id}"))
}

impl BootstrapGenerationsShow {
    async fn run(self) -> Result<()> {
        let (store, _tracked, entries) = crate::cli::history::open().await?;
        let entry = resolve_operation(self.id.as_deref().unwrap_or("latest"), &entries)?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&entry)?);
            return Ok(());
        }
        let c = &entry.checkpoint;
        let Some(operation) = &c.operation else {
            bail!(
                "checkpoint {} is not a bootstrap operation; see `mise history show {}`",
                entry.id,
                entry.id
            );
        };
        miseprintln!("Generation {} ({})", entry.id, operation.status.as_str());
        miseprintln!("  Command:    mise {}", operation.command);
        miseprintln!("  Started:    {}", local_time(&c.created_at));
        if let Some(finished) = &operation.finished_at {
            miseprintln!("  Finished:   {}", local_time(finished));
        }
        miseprintln!("  Directory:  {}", display_path(&operation.cwd));
        if let Some(user) = &operation.user {
            miseprintln!("  User:       {user}");
        }
        miseprintln!("  mise:       {}", c.mise_version);
        if let Some(lock) = &operation.lockfile {
            miseprintln!(
                "  Lockfile:   {} (sha256 {})",
                display_path(&lock.path),
                &lock.sha256[..12.min(lock.sha256.len())]
            );
        }
        let before = before_of(&entries, &entry);
        if c.tree.available {
            let before_tree = before.and_then(|before| before.checkpoint.tree.snapshot.as_deref());
            let unchanged = match (before_tree, c.tree.snapshot.as_deref()) {
                (Some(a), Some(b)) if a == b => " (unchanged)",
                _ => "",
            };
            miseprintln!(
                "  Snapshot:   before {} after {}{unchanged}",
                before
                    .map(|before| format!("checkpoint {}", before.id))
                    .unwrap_or_else(|| "-".into()),
                c.tree
                    .snapshot
                    .as_deref()
                    .map(short)
                    .unwrap_or_else(|| "-".into())
            );
            miseprintln!(
                "  Repository: {}",
                display_path(
                    store
                        .repo()
                        .map(|repo| repo.dir().to_path_buf())
                        .unwrap_or_default()
                )
            );
        } else {
            miseprintln!(
                "  Snapshot:   unavailable ({})",
                c.tree.reason.as_deref().unwrap_or("unknown reason")
            );
        }
        if !operation.parts.is_empty() {
            miseprintln!("  Parts:      {}", operation.parts.join(", "));
        }
        if let Some(message) = &operation.message {
            miseprintln!("  Note:       {message}");
        }
        if let Some(error) = &operation.error {
            miseprintln!("  Error:      {error}");
        }
        if !c.changes.is_empty() {
            miseprintln!("");
            miseprintln!("Changed by the run:");
            for path in &c.changes.modified {
                miseprintln!("  M {path}");
            }
            for path in &c.changes.added {
                miseprintln!("  A {path}");
            }
            for path in &c.changes.removed {
                miseprintln!("  D {path}");
            }
        }
        if !operation.journal.is_empty() {
            miseprintln!("");
            miseprintln!("Journal:");
            for line in journal::render(&operation.journal) {
                miseprintln!("  - {line}");
            }
        }
        if self.files {
            let Some(tree) = &c.tree.snapshot else {
                bail!("generation {} has no content snapshot", entry.id);
            };
            let Some(repo) = store.repo() else {
                bail!("listing snapshot files requires git");
            };
            miseprintln!("");
            miseprintln!("Files in the snapshot taken after the run:");
            for file in repo.ls_tree(tree)? {
                let size = file.size.map(|size| size.to_string()).unwrap_or_default();
                miseprintln!(
                    "  {} {:>8} {}",
                    file.mode,
                    size,
                    crate::system::history::tracked::tree_path_to_display(&file.path)
                );
            }
        }
        Ok(())
    }
}

impl BootstrapGenerationsDiff {
    async fn run(self) -> Result<()> {
        let (store, _tracked, entries) = crate::cli::history::open().await?;
        let a = resolve_operation(&self.a, &entries)?;
        let (from, to, covered, label) = match &self.b {
            Some(b) => {
                let b = resolve_operation(b, &entries)?;
                let covered: Vec<Entry> = entries
                    .iter()
                    .filter(|entry| {
                        entry.checkpoint.operation.is_some() && entry.id > a.id && entry.id <= b.id
                    })
                    .cloned()
                    .collect();
                (
                    tree_of(&a)?,
                    tree_of(&b)?,
                    covered,
                    format!("generation {} -> {}", a.id, b.id),
                )
            }
            None => {
                if a.checkpoint.status() == Some(OperationStatus::Pending) {
                    bail!(
                        "generation {} did not finish; there is no state after the run to compare",
                        a.id
                    );
                }
                let Some(before) = before_of(&entries, &a) else {
                    bail!(
                        "generation {} has no snapshot before the run to compare",
                        a.id
                    );
                };
                (
                    tree_of(before)?,
                    tree_of(&a)?,
                    vec![a.clone()],
                    format!("generation {}", a.id),
                )
            }
        };
        let paths = self.root.as_deref().map(root_path).transpose()?;
        let Some(repo) = store.repo() else {
            bail!("comparing snapshots requires git");
        };
        let result = repo.diff(
            &from,
            &to,
            &DiffOpts {
                patch: self.patch,
                color: console::colors_enabled(),
                paths: paths.map(|path| (path.clone(), path)),
            },
        )?;
        if result.changed {
            miseprint!("{}", String::from_utf8_lossy(&result.output))?;
        } else {
            info!("{label}: no differences");
        }
        if !self.no_journal {
            for entry in &covered {
                let Some(operation) = &entry.checkpoint.operation else {
                    continue;
                };
                for line in journal::render(&operation.journal) {
                    info!("{line}");
                }
            }
        }
        if self.exit_code && result.changed {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

fn tree_of(entry: &Entry) -> Result<String> {
    entry
        .checkpoint
        .tree
        .snapshot
        .clone()
        .ok_or_else(|| eyre::eyre!("generation {} has no content snapshot", entry.id))
}

/// `config[/path]`, `dotfiles[/path]`, or any path -> a snapshot tree path.
fn root_path(spec: &str) -> Result<String> {
    let (label, rest) = spec.split_once('/').unwrap_or((spec, ""));
    let base = match label {
        "config" => global_config_dir(),
        "dotfiles" => crate::system::files::dotfiles_root(),
        _ => {
            return Ok(display_to_tree_path(spec));
        }
    };
    let path = if rest.is_empty() {
        base
    } else {
        base.join(rest.trim_end_matches('/'))
    };
    Ok(display_to_tree_path(&path.to_string_lossy()))
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
