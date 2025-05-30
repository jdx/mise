use crate::cmd::cmd;
use crate::config::{Config, Settings};
use crate::dirs;
use crate::toolset::Toolset;
use eyre::Result;
use globset::{GlobBuilder, GlobSetBuilder};
use itertools::Itertools;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{collections::BTreeSet, sync::Arc};

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

pub async fn execute_runs(config: &Arc<Config>, ts: &Toolset) {
    let files = {
        let mut mu = MODIFIED_FILES.lock().unwrap();
        mu.take().unwrap_or_default()
    };
    if files.is_empty() {
        return;
    }
    for (root, wf) in config.watch_file_hooks().unwrap_or_default() {
        match has_matching_files(&root, &wf, &files) {
            Ok(files) if files.is_empty() => {
                continue;
            }
            Ok(files) => {
                if let Err(e) = execute(config, ts, &root, &wf.run, files).await {
                    warn!("error executing watch_file hook: {e}");
                }
            }
            Err(e) => {
                warn!("error matching files: {e}");
            }
        }
    }
}

async fn execute(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    run: &str,
    files: Vec<&PathBuf>,
) -> Result<()> {
    Settings::get().ensure_experimental("watch_file_hooks")?;
    let modified_files_var = files
        .iter()
        .map(|f| f.to_string_lossy().replace(':', "\\:"))
        .join(":");
    let shell = Settings::get().default_inline_shell()?;

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(run))
        .collect_vec();
    let mut env = ts.full_env(config).await?;
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
        match GlobBuilder::new(pattern).literal_separator(true).build() {
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

pub fn glob(root: &Path, patterns: &[String]) -> Result<Vec<PathBuf>> {
    if patterns.is_empty() {
        return Ok(vec![]);
    }
    let opts = glob::MatchOptions {
        require_literal_separator: true,
        ..Default::default()
    };
    Ok(patterns
        .iter()
        .map(|pattern| root.join(pattern).to_string_lossy().to_string())
        .filter_map(|pattern| glob::glob_with(&pattern, opts).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|paths| paths.filter_map(|p| p.ok()))
        .collect())

    // let mut overrides = ignore::overrides::OverrideBuilder::new(root);
    // for pattern in patterns {
    //     overrides.add(&format!("./{pattern}"))?;
    // }
    // let files = Arc::new(Mutex::new(vec![]));
    // ignore::WalkBuilder::new(root)
    //     .overrides(overrides.build()?)
    //     .standard_filters(false)
    //     .follow_links(true)
    //     .build_parallel()
    //     .run(|| {
    //         let files = files.clone();
    //         Box::new(move |entry| {
    //             if let Ok(entry) = entry {
    //                 let mut files = files.lock().unwrap();
    //                 files.push(entry.path().to_path_buf());
    //             }
    //             WalkState::Continue
    //         })
    //     });
    //
    // let files = files.lock().unwrap();
    // Ok(files.to_vec())
}
