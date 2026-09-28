use crate::backend::Backend;
use crate::config::{self, Config, Settings, load_command_wrappers};
use crate::file;
use crate::hash::hash_to_str;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::{dirs, env, shims};
use eyre::{Result, WrapErr, bail, eyre};
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub const SHIM_PROTOCOL: &str = "2";

pub struct Bootstrap {
    pub paths: Vec<PathBuf>,
    pub command_names: BTreeSet<String>,
    project_paths: Vec<PathBuf>,
    config_fingerprints: Vec<(PathBuf, String)>,
}

pub struct Selection {
    pub executable: PathBuf,
    pub directory: PathBuf,
}

impl Bootstrap {
    pub fn prepare(&self) -> Result<Selection> {
        let executable = select_mise(&self.project_paths)?;
        if executable.parent().is_some_and(file::is_mise_shims_dir)
            || file::is_active_mise_shim(&executable)
        {
            bail!("mise: recursive checkout shim selection");
        }
        let directory = file::lookup_shims_root().join(fingerprint(self, &executable)?);
        prepare_shims(&executable, &directory)?;
        Ok(Selection {
            executable,
            directory,
        })
    }
}

pub fn was_active() -> bool {
    std::env::var_os("__MISE_LOOKUP_EXE").is_some()
        || env::__MISE_DIFF
            .path
            .iter()
            .any(|path| file::is_lookup_shims_dir(path))
}

pub fn bootstrap() -> Result<Bootstrap> {
    let mut paths = Vec::new();
    let mut project_paths = Vec::new();
    let mut config_fingerprints = Vec::new();
    let mut command_names = BTreeSet::new();
    for path in config::load_config_paths(&config::DEFAULT_CONFIG_FILENAMES, false) {
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let project = !config::is_global_config(&path);
        if project && !config::config_file::is_path_trusted(&path) {
            bail!(
                "mise: {} is not trusted; run `mise trust` to enable it",
                path.display()
            );
        }
        let body = file::read_to_string(&path)?;
        let value: toml::Value = toml::from_str(&body)
            .map_err(|err| eyre!("mise: cannot read {}: {err}", path.display()))?;
        config_fingerprints.push((path.clone(), hash_to_str(&body)));
        command_names.extend(declared_command_names(&value));
        let Some(raw) = value
            .get("env")
            .and_then(|env| env.get("_"))
            .and_then(|env| env.get("path"))
        else {
            continue;
        };
        let entries = match raw {
            toml::Value::String(value) => vec![value.as_str()],
            toml::Value::Array(values) => values
                .iter()
                .map(|value| {
                    value.as_str().ok_or_else(|| {
                        eyre!(
                            "mise: env._.path in {} must contain strings",
                            path.display()
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            _ => bail!(
                "mise: env._.path in {} must be a string or an array of strings",
                path.display()
            ),
        };
        let root = config::config_file::config_root::config_root(&path);
        for entry in entries {
            let expanded = entry.replace("{{config_root}}", &root.to_string_lossy());
            if expanded.contains("{{") || expanded.contains("{%") {
                bail!(
                    "mise: env._.path in {} uses a dynamic template that env_path activation cannot evaluate",
                    path.display()
                );
            }
            let expanded = file::replace_path(PathBuf::from(expanded));
            let expanded = if expanded.is_absolute() {
                expanded
            } else {
                root.join(expanded)
            };
            if project {
                project_paths.push(expanded.clone());
            }
            paths.push(expanded);
        }
    }
    Ok(Bootstrap {
        paths,
        project_paths,
        config_fingerprints,
        command_names,
    })
}

fn select_mise(paths: &[PathBuf]) -> Result<PathBuf> {
    if paths.is_empty() {
        bail!("mise: activate_mise_lookup=env_path requires a trusted project env._.path entry");
    }
    let name = if cfg!(windows) { "mise.exe" } else { "mise" };
    paths
        .iter()
        .map(|path| path.join(name))
        .find(|path| path.is_file() && file::is_executable(path))
        .ok_or_else(|| {
            eyre!("mise: no executable {name} found in trusted project env._.path directories")
        })
}

fn fingerprint(bootstrap: &Bootstrap, selected: &Path) -> Result<String> {
    let mut inputs = Vec::new();
    for path in [selected, &*env::MISE_BIN] {
        let metadata = std::fs::metadata(path)?;
        let body = if metadata.len() <= 1_000_000 {
            std::fs::read(path).ok()
        } else {
            None
        };
        inputs.push((
            path.to_path_buf(),
            metadata.len(),
            metadata.modified().ok(),
            body,
        ));
    }
    // Windows DotSlash launchers read their adjacent extensionless manifest on each invocation.
    let manifest = if cfg!(windows) {
        std::fs::read(selected.with_extension("")).ok()
    } else {
        None
    };
    let generation = std::fs::read(file::lookup_shims_root().join(".generation")).ok();
    Ok(hash_to_str(&(
        SHIM_PROTOCOL,
        &bootstrap.config_fingerprints,
        inputs,
        manifest,
        generation,
    )))
}

fn prepare_shims(selected: &Path, directory: &Path) -> Result<()> {
    let _lock = crate::lock_file::LockFile::new(directory).lock()?;
    if repair_cached_shims(directory)? {
        return Ok(());
    }
    let output = Command::new(selected)
        .arg("__lookup-shim-names")
        .arg(SHIM_PROTOCOL)
        .output()
        .map_err(|err| eyre!("mise: failed to ask selected mise for shim names: {err}"))?;
    if !output.status.success() {
        bail!(
            "mise: selected mise cannot prepare checkout shims; update the pinned mise version. {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let names: BTreeSet<String> = serde_json::from_slice(&output.stdout)
        .map_err(|err| eyre!("mise: selected mise cannot prepare checkout shims; update the pinned mise version. Invalid response: {err}"))?;
    publish_shims(directory, &names)
}

pub fn failure_shims(names: Option<&BTreeSet<String>>) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if let Some(names) = names {
        paths.push(prepare_declared_shims(names)?);
    }
    for directory in &env::__MISE_DIFF.path {
        if file::is_lookup_shims_dir(directory) {
            let _lock = crate::lock_file::LockFile::new(directory).lock()?;
            repair_cached_shims(directory)?;
            paths.push(directory.clone());
        }
    }
    Ok(paths)
}

fn prepare_declared_shims(names: &BTreeSet<String>) -> Result<PathBuf> {
    let directory = file::lookup_shims_root().join(hash_to_str(&(
        "declared",
        SHIM_PROTOCOL,
        &*env::MISE_BIN,
        names,
    )));
    let _lock = crate::lock_file::LockFile::new(&directory).lock()?;
    publish_shims(&directory, names)?;
    Ok(directory)
}

fn publish_shims(directory: &Path, names: &BTreeSet<String>) -> Result<()> {
    reconcile_shims(directory, names)?;
    std::fs::write(directory.join(".complete"), serde_json::to_vec(names)?)?;
    Ok(())
}

fn repair_cached_shims(directory: &Path) -> Result<bool> {
    let Some(names) = std::fs::read(directory.join(".complete"))
        .ok()
        .and_then(|body| serde_json::from_slice::<BTreeSet<String>>(&body).ok())
    else {
        return Ok(false);
    };
    reconcile_shims(directory, &names)?;
    Ok(true)
}

fn reconcile_shims(directory: &Path, names: &BTreeSet<String>) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let host = &*env::MISE_BIN;
    #[cfg(windows)]
    let launcher = lookup_shim_binary(host)?;
    #[cfg(windows)]
    let launcher_size = std::fs::metadata(&launcher)?.len();
    for name in names
        .iter()
        .map(String::as_str)
        .chain(std::iter::once("mise"))
    {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
            bail!("mise: selected mise returned an invalid shim name: {name}");
        }
        #[cfg(unix)]
        if !shim_is_intact(directory, host, name) {
            write_shim(directory, host, name)?;
        }
        #[cfg(windows)]
        if !shim_is_intact(directory, host, name, launcher_size) {
            write_shim(directory, host, name, &launcher)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn shim_body(host: &Path, name: &str) -> String {
    let host = host.to_string_lossy();
    let quoted_host = shell_words::quote(&host);
    let quoted_name = shell_words::quote(name);
    format!(
        "#!/bin/sh\nif [ \"${{__MISE_SHIM_PATH:-}}\" = \"$0\" ]; then echo 'mise: recursive lookup shim' >&2; exit 1; fi\nexport __MISE_SHIM_PATH=\"$0\"\nexec {quoted_host} __lookup-dispatch {quoted_name} \"$@\"\n"
    )
}

#[cfg(unix)]
fn shim_is_intact(directory: &Path, host: &Path, name: &str) -> bool {
    let path = directory.join(name);
    file::is_executable(&path)
        && std::fs::read_to_string(&path).is_ok_and(|body| body == shim_body(host, name))
}

#[cfg(windows)]
fn shim_is_intact(directory: &Path, host: &Path, name: &str, launcher_size: u64) -> bool {
    let path = directory.join(format!("{name}.exe"));
    std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() == launcher_size)
        && std::fs::read_to_string(path.with_extension("lookup"))
            .is_ok_and(|body| body == host.to_string_lossy())
}

#[cfg(unix)]
fn write_shim(directory: &Path, host: &Path, name: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join(name);
    std::fs::write(&path, shim_body(host, name))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(windows)]
fn write_shim(directory: &Path, host: &Path, name: &str, launcher: &Path) -> Result<()> {
    let path = directory.join(format!("{name}.exe"));
    std::fs::copy(launcher, &path)?;
    std::fs::write(
        path.with_extension("lookup"),
        host.to_string_lossy().as_bytes(),
    )?;
    Ok(())
}

#[cfg(windows)]
fn lookup_shim_binary(host: &Path) -> Result<PathBuf> {
    let host = dunce::canonicalize(host).unwrap_or_else(|_| host.to_path_buf());
    let launcher = host
        .parent()
        .ok_or_else(|| eyre!("mise executable has no parent directory"))?
        .join("mise-shim.exe");
    if !launcher.is_file() {
        bail!("mise-shim.exe not found beside {}", host.display());
    }
    Ok(launcher)
}

pub async fn command_names(
    config: &Arc<Config>,
    toolset: Option<&Toolset>,
) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    if let Some(toolset) = toolset {
        for (backend, version) in toolset.list_current_installed_versions(config) {
            names.extend(known_tool_bins(config, backend, &version).await?);
        }
    }
    let requests = config.get_tool_request_set().await?;
    for request in requests.tools.values().flatten() {
        names.extend(request.declared_bin_names().map(str::to_owned));
    }
    let wrappers = load_command_wrappers(&config.config_files, requests.tools.values().flatten())?;
    names.extend(wrappers.keys().cloned());
    Ok(filter_command_names(names))
}

/// Collect command names without resolving tool requests or evaluating configuration templates.
fn declared_command_names(value: &toml::Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(tools) = value.get("tools").and_then(toml::Value::as_table) {
        for (name, declaration) in tools {
            if let Some(tool) = crate::registry::REGISTRY.get(name) {
                names.extend(tool.bins.iter().map(|name| (*name).to_string()));
            }
            let versions = declaration
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(declaration));
            for version in versions {
                if let Some(bins) = version.get("lazy_bins").and_then(toml::Value::as_array) {
                    names.extend(
                        bins.iter()
                            .filter_map(toml::Value::as_str)
                            .map(str::to_owned),
                    );
                }
            }
        }
    }
    if let Some(wrappers) = value.get("wrappers").and_then(toml::Value::as_table) {
        names.extend(wrappers.keys().cloned());
    }
    filter_command_names(names)
}

fn filter_command_names(names: BTreeSet<String>) -> BTreeSet<String> {
    names
        .into_iter()
        .map(|name| shims::command_name_key(&name))
        .filter(|name| {
            !name.eq_ignore_ascii_case("mise")
                && !name.is_empty()
                && name != "."
                && name != ".."
                && !name.contains(['/', '\\'])
                && !shims::shim_name_excluded(&Settings::get().shims.exclude, name)
        })
        .collect()
}

pub(crate) async fn known_tool_bins(
    config: &Arc<Config>,
    backend: Arc<dyn Backend>,
    version: &ToolVersion,
) -> Result<BTreeSet<String>> {
    if matches!(version.request, ToolRequest::System { .. }) {
        return Ok(BTreeSet::new());
    }
    // Command ownership must survive cache clearing and damage to the installation.
    let root = dirs::STATE.join("lookup-bins");
    let path = root.join(format!("{}.json", hash_to_str(&version.install_path())));
    let _lock = crate::lock_file::LockFile::new(&path).lock()?;
    let previous: BTreeSet<String> = match std::fs::read(&path) {
        Ok(body) => serde_json::from_slice(&body)
            .wrap_err_with(|| format!("cannot read {}", path.display()))?,
        Err(err) if err.kind() == ErrorKind::NotFound => BTreeSet::new(),
        Err(err) => return Err(err).wrap_err_with(|| format!("cannot read {}", path.display())),
    };
    let mut names = previous.clone();
    if backend.is_version_installed(config, version, true) {
        names.extend(
            shims::list_tool_bins(config, backend, version)
                .await?
                .into_iter()
                .map(|name| shims::command_name_key(&name)),
        );
    }
    if names != previous {
        file::create_dir_all(&root)?;
        std::fs::write(path, serde_json::to_vec(&names)?)?;
    }
    Ok(names)
}

pub(crate) async fn tools_changed(config: &Arc<Config>, versions: &[ToolVersion]) -> Result<()> {
    if Settings::get().activate_mise_lookup == "env_path" {
        for version in versions {
            known_tool_bins(config, version.backend()?, version).await?;
        }
    }
    invalidate_shims()
}

pub fn invalidate_shims() -> Result<()> {
    if Settings::get().activate_mise_lookup != "env_path" {
        return Ok(());
    }
    let root = file::lookup_shims_root();
    std::fs::create_dir_all(&root)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    std::fs::write(root.join(".generation"), stamp.to_string())?;
    Ok(())
}
