use std::fs;
use std::path::Path;

use crate::cli::args::short_to_pathname;
use crate::dirs::*;
use crate::file;
use eyre::Result;

pub async fn run() {
    tokio::join!(
        task(migrate_trusted_configs),
        task(migrate_tracked_configs),
        task(|| remove_deprecated_plugin("node", "rtx-nodejs")),
        task(|| remove_deprecated_plugin("go", "rtx-golang")),
        task(|| remove_deprecated_plugin("java", "rtx-java")),
        task(|| remove_deprecated_plugin("python", "rtx-python")),
        task(|| remove_deprecated_plugin("ruby", "rtx-ruby")),
        task(migrate_flat_tool_dirs),
    );
}

async fn task(job: impl FnOnce() -> Result<()> + Send + 'static) {
    if let Err(err) = job() {
        eprintln!("[WARN] migrate: {err}");
    }
}

fn migrate_tracked_configs() -> Result<()> {
    move_dirs(&DATA.join("tracked_config_files"), &TRACKED_CONFIGS)?;
    move_dirs(&DATA.join("tracked-config-files"), &TRACKED_CONFIGS)?;
    Ok(())
}

fn migrate_trusted_configs() -> Result<()> {
    move_dirs(&CACHE.join("trusted-configs"), &TRUSTED_CONFIGS)?;
    move_dirs(&CONFIG.join("trusted-configs"), &TRUSTED_CONFIGS)?;
    move_dirs(&DATA.join("trusted-configs"), &TRUSTED_CONFIGS)?;
    Ok(())
}

fn move_dirs(from: &Path, to: &Path) -> Result<bool> {
    if from.exists() && !to.exists() {
        eprintln!("migrating {} to {}", from.display(), to.display());
        file::create_dir_all(to.parent().unwrap())?;
        file::rename(from, to)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn remove_deprecated_plugin(name: &str, plugin_name: &str) -> Result<()> {
    let plugin_root = PLUGINS.join(name);
    let gitconfig = plugin_root.join(".git").join("config");
    let gitconfig_body = fs::read_to_string(gitconfig).unwrap_or_default();
    if !gitconfig_body.contains(&format!("github.com/mise-plugins/{plugin_name}")) {
        return Ok(());
    }
    eprintln!("removing deprecated plugin {plugin_name}, will use core {name} plugin from now on");
    file::remove_all(plugin_root)?;
    Ok(())
}

/// Migrate flat kebab-cased tool directories to nested backend/tool structure.
///
/// Old layout: `$INSTALLS/npm-prettier/`, `$CACHE/npm-prettier/`, `$DOWNLOADS/npm-prettier/`
/// New layout: `$INSTALLS/@npm/prettier/`, `$CACHE/@npm/prettier/`, `$DOWNLOADS/@npm/prettier/`
///
/// Uses the manifest (.mise-installs.toml) to identify which flat dirs need migration,
/// since the manifest's `short` field preserves the original `backend:tool` name.
fn migrate_flat_tool_dirs() -> Result<()> {
    use std::collections::BTreeMap;

    let manifest_path = INSTALLS.join(".mise-installs.toml");
    let body = match file::read_to_string(&manifest_path) {
        Ok(body) => body,
        Err(_) => return Ok(()), // No manifest, nothing to migrate
    };
    #[derive(serde::Deserialize, serde::Serialize)]
    struct ManifestTool {
        short: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        full: Option<String>,
        #[serde(default = "default_true")]
        explicit_backend: bool,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        opts: BTreeMap<String, toml::Value>,
    }
    fn default_true() -> bool {
        true
    }
    let manifest: BTreeMap<String, ManifestTool> = match toml::from_str(&body) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    let mut updated = false;
    let mut new_manifest = BTreeMap::new();
    for (key, tool) in &manifest {
        // Check if this is a flat key that should be nested
        let new_pathname = short_to_pathname(&tool.short);
        let new_key = new_pathname.to_string_lossy().replace('\\', "/");

        if *key != new_key && !key.contains('/') && tool.short.contains(':') {
            let old_flat = key.clone();
            for base in [&*CACHE, &*INSTALLS, &*DOWNLOADS] {
                let old_path = base.join(&old_flat);
                let new_path = base.join(&new_pathname);
                if old_path.exists() && !new_path.exists() {
                    eprintln!("migrating {} to {}", old_path.display(), new_path.display());
                    if let Some(parent) = new_path.parent() {
                        file::create_dir_all(parent)?;
                    }
                    file::rename(&old_path, &new_path)?;
                }
            }
            updated = true;
            new_manifest.insert(
                new_key,
                ManifestTool {
                    short: tool.short.clone(),
                    full: tool.full.clone(),
                    explicit_backend: tool.explicit_backend,
                    opts: tool.opts.clone(),
                },
            );
        } else {
            new_manifest.insert(
                key.clone(),
                ManifestTool {
                    short: tool.short.clone(),
                    full: tool.full.clone(),
                    explicit_backend: tool.explicit_backend,
                    opts: tool.opts.clone(),
                },
            );
        }
    }

    if updated {
        let body = toml::to_string_pretty(&new_manifest)?;
        file::write(&manifest_path, body.trim())?;

        // Migrate flat dirs not in the manifest (e.g., cache-only dirs from ls-remote).
        // Only runs when manifest migration happened, since that's the signal that
        // old-format dirs may still exist.
        for base in [&*CACHE, &*DOWNLOADS] {
            migrate_unmanifested_flat_dirs(base)?;
        }
    }

    Ok(())
}

/// Known backend prefixes used in plugin:tool format.
/// Must match the prefixes recognized by `BackendType::guess()`.
const KNOWN_BACKENDS: &[&str] = &[
    "aqua", "asdf", "cargo", "conda", "core", "dotnet", "forgejo", "gem", "github", "gitlab", "go",
    "npm", "pipx", "spm", "http", "s3", "ubi", "vfox",
];

/// Migrate unmanifested flat dirs (e.g., cache from ls-remote) by checking if
/// a corresponding backend container dir already exists from manifest migration
/// and the prefix is a known backend type.
fn migrate_unmanifested_flat_dirs(base: &Path) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }
    let entries: Vec<_> = base
        .read_dir()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.contains('/') {
            continue;
        }
        if let Some(dash_pos) = name.find('-') {
            let backend_part = &name[..dash_pos];
            if !KNOWN_BACKENDS.contains(&backend_part) {
                continue;
            }
            let backend_dir = base.join(format!("@{backend_part}"));
            if backend_dir.is_dir() {
                let tool_part = &name[dash_pos + 1..];
                let new_path = backend_dir.join(tool_part);
                let old_path = entry.path();
                if !new_path.exists() {
                    eprintln!("migrating {} to {}", old_path.display(), new_path.display());
                    file::rename(&old_path, &new_path)?;
                }
            }
        }
    }
    Ok(())
}
