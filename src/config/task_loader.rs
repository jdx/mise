//! Task loading functionality for mise configuration
//!
//! This module handles discovering and loading tasks from:
//! - Config files (mise.toml `[tasks]` sections)
//! - Task directories (mise-tasks/, .mise-tasks/, etc.)
//! - Monorepo subdirectories (when experimental_monorepo_root is enabled)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use walkdir::WalkDir;

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::Tasks;
use crate::config::settings::Settings;
use crate::config::{Config, DEFAULT_CONFIG_FILENAMES, config_file, is_global_config};
use crate::file::{self, display_path};
use crate::task::Task;

type ConfigMap = IndexMap<PathBuf, Arc<dyn ConfigFile>>;

/// Default directories to search for task files
pub fn default_task_includes() -> Vec<PathBuf> {
    vec![
        PathBuf::from("mise-tasks"),
        PathBuf::from(".mise-tasks"),
        PathBuf::from(".mise").join("tasks"),
        PathBuf::from(".config").join("mise").join("tasks"),
        PathBuf::from("mise").join("tasks"),
    ]
}

/// Load all tasks for a config, optionally with a specific context
pub async fn load_all_tasks_with_context(
    config: &Arc<Config>,
    ctx: Option<&crate::task::TaskLoadContext>,
) -> Result<BTreeMap<String, Task>> {
    time!("load_all_tasks");
    let local_tasks = load_local_tasks_with_context(&config, ctx).await?;
    let global_tasks = load_global_tasks(&config).await?;
    let mut tasks: BTreeMap<String, Task> = local_tasks
        .into_iter()
        .chain(global_tasks)
        .rev()
        .inspect(|t| {
            trace!(
                "loaded task {} – {}",
                &t.name,
                display_path(&t.config_source)
            )
        })
        .map(|t| (t.name.clone(), t))
        .collect();
    let all_tasks = tasks.clone();
    for task in tasks.values_mut() {
        task.display_name = task.display_name(&all_tasks);
    }
    time!("load_all_tasks {count}", count = tasks.len(),);
    Ok(tasks)
}

/// Load tasks from local (non-global) config files
pub async fn load_local_tasks_with_context(
    config: &Arc<Config>,
    ctx: Option<&crate::task::TaskLoadContext>,
) -> Result<Vec<Task>> {
    use crate::config::find_monorepo_root;
    use crate::dirs;

    let mut tasks = vec![];
    let monorepo_root = find_monorepo_root(&config.config_files);

    // Load tasks from parent directories (current working directory up to root)
    let local_config_files = config
        .config_files
        .iter()
        .filter(|(_, cf)| !is_global_config(cf.get_path()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<IndexMap<_, _>>();

    for d in crate::config::all_dirs()? {
        if cfg!(test) && !d.starts_with(*dirs::HOME) {
            continue;
        }
        let mut dir_tasks = load_tasks_in_dir(config, &d, &local_config_files).await?;

        if let Some(ref monorepo_root) = monorepo_root {
            prefix_monorepo_task_names(&mut dir_tasks, &d, monorepo_root);
        }

        tasks.extend(dir_tasks);
    }

    // Determine if we should load monorepo tasks from subdirectories
    let should_load_subdirs = ctx.is_some_and(|c| c.load_all || !c.path_hints.is_empty());

    // If in a monorepo, also discover and load tasks from subdirectories
    if let Some(monorepo_root) = &monorepo_root {
        if !should_load_subdirs {
            return Ok(tasks);
        }

        let subdirs = discover_monorepo_subdirs(monorepo_root, ctx)?;

        // Load tasks from subdirectories in parallel
        let subdir_tasks_futures: Vec<_> = subdirs
            .into_iter()
            .filter(|subdir| !cfg!(test) || subdir.starts_with(*dirs::HOME))
            .map(|subdir| {
                let config = config.clone();
                let monorepo_root = monorepo_root.clone();
                async move {
                    let mut all_tasks = Vec::new();
                    for config_filename in DEFAULT_CONFIG_FILENAMES.iter() {
                        let config_path = subdir.join(config_filename);
                        if config_path.exists() {
                            match config_file::parse(&config_path).await {
                                Ok(cf) => {
                                    let mut subdir_tasks =
                                        load_config_and_file_tasks(&config, cf.clone()).await?;

                                    prefix_monorepo_task_names(
                                        &mut subdir_tasks,
                                        &subdir,
                                        &monorepo_root,
                                    );
                                    for task in subdir_tasks.iter_mut() {
                                        task.cf = Some(cf.clone());
                                    }

                                    all_tasks.extend(subdir_tasks);
                                }
                                Err(err) => {
                                    let rel_path = subdir
                                        .strip_prefix(&monorepo_root)
                                        .unwrap_or(&subdir);
                                    warn!(
                                        "Failed to parse config file {} in monorepo subdirectory {}: {}. Tasks from this directory will not be loaded.",
                                        config_path.display(),
                                        rel_path.display(),
                                        err
                                    );
                                }
                            }
                        }
                    }
                    Ok::<Vec<Task>, eyre::Report>(all_tasks)
                }
            })
            .collect();

        use tokio::task::JoinSet;
        let mut join_set = JoinSet::new();
        for future in subdir_tasks_futures {
            join_set.spawn(future);
        }

        while let Some(result) = join_set.join_next().await {
            tasks.extend(result??);
        }
    }

    Ok(tasks)
}

/// Load tasks from global config files
pub async fn load_global_tasks(config: &Arc<Config>) -> Result<Vec<Task>> {
    let config_files = config
        .config_files
        .values()
        .filter(|cf| is_global_config(cf.get_path()))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    for cf in config_files {
        tasks.extend(load_config_and_file_tasks(config, cf.clone()).await?);
    }
    Ok(tasks)
}

/// Load tasks from both config file [tasks] section and task directories
pub async fn load_config_and_file_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
) -> Result<Vec<Task>> {
    let config_root = cf.config_root();
    let tasks = load_config_tasks(config, cf.clone(), &config_root).await?;
    let file_tasks = load_file_tasks(config, cf.clone(), &config_root).await?;
    Ok(tasks.into_iter().chain(file_tasks).collect())
}

/// Load tasks defined in the [tasks] section of a config file
pub async fn load_config_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    config_root: &Path,
) -> Result<Vec<Task>> {
    let is_global = is_global_config(cf.get_path());
    let config_root = Arc::new(config_root.to_path_buf());
    let mut tasks = vec![];
    for t in cf.tasks().into_iter() {
        let config_root = config_root.clone();
        let config = config.clone();
        let mut t = t.clone();
        if is_global {
            t.global = true;
        }
        match t.render(&config, &config_root).await {
            Ok(()) => {
                tasks.push(t);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
    Ok(tasks)
}

/// Load tasks from task include directories (mise-tasks/, etc.)
pub async fn load_file_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    config_root: &Path,
) -> Result<Vec<Task>> {
    let includes = cf
        .task_config()
        .includes
        .clone()
        .unwrap_or_else(default_task_includes)
        .into_iter()
        .map(|p| cf.get_path().parent().unwrap().join(p))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    let config_root = Arc::new(config_root.to_path_buf());
    for p in includes {
        let config_root = config_root.clone();
        let config = config.clone();
        tasks.extend(load_tasks_includes(&config, &p, &config_root).await?);
    }
    Ok(tasks)
}

/// Load tasks from an include path (file or directory)
pub async fn load_tasks_includes(
    config: &Arc<Config>,
    root: &Path,
    config_root: &Path,
) -> Result<Vec<Task>> {
    if root.is_file() && root.extension().map(|e| e == "toml").unwrap_or(false) {
        load_task_file(config, root, config_root).await
    } else if root.is_dir() {
        let files = WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| e.path() == root || !e.file_name().to_string_lossy().starts_with('.'))
            .filter_ok(|e| e.file_type().is_file())
            .map_ok(|e| e.path().to_path_buf())
            .try_collect::<_, Vec<PathBuf>, _>()?
            .into_iter()
            .filter(|p| file::is_executable(p))
            .filter(|p| {
                !Settings::get()
                    .task_disable_paths
                    .iter()
                    .any(|d| p.starts_with(d))
            })
            .collect::<Vec<_>>();
        let mut tasks = vec![];
        let root = Arc::new(root.to_path_buf());
        let config_root = Arc::new(config_root.to_path_buf());
        for path in files {
            let root = root.clone();
            let config_root = config_root.clone();
            let config = config.clone();
            tasks.push(Task::from_path(&config, &path, &root, &config_root).await?);
        }
        Ok(tasks)
    } else {
        Ok(vec![])
    }
}

/// Load a single TOML task file
pub async fn load_task_file(
    config: &Arc<Config>,
    path: &Path,
    config_root: &Path,
) -> Result<Vec<Task>> {
    let raw = file::read_to_string_async(path).await?;
    let mut tasks = toml::from_str::<Tasks>(&raw)
        .wrap_err_with(|| format!("Error parsing task file: {}", display_path(path)))?
        .0;
    for (name, task) in &mut tasks {
        task.name = name.clone();
        task.config_source = path.to_path_buf();
        task.config_root = Some(config_root.to_path_buf());
    }
    let mut out = vec![];
    for (_, mut task) in tasks {
        let config_root = config_root.to_path_buf();
        if let Err(err) = task.render(config, &config_root).await {
            warn!("rendering task: {err:?}");
        }
        out.push(task);
    }
    Ok(out)
}

/// Load tasks in a specific directory
pub async fn load_tasks_in_dir(
    config: &Arc<Config>,
    dir: &Path,
    config_files: &ConfigMap,
) -> Result<Vec<Task>> {
    let configs = configs_at_root(dir, config_files);
    let mut config_tasks = vec![];
    for cf in configs {
        let dir = dir.to_path_buf();
        config_tasks.extend(load_config_tasks(config, cf.clone(), &dir).await?);
    }
    let mut file_tasks = vec![];
    for p in task_includes_for_dir(dir, config_files) {
        file_tasks.extend(load_tasks_includes(config, &p, dir).await?);
    }
    let mut tasks = file_tasks
        .into_iter()
        .chain(config_tasks)
        .sorted_by_cached_key(|t| t.name.clone())
        .collect::<Vec<_>>();
    let all_tasks = tasks
        .clone()
        .into_iter()
        .map(|t| (t.name.clone(), t))
        .collect::<BTreeMap<_, _>>();
    for task in tasks.iter_mut() {
        task.display_name = task.display_name(&all_tasks);
    }
    Ok(tasks)
}

/// Get task include paths for a directory
pub fn task_includes_for_dir(dir: &Path, config_files: &ConfigMap) -> Vec<PathBuf> {
    configs_at_root(dir, config_files)
        .iter()
        .rev()
        .find_map(|cf| cf.task_config().includes.clone())
        .unwrap_or_else(default_task_includes)
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { dir.join(p) })
        .filter(|p| p.exists())
        .collect::<Vec<_>>()
        .into_iter()
        .unique()
        .collect::<Vec<_>>()
}

// ========== Monorepo Support ==========

/// Add path prefix to task names for monorepo tasks
fn prefix_monorepo_task_names(tasks: &mut [Task], dir: &Path, monorepo_root: &Path) {
    const MONOREPO_PATH_PREFIX: &str = "//";
    const MONOREPO_TASK_SEPARATOR: &str = ":";

    if let Ok(rel_path) = dir.strip_prefix(monorepo_root) {
        let prefix = rel_path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        for task in tasks.iter_mut() {
            task.name = format!(
                "{}{}{}{}",
                MONOREPO_PATH_PREFIX, prefix, MONOREPO_TASK_SEPARATOR, task.name
            );
        }
    }
}

/// Discover subdirectories in a monorepo that contain mise config files
fn discover_monorepo_subdirs(
    root: &Path,
    ctx: Option<&crate::task::TaskLoadContext>,
) -> Result<Vec<PathBuf>> {
    const DEFAULT_IGNORED_DIRS: &[&str] = &["node_modules", "target", "dist", "build"];

    let mut subdirs = Vec::new();
    let settings = Settings::get();
    let respect_gitignore = settings.task.monorepo_respect_gitignore;
    let max_depth = settings.task.monorepo_depth as usize;

    let excluded_dirs: Vec<&str> = if settings.task.monorepo_exclude_dirs.is_empty() {
        DEFAULT_IGNORED_DIRS.to_vec()
    } else {
        settings
            .task
            .monorepo_exclude_dirs
            .iter()
            .map(|s| s.as_str())
            .collect()
    };

    if respect_gitignore {
        let walker = ignore::WalkBuilder::new(root)
            .max_depth(Some(max_depth))
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .build();

        for entry in walker {
            let entry = entry?;
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                let dir = entry.path();

                if dir == root {
                    continue;
                }

                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if excluded_dirs.contains(&name) {
                    continue;
                }

                let has_config = DEFAULT_CONFIG_FILENAMES
                    .iter()
                    .any(|f| dir.join(f).exists());
                if has_config {
                    if let Some(ctx) = ctx {
                        let rel_path = dir
                            .strip_prefix(root)
                            .ok()
                            .and_then(|p| p.to_str())
                            .unwrap_or("");
                        if ctx.should_load_subdir(rel_path, root.to_str().unwrap_or("")) {
                            subdirs.push(dir.to_path_buf());
                        }
                    } else {
                        subdirs.push(dir.to_path_buf());
                    }
                }
            }
        }
    } else {
        for entry in WalkDir::new(root)
            .min_depth(1)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && !excluded_dirs.contains(&name.as_ref())
            })
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                let dir = entry.path();
                let has_config = DEFAULT_CONFIG_FILENAMES
                    .iter()
                    .any(|f| dir.join(f).exists());
                if has_config {
                    if let Some(ctx) = ctx {
                        let rel_path = dir
                            .strip_prefix(root)
                            .ok()
                            .and_then(|p| p.to_str())
                            .unwrap_or("");
                        if ctx.should_load_subdir(rel_path, root.to_str().unwrap_or("")) {
                            subdirs.push(dir.to_path_buf());
                        }
                    } else {
                        subdirs.push(dir.to_path_buf());
                    }
                }
            }
        }
    }

    Ok(subdirs)
}

// ========== Helper Functions ==========

/// Get config files at a specific directory root
fn configs_at_root<'a>(dir: &Path, config_files: &'a ConfigMap) -> Vec<&'a Arc<dyn ConfigFile>> {
    let mut configs: Vec<&'a Arc<dyn ConfigFile>> = DEFAULT_CONFIG_FILENAMES
        .iter()
        .rev()
        .flat_map(|f| {
            if f.contains('*') {
                crate::config::glob(dir, f)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|path| config_files.get(&path))
                    .collect::<Vec<_>>()
            } else {
                config_files
                    .get(&dir.join(f))
                    .into_iter()
                    .collect::<Vec<_>>()
            }
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    configs.retain(|cf| seen.insert(cf.get_path().to_path_buf()));
    configs
}

use eyre::WrapErr;
