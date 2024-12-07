use crate::cmd::cmd;
use crate::config::{Config, SETTINGS};
use crate::dirs;
use crate::toolset::Toolset;
use eyre::Result;
use globset::{Glob, GlobSetBuilder};
use itertools::Itertools;
use std::collections::BTreeSet;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, Ord, PartialOrd, Eq, PartialEq, Hash,
)]
pub struct WatchFile {
    pub patterns: Vec<String>,
    pub run: String,
}

pub static MODIFIED_FILES: Mutex<Option<BTreeSet<PathBuf>>> = Mutex::new(None);

pub fn add_modified_file(file: PathBuf) {
    let mut mu = MODIFIED_FILES.lock().unwrap();
    let set = mu.get_or_insert_with(BTreeSet::new);
    set.insert(file);
}

pub fn execute_runs(ts: &Toolset) {
    let mut mu = MODIFIED_FILES.lock().unwrap();
    let files = mu.take().unwrap_or_default();
    if files.is_empty() {
        return;
    }
    for (root, wf) in Config::get().watch_file_hooks().unwrap_or_default() {
        match has_matching_files(&root, &wf, &files) {
            Ok(files) if files.is_empty() => {
                continue;
            }
            Ok(files) => {
                if let Err(e) = execute(ts, &root, &wf.run, files) {
                    warn!("error executing watch_file hook: {e}");
                }
            }
            Err(e) => {
                warn!("error matching files: {e}");
            }
        }
    }
}

fn execute(ts: &Toolset, root: &Path, run: &str, files: Vec<&PathBuf>) -> Result<()> {
    SETTINGS.ensure_experimental("watch_file_hooks")?;
    let modified_files_var = files
        .iter()
        .map(|f| f.to_string_lossy().replace(':', "\\:"))
        .join(":");
    #[cfg(unix)]
    let shell = shell_words::split(&SETTINGS.unix_default_inline_shell_args)?;
    #[cfg(windows)]
    let shell = shell_words::split(&SETTINGS.windows_default_inline_shell_args)?;

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(run))
        .collect_vec();
    let mut env = ts.full_env()?;
    env.insert("MISE_WATCH_FILES_MODIFIED".to_string(), modified_files_var);
    if let Some(cwd) = &*dirs::CWD {
        env.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    env.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    // TODO: this should be different but I don't have easy access to it
    // env.insert("MISE_CONFIG_ROOT".to_string(), root.to_string_lossy().to_string());
    cmd(&shell[0], args)
        .stdout_to_stderr()
        // .dir(root)
        .full_env(env)
        .run()?;
    Ok(())
}

fn has_matching_files<'a>(
    root: &Path,
    wf: &'a WatchFile,
    files: &'a BTreeSet<PathBuf>,
) -> Result<Vec<&'a PathBuf>> {
    let mut glob = GlobSetBuilder::new();
    for pattern in &wf.patterns {
        match Glob::new(pattern) {
            Ok(g) => {
                glob.add(g);
            }
            Err(e) => {
                warn!("invalid glob pattern: {e}");
            }
        }
    }
    let glob = glob.build()?;
    Ok(files
        .iter()
        .filter(|file| {
            if let Ok(rel) = file.strip_prefix(root) {
                !glob.matches(rel).is_empty()
            } else {
                false
            }
        })
        .collect())
}
