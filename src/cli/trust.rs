use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::config_file::config_trust_root;
use crate::config::{
    ALL_CONFIG_FILES, DEFAULT_CONFIG_FILENAMES, Settings, config_file, config_files_in_dir,
    is_global_config,
};
use crate::file::{display_path, remove_file};
use crate::{config, dirs, env, file};
use eyre::{Result, bail};
use itertools::Itertools;

/// Marks a config file as trusted
///
/// This means mise is allowed to parse the file when it needs to read config
/// that may execute code or affect the environment. Without trust, mise may
/// prompt, skip the config in some discovery paths, or fail with an
/// untrusted-config error when it cannot prompt.
///
/// In normal mode, commands that execute project-defined behavior (`mise run`,
/// naked task invocations such as `mise <TASK>`, `mise install`, `mise exec`,
/// and `mise watch`) automatically trust their active config. Paranoid mode
/// requires explicit, content-bound trust for every non-global config.
///
/// In normal mode, safe config files do not require trust: files that only contain
/// `min_version`, `[tools]` entries with plain version strings (or arrays of
/// them), and `[tasks]` without templates or tool options.
///
/// Trust is shared across git worktrees: a config file inside a linked
/// worktree is trusted when the equivalent path in the repository's main
/// checkout has been trusted. Paranoid mode disables this sharing since
/// worktrees can check out branches with different config contents.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct Trust {
    /// The config file whose trust status to change
    #[usage(value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    config_file: Option<PathBuf>,

    /// Trust all config files in the current directory, its parents, and its subdirectories
    ///
    /// Subdirectories are walked respecting .gitignore, skipping hidden directories
    /// and common build/dependency directories (node_modules, vendor, target, dist, build).
    #[usage(long, short, verbatim_doc_comment, conflicts = &["ignore", "untrust"])]
    all: bool,

    /// Do not trust this config and ignore it in the future
    #[usage(long, conflicts = "untrust")]
    ignore: bool,

    /// Show the trusted status of config files from the current directory and its parents.
    /// Does not trust or untrust any files.
    #[usage(long, verbatim_doc_comment)]
    show: bool,

    /// Remove explicit trust for this config
    #[usage(long)]
    untrust: bool,
}

impl Trust {
    pub(crate) async fn run(mut self) -> Result<()> {
        if self.show {
            return self.show();
        }
        if self.untrust {
            untrust_config_file(self.config_file()?)
        } else if self.ignore {
            self.ignore()
        } else if self.all {
            while let Some(p) = self.get_next_untrusted() {
                self.config_file = Some(p);
                self.trust()?;
            }
            for p in self.get_untrusted_descendants() {
                self.config_file = Some(p);
                self.trust()?;
            }
            Ok(())
        } else {
            self.trust()
        }
    }
    pub(crate) fn clean() -> Result<()> {
        Self::clean_in(&dirs::TRUSTED_CONFIGS)?;
        Self::clean_in(&dirs::IGNORED_CONFIGS)
    }

    /// Remove the entries whose target is gone.
    ///
    /// Resolve first. Asking whether the *entry* exists answers the wrong question: mise records
    /// these as symlinks on unix and, since Windows symlinks need a privilege mise does not
    /// require, as plain files holding the path there (`file::make_symlink_or_file`). A plain file
    /// always exists no matter what it records, so on Windows this pruned nothing at all — while
    /// `mise prune --configs` says it removes "tracked and trusted configuration links that point
    /// to nonexistent configurations".
    ///
    /// `Tracker::clean_in` is the same shape, one line above the call to this in
    /// `Prune::prune_configs`. The one difference is deliberate: it asks `is_file()` because it
    /// records config files, and a trust root is a **directory**, so the same question here would
    /// delete every entry.
    ///
    /// Not everything in the directory is an entry. `config_file::trust` writes a `.hash` beside
    /// one in paranoid mode and `mark_as_monorepo_root` writes a `.monorepo` marker; those hold a
    /// checksum and nothing at all, not a path, so they are skipped here and removed with the
    /// entry they belong to — the way `config_file::untrust` already removes all three together.
    fn clean_in(dir: &Path) -> Result<()> {
        if dir.is_dir() {
            for path in file::ls(dir)? {
                if is_trust_metadata(&path) {
                    continue;
                }
                let keep = match file::resolve_symlink(&path)? {
                    // `try_exists` rather than `exists`, which reports a metadata error — an
                    // offline network share, a home directory that is not unlocked — as "not
                    // there". Keep the entry when the answer is unknown: prune removes records,
                    // and losing trust for a project that does exist is the costlier mistake.
                    Some(target) => target.try_exists().unwrap_or_else(|err| {
                        debug!("keeping {}: {err}", display_path(&path));
                        true
                    }),
                    None => false,
                };
                if !keep {
                    remove_file(&path)?;
                    for ext in TRUST_METADATA_EXTENSIONS {
                        let sibling = config_file::with_appended_extension(&path, ext);
                        if sibling.exists() {
                            remove_file(&sibling)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// The suffixes `config_file` appends to a trust entry for the metadata that belongs to it.
const TRUST_METADATA_EXTENSIONS: [&str; 2] = ["hash", "monorepo"];

/// Whether a file in the trust directory is metadata for an entry rather than an entry itself.
fn is_trust_metadata(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            TRUST_METADATA_EXTENSIONS
                .iter()
                .any(|ext| name.ends_with(&format!(".{ext}")))
        })
}

pub(super) fn untrust_config_file(config_file: Option<PathBuf>) -> Result<()> {
    let path = match config_file {
        Some(filename) => filename,
        None => match ALL_CONFIG_FILES.first().cloned() {
            Some(path) => path,
            None => {
                warn!("No trusted config files found.");
                return Ok(());
            }
        },
    };
    let cfr = config_trust_root(&path);
    config_file::untrust(&cfr)?;
    let cfr = cfr.canonicalize()?;
    info!("untrusted {}", display_path(&cfr));

    let trusted_via_settings = Settings::get()
        .trusted_config_paths()
        .any(|p| cfr.starts_with(p));
    if trusted_via_settings {
        warn!(
            "{} is trusted via settings so it will still be trusted.",
            display_path(&cfr)
        );
    }

    if !Settings::get().paranoid
        && let Some(main_path) = crate::git::main_checkout_equivalent(&cfr)
        && config_file::is_trusted(&main_path)
    {
        warn!(
            "{} is a git worktree of {} which is trusted, so it will still be trusted. Untrust that path or use `mise trust --ignore`.",
            display_path(&cfr),
            display_path(&main_path)
        );
    }

    Ok(())
}

/// The config file a user-supplied path refers to, or `None` when none was given.
///
/// The path has to exist. Trusting is not done against the path as typed: it is resolved to a
/// trust root first, and `config_root` does that by counting path components, never by looking at
/// the filesystem. A path that is not there therefore used to resolve to its *parent*, and mise
/// would trust or untrust that instead — exit 0, `trusted <parent>`, and a typo silently granting
/// trust to a directory nobody named.
///
/// Existence is the whole check. A directory with no config file in it yet still resolves, since
/// its trust root is the directory itself and trusting a project before writing its `mise.toml`
/// is a real thing to want.
pub(super) fn resolve_config_file(config_file: Option<&PathBuf>) -> Result<Option<PathBuf>> {
    let Some(config_file) = config_file else {
        return Ok(None);
    };
    if !config_file.exists() {
        bail!(
            "Path does not exist: {}\n\
             mise resolves this to a trust root before recording anything, and that resolution is \
             lexical — a path that is not there would act on its parent directory instead.",
            display_path(config_file)
        );
    }
    Ok(Some(if config_file.is_dir() {
        config_files_in_dir(config_file)
            .last()
            .cloned()
            .unwrap_or(config_file.join(&*env::MISE_DEFAULT_CONFIG_FILENAME))
    } else {
        config_file.clone()
    }))
}

impl Trust {
    fn ignore(&self) -> Result<()> {
        let path = match self.config_file()? {
            Some(filename) => filename,
            None => match self.get_next() {
                Some(path) => path,
                None => {
                    warn!("No trusted config files found.");
                    return Ok(());
                }
            },
        };
        let cfr = config_trust_root(&path);
        config_file::add_ignored(cfr.clone())?;
        let cfr = cfr.canonicalize()?;
        info!("ignored {}", display_path(&cfr));

        let trusted_via_settings = Settings::get()
            .trusted_config_paths()
            .any(|p| cfr.starts_with(p));
        if trusted_via_settings {
            warn!(
                "{} is trusted via settings so it will still be trusted.",
                display_path(&cfr)
            );
        }
        Ok(())
    }
    fn trust(&self) -> Result<()> {
        let path = match self.config_file()? {
            Some(filename) => config_trust_root(&filename),
            None => match self.get_next_untrusted() {
                Some(path) => path,
                None => {
                    warn!("No untrusted config files found.");
                    return Ok(());
                }
            },
        };
        config_file::trust(&path)?;
        let cfr = path.canonicalize()?;
        info!("trusted {}", display_path(&cfr));
        Ok(())
    }

    fn config_file(&self) -> Result<Option<PathBuf>> {
        resolve_config_file(self.config_file.as_ref())
    }

    fn get_next(&self) -> Option<PathBuf> {
        ALL_CONFIG_FILES.first().cloned()
    }
    fn get_next_untrusted(&self) -> Option<PathBuf> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
            .into_iter()
            .filter(|p| !is_global_config(p))
            .map(|p| config_trust_root(&p))
            .unique()
            .find(|ctr| !config_file::is_trusted(ctr))
    }

    /// Untrusted config files in subdirectories of the current directory.
    ///
    /// Walks respecting .gitignore, skipping hidden directories and common
    /// build/dependency directories so e.g. vendored configs in node_modules
    /// or vendor are not trusted. Returns one config file per untrusted trust
    /// root; `trust()` computes the trust root from each.
    fn get_untrusted_descendants(&self) -> Vec<PathBuf> {
        const EXCLUDED_DIRS: &[&str] = &["node_modules", "vendor", "target", "dist", "build"];
        // Respect config discovery being disabled, matching load_config_paths
        // used by the ancestor-walk pass.
        if Settings::no_config() {
            return vec![];
        }
        // Use the live cwd (not the cached dirs::CWD) so this anchors to the
        // same directory as the ancestor-walk pass, which uses env::current_dir
        // via load_config_paths -> all_dirs. A `cd` setting applied during
        // settings load can move the process directory, and both passes must
        // agree on where "here" is.
        let Ok(cwd) = env::current_dir() else {
            return vec![];
        };
        let walker = ignore::WalkBuilder::new(&cwd)
            .hidden(true) // Skip hidden files/dirs
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global .gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .require_git(false) // Don't require a git repo
            .filter_entry(|e| {
                // Never exclude the walk root itself (depth 0), even if cwd is
                // named e.g. `build` or `vendor` — otherwise nothing is walked.
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !EXCLUDED_DIRS.contains(&name.as_ref())
            })
            .build();
        let mut config_files = vec![];
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    // Skip unreadable paths (permission denied, broken symlinks,
                    // etc.) so one bad directory doesn't abort the whole scan.
                    warn!("trust --all: skipping unreadable path: {err}");
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|ft| ft.is_dir()) {
                continue;
            }
            let dir = entry.path();
            if dir == cwd {
                continue; // already covered by the parent walk
            }
            for p in config::config_paths_in_dir(dir) {
                if !is_global_config(&p) {
                    config_files.push(p);
                }
            }
        }
        // Keep one config file per untrusted trust root.
        let mut seen = HashSet::new();
        config_files
            .into_iter()
            .filter(|p| {
                let ctr = config_trust_root(p);
                !config_file::is_trusted(&ctr) && seen.insert(ctr)
            })
            .collect()
    }

    fn show(&self) -> Result<()> {
        let trusted = config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
            .into_iter()
            .filter(|p| !is_global_config(p))
            .map(|p| config_trust_root(&p))
            .unique()
            .map(|p| (display_path(&p), config_file::is_trusted(&p)))
            .rev()
            .collect::<Vec<_>>();
        if trusted.is_empty() {
            info!("No trusted config files found.");
        }
        for (dp, trusted) in trusted {
            if trusted {
                miseprintln!("{dp}: trusted");
            } else {
                miseprintln!("{dp}: untrusted");
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # trusts ~/some_dir/mise.toml
    $ <bold>mise trust ~/some_dir/mise.toml</bold>

    # trusts mise.toml in the current or parent directory
    $ <bold>mise trust</bold>
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_does_not_exist_is_refused_and_named() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("mise.toml");
        std::fs::write(&existing, "").unwrap();

        // Control: the same call resolves rather than erroring when the path is there, so the
        // assertion below is about existence and not about a function that always fails.
        assert_eq!(
            resolve_config_file(Some(&existing)).unwrap(),
            Some(existing.clone())
        );

        let missing = dir.path().join("nope");
        let err = resolve_config_file(Some(&missing)).unwrap_err().to_string();
        // The path is the whole point of the message: what used to happen instead was that this
        // resolved to `dir` and mise trusted that, reporting success.
        assert!(
            err.contains("nope"),
            "the message has to name the path: {err}"
        );
    }

    #[test]
    fn a_directory_with_no_config_in_it_yet_still_resolves() {
        // The case the existence check must not break. Trusting a project before its `mise.toml`
        // exists is a real thing to want, and it works because the trust root is the directory —
        // which is exactly why "the path must exist" cannot be tightened to "the file must exist".
        let dir = tempfile::tempdir().unwrap();
        let resolved = resolve_config_file(Some(&dir.path().to_path_buf()))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.parent(), Some(dir.path()));
    }

    #[test]
    fn no_argument_is_still_no_argument() {
        // `mise trust` with no path falls back to config discovery further up; this must stay a
        // `None` rather than becoming an error.
        assert_eq!(resolve_config_file(None).unwrap(), None);
    }

    /// Whether the directory entry is there, without following it.
    ///
    /// `Path::exists` answers about the *target*, and a unix entry is a symlink: once its target
    /// is deleted it reports `false` whether or not anything removed the entry. Every "was
    /// removed" assertion below would then pass on unix without testing what it names, leaving
    /// only Windows — where the entry is a plain file — actually checking anything.
    fn entry_present(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    /// Entries go in through `file::make_symlink_or_file`, the same writer `config_file::trust`
    /// uses, so each platform is tested in the form it actually writes: a symlink on unix, a plain
    /// file holding the path on Windows. Going through it is what keeps this meaningful on both —
    /// the Windows form is the one that always `exists()` and so was never pruned.
    #[test]
    fn entries_are_pruned_by_what_they_point_at_not_by_their_own_existence() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("trusted-configs");
        std::fs::create_dir_all(&store).unwrap();

        let live = tmp.path().join("live-project");
        std::fs::create_dir_all(&live).unwrap();
        let gone = tmp.path().join("gone-project");
        std::fs::create_dir_all(&gone).unwrap();

        file::make_symlink_or_file(&live, &store.join("live")).unwrap();
        file::make_symlink_or_file(&gone, &store.join("gone")).unwrap();
        std::fs::remove_dir_all(&gone).unwrap();

        Trust::clean_in(&store).unwrap();

        // The entry whose target is gone goes...
        assert!(
            !entry_present(&store.join("gone")),
            "an entry pointing at a deleted project has to be removed"
        );
        // ...and the one still pointing somewhere stays. Without this a `clean` that deleted
        // everything would pass just as well.
        assert!(
            entry_present(&store.join("live")),
            "an entry pointing at a live project has to survive"
        );
    }

    /// A trust root is a directory — `mise trust ./mise.toml` records the directory that contains
    /// it. Reusing `Tracker::clean_in`'s `is_file()` here would therefore delete every entry, so
    /// the difference is pinned rather than left to a comment.
    #[test]
    fn a_directory_target_counts_as_present() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("trusted-configs");
        std::fs::create_dir_all(&store).unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        file::make_symlink_or_file(&project, &store.join("dir-target")).unwrap();
        Trust::clean_in(&store).unwrap();

        assert!(entry_present(&store.join("dir-target")));
    }

    #[test]
    fn a_store_that_was_never_created_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        Trust::clean_in(&tmp.path().join("never-made")).unwrap();
    }

    /// A trust entry can have two files beside it: `.hash`, the content-bound trust paranoid mode
    /// records, and `.monorepo`, the marker that lets descendants inherit trust. Neither holds a
    /// path — one holds a checksum and the other is empty — so resolving them as if they were
    /// entries made every `mise prune --configs` delete them while the project was still there.
    #[test]
    fn metadata_beside_a_live_entry_is_not_mistaken_for_one() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("trusted-configs");
        std::fs::create_dir_all(&store).unwrap();
        let live = tmp.path().join("live-project");
        std::fs::create_dir_all(&live).unwrap();

        let entry = store.join("live");
        file::make_symlink_or_file(&live, &entry).unwrap();
        let hash = config_file::with_appended_extension(&entry, "hash");
        std::fs::write(&hash, "0123456789abcdef").unwrap();
        let monorepo = config_file::with_appended_extension(&entry, "monorepo");
        std::fs::write(&monorepo, "").unwrap();

        Trust::clean_in(&store).unwrap();

        assert!(
            entry_present(&entry),
            "the entry itself still points somewhere"
        );
        assert!(
            entry_present(&hash),
            "a paranoid trust hash is not an entry"
        );
        assert!(
            entry_present(&monorepo),
            "a monorepo marker is not an entry"
        );
    }

    #[test]
    fn a_removed_entry_takes_its_metadata_with_it() {
        // The other side of the check above: skipping metadata must not turn it into litter that
        // outlives the entry it describes. `config_file::untrust` removes all three together.
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("trusted-configs");
        std::fs::create_dir_all(&store).unwrap();
        let gone = tmp.path().join("gone-project");
        std::fs::create_dir_all(&gone).unwrap();

        let entry = store.join("gone");
        file::make_symlink_or_file(&gone, &entry).unwrap();
        let hash = config_file::with_appended_extension(&entry, "hash");
        std::fs::write(&hash, "0123456789abcdef").unwrap();
        let monorepo = config_file::with_appended_extension(&entry, "monorepo");
        std::fs::write(&monorepo, "").unwrap();
        std::fs::remove_dir_all(&gone).unwrap();

        Trust::clean_in(&store).unwrap();

        assert!(!entry_present(&entry));
        assert!(!entry_present(&hash));
        assert!(!entry_present(&monorepo));
    }
}
