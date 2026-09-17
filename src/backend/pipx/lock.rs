//! Portable, wheel-only Python tool environments. uv owns dependency resolution;
//! mise owns lock persistence, interpreter selection, and executable exposure.
use super::*;
use crate::lockfile::{GraphRef, NativeGraph, UvLock};
use eyre::WrapErr;
use std::path::PathBuf;

const MIN_UV_VERSION: &str = "0.12.10";
const PROJECT_NAME: &str = "mise-pypi-tool-environment";
const DISCOVER_SCRIPTS: &str = r#"
import importlib.metadata
import json
import os
from pathlib import Path
import sys

packages = json.loads(sys.argv[1])
scripts = Path(sys.argv[2]).resolve()
names = set()
for package in packages:
    try:
        distribution = importlib.metadata.distribution(package)
    except importlib.metadata.PackageNotFoundError:
        # Requirements with inactive environment markers are part of the
        # portable lock but are intentionally absent from this environment.
        continue
    names.update(
        f"{entry.name}.exe" if os.name == "nt" else entry.name
        for entry in distribution.entry_points
        if entry.group in ("console_scripts", "gui_scripts")
    )
    names.update(
        path.name
        for file in distribution.files or []
        if (path := Path(distribution.locate_file(file)).resolve()).parent == scripts
        and path.is_file()
    )
print(json.dumps(sorted(names)))
"#;

impl PIPXBackend {
    pub(crate) fn uv_lock_allowed(&self, tv: &ToolVersion) -> bool {
        Settings::get().pypi.uvx != Some(false)
            && !PipxOptions::new(&tv.request.options()).uvx_disabled()
            && matches!(
                self.tool_name().parse::<PipxRequest>(),
                Ok(PipxRequest::Pypi(_))
            )
    }

    pub(super) fn uv_lock_options_supported(&self, tv: &ToolVersion) -> bool {
        let raw = tv.request.options();
        let opts = PipxOptions::new(&raw);
        [opts.uvx_args(), opts.pipx_args()]
            .into_iter()
            .flatten()
            .all(|s| s.trim().is_empty())
    }

    pub(super) fn validate_lock_options(&self, tv: &ToolVersion) -> Result<()> {
        if !self.uv_lock_options_supported(tv) {
            bail!(
                "{} dependency locking does not support uvx_args or pipx_args",
                self.ba.short
            );
        }
        if !self.uv_lock_allowed(tv) {
            bail!(
                "{} has a uv dependency graph; use uv with a PyPI package to replay it",
                self.ba.short
            );
        }
        Ok(())
    }

    async fn lock_uv_program(&self, config: &Arc<Config>) -> Result<PathBuf> {
        let uv = self.spawnable_dependency(config, None, "uv").await
            .ok_or_else(|| eyre!("Python dependency locks require uv >= {MIN_UV_VERSION}; install it with `mise use uv`"))?;
        let output = CmdLineRunner::new(&uv).arg("--version").read().await?;
        let version = output
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| eyre!("cannot determine uv version"))?;
        if semver_is_older_than(version, MIN_UV_VERSION).unwrap_or(true) {
            bail!("Python dependency locks require uv >= {MIN_UV_VERSION}");
        }
        Ok(uv)
    }

    async fn configured_python_identity(&self, config: &Arc<Config>) -> Option<String> {
        let ts = self.dependency_toolset(config).await.ok()?;
        let (_, python) = ts
            .list_current_versions()
            .into_iter()
            .find(|(_, tv)| tv.ba().short == "python")?;
        Some(format!(
            "{}:{}:{}",
            python.ba().full(),
            python.version,
            python.install_path().display()
        ))
    }

    pub(crate) async fn restore_uv_python(&self, config: &Arc<Config>, tv: &mut ToolVersion) {
        if let Some(identity) = self.configured_python_identity(config).await {
            tv.uv_python = Some((PathBuf::new(), identity));
            return;
        }
        Self::restore_system_uv_python(tv);
    }

    fn restore_system_uv_python(tv: &mut ToolVersion) {
        // Installed environments remain usable without rediscovering a system Python.
        let roots = std::iter::once(tv.ba().installs_path.clone()).chain(
            crate::env::shared_install_dirs()
                .into_iter()
                .map(|root| root.join(tv.ba().tool_dir_name())),
        );
        for entry in roots
            .filter_map(|root| std::fs::read_dir(root).ok())
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{}~uv~", tv.version))
            {
                continue;
            }
            let Ok(contents) = crate::file::read_to_string(path.join(".mise-uv/python.json"))
            else {
                continue;
            };
            let Ok(python) = serde_json::from_str::<(PathBuf, String)>(&contents) else {
                continue;
            };
            let mut candidate = tv.clone();
            candidate.uv_python = Some(python);
            if candidate.install_path() == path {
                tv.uv_python = candidate.uv_python;
                return;
            }
        }
    }

    pub(crate) async fn bind_uv_python(
        &self,
        config: &Arc<Config>,
        tv: &mut ToolVersion,
    ) -> Result<()> {
        let python = self.spawnable_dependency(config, None, "python").await
            .ok_or_else(|| eyre!("Python graph installs require an installed interpreter; run `mise install python`"))?;
        let identity = if let Some(identity) = self.configured_python_identity(config).await {
            identity
        } else {
            CmdLineRunner::new(&python).args(["-I", "-c", "import sys, sysconfig; print((sys.implementation.name, sys.version_info[:2], sysconfig.get_config_var('SOABI'), sysconfig.get_platform()))"]).read().await?.trim().to_owned()
        };
        tv.uv_python = Some((python, identity));
        Ok(())
    }

    fn lock_requirement(&self, tv: &ToolVersion) -> Result<String> {
        let PipxRequest::Pypi(package) = self.tool_name().parse()? else {
            bail!("uv graph locking requires a PyPI package");
        };
        let raw = tv.request.options();
        let opts = PipxOptions::new(&raw);
        Ok(format!(
            "{package}{}=={}",
            opts.extras().map(|v| format!("[{v}]")).unwrap_or_default(),
            tv.version
        ))
    }

    fn lock_requirements(&self, tv: &ToolVersion) -> Result<Vec<String>> {
        let raw = tv.request.options();
        let opts = PipxOptions::new(&raw);
        let mut requirements = vec![self.lock_requirement(tv)?];
        requirements.extend(opts.with()?);
        requirements.extend(opts.expose()?);
        Ok(requirements)
    }

    async fn uv_lock_command(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        uv: &Path,
        project: &Path,
    ) -> Result<CmdLineRunner<'static>> {
        let registry = self.get_registry_url(config).await?;
        let index = uv_index_url(&registry)?;
        Ok(CmdLineRunner::new(uv)
            .current_dir(project)
            .envs(config.env().await?)
            .env_values(tv.install_env())
            .env("UV_DEFAULT_INDEX", index)
            .env_remove("UV_PROJECT")
            .env_remove("UV_WORKING_DIR")
            .env_remove("UV_PROJECT_ENVIRONMENT")
            .env_remove("VIRTUAL_ENV"))
    }

    /// The Python range a published release declares, or an empty string when it
    /// declares none.
    async fn release_python_requirement(
        &self,
        registry: &str,
        package: &str,
        version: &str,
    ) -> Result<String> {
        // JSON release metadata supplies the Python constraint without running a
        // build backend. Simple-only indexes expose it on wheel links.
        if registry.ends_with("/json") {
            let url = registry.replace("{}", &format!("{package}/{version}"));
            let metadata: Value = HTTP_FETCH.json(&url).await?;
            Ok(metadata
                .pointer("/info/requires_python")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned())
        } else {
            let html = HTTP_FETCH.get_html(registry.replace("{}", package)).await?;
            simple_index_python_requirement(package, version, &html)
        }
    }

    pub(crate) async fn resolve_uv_lock(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<GraphRef<UvLock>> {
        self.validate_lock_options(tv)?;
        let uv = self.lock_uv_program(config).await?;
        let registry = self.get_registry_url(config).await?;
        let requirements = self.lock_requirements(tv)?;
        let root = self
            .release_python_requirement(&registry, &self.tool_name(), &tv.version)
            .await
            .wrap_err_with(|| format!("failed to lock {}", self.ba.short))?;
        let mut constraints = vec![">=3.8".to_string()];
        constraints.extend(python_constraint(&root));
        // Everything in `with` and `expose` shares the sidecar's interpreter, so a
        // pinned injection with a narrower floor narrows the whole project. uv
        // spreads an unpinned requirement across the range by itself, and a marker
        // can switch a requirement off for part of it, so neither one applies here.
        for requirement in requirements.iter().skip(1) {
            let Some((package, version)) = pinned_requirement(requirement) else {
                continue;
            };
            match self
                .release_python_requirement(&registry, package, version)
                .await
            {
                Ok(value) => constraints.extend(python_constraint(&value)),
                // uv reports an unusable requirement far better than a partial
                // lookup can, so an unreadable release just leaves the range alone.
                Err(err) => debug!("no Python metadata for {requirement}: {err:#}"),
            }
        }
        constraints.dedup();
        let requires_python = constraints.join(",");
        let mut project = toml::Table::new();
        project.insert(
            "project".into(),
            toml::toml! {
                name = PROJECT_NAME
                version = "0.0.0"
                requires-python = requires_python
                dependencies = requirements
            }
            .into(),
        );
        let temp = tempfile::tempdir()?;
        crate::file::write(
            temp.path().join("pyproject.toml"),
            toml::to_string(&project)?,
        )?;
        let mut cmd = self
            .uv_lock_command(config, tv, &uv, temp.path())
            .await?
            .args(["lock", "--no-build", "--no-config", "--no-python-downloads"])
            .args(Self::uv_exclude_newer_args(tv.before_date));
        let raw = tv.request.options();
        if let Some(value) = PipxOptions::new(&raw).dependency_prereleases()? {
            cmd = cmd.args(["--prerelease", value]);
        }
        cmd.execute()?;
        let graph_text = crate::file::read_to_string(temp.path().join("uv.lock"))?;
        let graph: toml::Table = graph_text.parse()?;
        if let Some(requires_python) = graph.get("requires-python") {
            project
                .get_mut("project")
                .unwrap()
                .as_table_mut()
                .unwrap()
                .insert("requires-python".into(), requires_python.clone());
        }
        let lock = UvLock {
            project,
            graph,
            graph_text,
        };
        self.validate_uv_lock(tv, &lock)?;
        Ok(lock.into())
    }

    pub(crate) fn validate_uv_lock(&self, tv: &ToolVersion, lock: &UvLock) -> Result<()> {
        self.validate_lock_options(tv)?;
        let project = lock
            .project
            .get("project")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| eyre!("missing uv project"))?;
        let expected = self
            .lock_requirements(tv)?
            .into_iter()
            .map(toml::Value::String)
            .collect::<Vec<_>>();
        if project.get("dependencies").and_then(toml::Value::as_array) != Some(&expected)
            || project.get("name").and_then(toml::Value::as_str) != Some(PROJECT_NAME)
            || project.get("requires-python") != lock.graph.get("requires-python")
        {
            bail!(
                "Python lock does not match the requested tool; run `mise lock --bump {}`",
                self.ba.short
            );
        }
        let packages = lock
            .graph
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| eyre!("missing uv packages"))?;
        let mut root = false;
        let mut virtual_root = false;
        for package in packages {
            let package = package
                .as_table()
                .ok_or_else(|| eyre!("invalid uv package"))?;
            let name = package
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| eyre!("missing uv package name"))?;
            let source = package
                .get("source")
                .and_then(toml::Value::as_table)
                .ok_or_else(|| eyre!("missing uv package source"))?;
            if name == PROJECT_NAME
                && source.get("virtual").and_then(toml::Value::as_str) == Some(".")
            {
                let requirements = package
                    .get("metadata")
                    .and_then(|m| m.get("requires-dist"))
                    .and_then(toml::Value::as_array)
                    .ok_or_else(|| eyre!("missing uv root requirements"))?;
                let raw = tv.request.options();
                let extras = PipxOptions::new(&raw)
                    .extras()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(Self::normalize_package_name)
                    .collect::<std::collections::BTreeSet<_>>();
                let root_name = Self::normalize_package_name(&self.tool_name());
                let root_specifier = format!("=={}", tv.version);
                let root_requirement = requirements.iter().find(|requirement| {
                    requirement.get("name").and_then(toml::Value::as_str)
                        == Some(root_name.as_str())
                        && requirement.get("specifier").and_then(toml::Value::as_str)
                            == Some(root_specifier.as_str())
                });
                let locked_extras = root_requirement
                    .and_then(|requirement| requirement.get("extras"))
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .map(|extra| Self::normalize_package_name(extra.trim()))
                    .collect::<std::collections::BTreeSet<_>>();
                if requirements.len() != expected.len()
                    || root_requirement.is_none()
                    || extras != locked_extras
                {
                    bail!(
                        "uv graph root requirements do not match the requested package and extras"
                    );
                }
                virtual_root = true;
                continue;
            }
            if source.len() != 1 || !source.contains_key("registry") {
                bail!("Python locks support registry wheels only");
            }
            if name == Self::normalize_package_name(&self.tool_name())
                && package.get("version").and_then(toml::Value::as_str) == Some(&tv.version)
            {
                root = true;
            }
            let wheels = package
                .get("wheels")
                .and_then(toml::Value::as_array)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    eyre!("{name} has no published wheels; Python graph locks require wheels")
                })?;
            for wheel in wheels {
                let hash = wheel
                    .get("hash")
                    .and_then(toml::Value::as_str)
                    .and_then(|h| h.strip_prefix("sha256:"));
                if !hash.is_some_and(|h| h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit()))
                {
                    bail!("{name} has a wheel without a SHA256 hash");
                }
            }
        }
        if !root || !virtual_root {
            bail!("Python lock is missing the requested root package");
        }
        validate_portable_urls(&toml::Value::Table(lock.graph.clone()))?;
        // No arbitrary project settings or build systems are accepted from a lockfile.
        if lock.project.len() != 1 || project.len() != 4 {
            bail!("unsupported Python lock project settings");
        }
        Ok(())
    }

    pub(super) async fn install_uv_lock(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> Result<()> {
        let lock = tv
            .uv_lock
            .as_ref()
            .ok_or_else(|| eyre!("missing uv lock"))?
            .load()?;
        self.validate_uv_lock(tv, lock)?;
        let uv = self.lock_uv_program(&ctx.config).await?;
        let (python, _) = tv.uv_python.as_ref().ok_or_else(|| {
            eyre!(
                "Python graph installs require an installed interpreter; run `mise install python`"
            )
        })?;
        let project = tv.install_path().join(".mise-uv");
        crate::file::create_dir_all(&project)?;
        crate::file::write(
            project.join("python.json"),
            serde_json::to_string(&tv.uv_python.as_ref().unwrap())?,
        )?;
        crate::file::write(
            project.join("pyproject.toml"),
            toml::to_string(&lock.project)?,
        )?;
        crate::file::write(project.join("uv.lock"), lock.graph_text()?)?;
        ctx.pr
            .set_message("installing frozen Python dependencies".to_owned());
        self.uv_lock_command(&ctx.config, tv, &uv, &project)
            .await?
            .args([
                "sync",
                "--frozen",
                "--no-build",
                "--no-config",
                "--no-python-downloads",
                "--no-install-project",
                "--python",
            ])
            .arg(python)
            .env("UV_PROJECT_ENVIRONMENT", project.join(".venv"))
            .env_remove("UV_EXCLUDE_NEWER")
            .with_pr(ctx.pr.as_ref())
            .execute()?;
        let scripts = project
            .join(".venv")
            .join(if cfg!(windows) { "Scripts" } else { "bin" });
        let python = scripts.join(if cfg!(windows) {
            "python.exe"
        } else {
            "python"
        });
        let packages = self.locked_entry_point_packages(tv, lock)?;
        let packages = serde_json::to_string(&packages)?;
        let names = CmdLineRunner::new(python)
            .args(["-I", "-c", DISCOVER_SCRIPTS, &packages])
            .arg(&scripts)
            .read()
            .await?;
        let names: Vec<String> = serde_json::from_str(names.trim())?;
        if names.is_empty() {
            bail!("{} exposes no executable scripts", self.ba.short);
        }
        let bin = tv.install_path().join("bin");
        crate::file::create_dir_all(&bin)?;
        for name in names {
            if !crate::file::is_plain_file_name(&name) {
                bail!("invalid Python entry point name");
            }
            crate::file::make_symlink_or_copy(&scripts.join(&name), &bin.join(&name))?;
        }
        Ok(())
    }

    fn locked_entry_point_packages(&self, tv: &ToolVersion, lock: &UvLock) -> Result<Vec<String>> {
        let root_requirement = lock
            .project
            .get("project")
            .and_then(toml::Value::as_table)
            .and_then(|project| project.get("dependencies"))
            .and_then(toml::Value::as_array)
            .and_then(|dependencies| dependencies.first())
            .and_then(toml::Value::as_str)
            .ok_or_else(|| eyre!("missing locked Python root requirement"))?;
        let root = PipxOptions::requirement_package_name(root_requirement)
            .ok_or_else(|| eyre!("invalid locked Python root requirement"))?;
        let raw = tv.request.options();
        let mut packages = vec![root.to_string()];
        packages.extend(PipxOptions::new(&raw).exposed_package_names()?);
        Ok(packages)
    }
}

/// The package and exact version of a requirement that pins one release across the
/// whole interpreter range, or `None` when the requirement cannot narrow it.
fn pinned_requirement(requirement: &str) -> Option<(&str, &str)> {
    let requirement = requirement.trim();
    // A direct URL reference has no index entry to read metadata from.
    if requirement.contains('@') {
        return None;
    }
    let (specifier, marker) = requirement.split_once(';').unwrap_or((requirement, ""));
    // A marker that tests the interpreter drops the requirement below its own
    // floor, so the release cannot constrain the project. Every other marker
    // leaves the requirement in place across the whole range.
    if [
        "python_version",
        "python_full_version",
        "implementation_version",
    ]
    .iter()
    .any(|variable| marker.contains(variable))
    {
        return None;
    }
    let specifier = specifier.trim();
    let package = PipxOptions::requirement_package_name(specifier)?;
    let mut rest = specifier[package.len()..].trim_start();
    if let Some(extras) = rest.strip_prefix('[') {
        rest = extras.split_once(']')?.1.trim_start();
    }
    // One `==` clause pins the release whatever the surrounding clauses allow.
    rest.split(',')
        .filter_map(|clause| clause.trim().strip_prefix("=="))
        .map(str::trim)
        // `===` is arbitrary equality and a wildcard spans releases.
        .find(|version| {
            !version.is_empty()
                && !version.starts_with('=')
                && !version.contains('*')
                && !version.contains(char::is_whitespace)
        })
        .map(|version| (package, version))
}

fn python_constraint(requires_python: &str) -> Option<String> {
    let requires_python = requires_python.trim();
    (!requires_python.is_empty()).then(|| requires_python.to_string())
}

fn simple_index_python_requirement(package: &str, version: &str, html: &str) -> Result<String> {
    let links = regex!(r#"(?is)<a\s+(?:[^"'<>]|"[^"]*"|'[^']*')*>"#);
    let href = regex!(r#"(?i)href\s*=\s*["']([^"']+)["']"#);
    let python = regex!(r#"(?i)data-requires-python\s*=\s*["']([^"']*)["']"#);
    let mut constraints = std::collections::BTreeSet::new();
    for link in links.find_iter(html) {
        let Some(url) = href.captures(link.as_str()).and_then(|c| c.get(1)) else {
            continue;
        };
        let Some(filename) = PIPXBackend::distribution_filename_from_url(url.as_str()) else {
            continue;
        };
        if PIPXBackend::version_from_distribution_filename(package, &filename).as_deref()
            == Some(version)
            && filename.ends_with(".whl")
        {
            let value = python
                .captures(link.as_str())
                .and_then(|c| c.get(1))
                .map(|v| v.as_str())
                .unwrap_or("");
            constraints.insert(
                value
                    .replace("&gt;", ">")
                    .replace("&lt;", "<")
                    .replace("&amp;", "&"),
            );
        }
    }
    if constraints.len() != 1 {
        bail!(
            "package {} requires consistent Python metadata on published wheels to generate a portable lock",
            package
        );
    }
    Ok(constraints.into_iter().next().unwrap())
}

fn uv_index_url(registry: &str) -> Result<String> {
    let base = registry.split("{}").next().unwrap_or(registry);
    let mut url = url::Url::parse(base)?;
    if url
        .host_str()
        .is_some_and(|host| host == "pypi.org" || host.ends_with(".pypi.org"))
    {
        url.set_path("/simple/");
    } else {
        let path = url.path().trim_end_matches('/').trim_end_matches("/simple");
        url.set_path(&format!("{path}/simple/"));
    }
    Ok(url.into())
}

fn validate_portable_urls(value: &toml::Value) -> Result<()> {
    match value {
        toml::Value::String(s) if s.contains("://") => {
            let url = url::Url::parse(s)?;
            if !matches!(url.scheme(), "https" | "http")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
            {
                bail!(
                    "Python locks require credential-free HTTP artifact URLs; configure authentication outside the lockfile"
                );
            }
        }
        toml::Value::Table(t) => {
            for value in t.values() {
                validate_portable_urls(value)?;
            }
        }
        toml::Value::Array(a) => {
            for value in a {
                validate_portable_urls(value)?;
            }
        }
        _ => (),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolset::ToolSource;

    #[test]
    fn system_environment_discovery_needs_no_interpreter_and_matches_graph() {
        let temp = tempfile::tempdir().unwrap();
        let mut ba = BackendArg::from("pypi:demo");
        ba.installs_path = temp.path().to_path_buf();
        let request = ToolRequest::new(Arc::new(ba), "1.0.0", ToolSource::Argument).unwrap();
        let mut installed = ToolVersion::new(request, "1.0.0".into());
        installed.uv_lock = Some(fixture().2.into());
        installed.uv_python = Some((
            PathBuf::from("/missing/python"),
            "cpython-3.12-platform".into(),
        ));
        let project = installed.install_path().join(".mise-uv");
        crate::file::create_dir_all(&project).unwrap();
        crate::file::write(
            project.join("python.json"),
            serde_json::to_string(installed.uv_python.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        let mut resolved = installed.clone();
        resolved.uv_python = None;
        PIPXBackend::restore_system_uv_python(&mut resolved);
        assert_eq!(resolved.uv_python, installed.uv_python);
        assert_eq!(resolved.install_path(), installed.install_path());
        resolved.uv_python = None;
        let mut changed = resolved.uv_lock.as_ref().unwrap().load().unwrap().clone();
        changed.graph.insert("revision".into(), 99.into());
        resolved.uv_lock = Some(changed.into());
        PIPXBackend::restore_system_uv_python(&mut resolved);
        assert!(resolved.uv_python.is_none());
    }

    fn fixture() -> (PIPXBackend, ToolVersion, UvLock) {
        let request = ToolRequest::new(
            Arc::new(BackendArg::from("pypi:demo")),
            "1.0.0",
            ToolSource::Argument,
        )
        .unwrap();
        let tv = ToolVersion::new(request, "1.0.0".into());
        let backend = PIPXBackend::from_arg(tv.ba().clone());
        let project = toml::toml! {
            [project]
            name = "mise-pypi-tool-environment"
            version = "0.0.0"
            requires-python = ">=3.10"
            dependencies = ["demo==1.0.0"]
        };
        let graph: toml::Table = format!(
            r#"
version = 1
revision = 3
requires-python = ">=3.10"
[[package]]
name = "demo"
version = "1.0.0"
source = {{ registry = "https://pypi.org/simple/" }}
wheels = [{{ url = "https://example.org/demo.whl", hash = "sha256:{}" }}]
[[package]]
name = "mise-pypi-tool-environment"
version = "0.0.0"
source = {{ virtual = "." }}
dependencies = [{{ name = "demo" }}]
[package.metadata]
requires-dist = [{{ name = "demo", specifier = "==1.0.0" }}]
"#,
            "a".repeat(64)
        )
        .parse()
        .unwrap();
        (
            backend,
            tv,
            UvLock {
                project,
                graph,
                graph_text: String::new(),
            },
        )
    }

    #[test]
    fn extras_validation_trims_and_normalizes_names() {
        let (backend, mut tv, mut lock) = fixture();
        let mut options = tv.request.options();
        options.opts.insert("extras".into(), "postgres, S_3".into());
        tv.request.set_options(options);
        lock.project
            .get_mut("project")
            .unwrap()
            .as_table_mut()
            .unwrap()
            .insert(
                "dependencies".into(),
                vec![toml::Value::String(backend.lock_requirement(&tv).unwrap())].into(),
            );
        let packages = lock
            .graph
            .get_mut("package")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let requirement = packages[1]
            .get_mut("metadata")
            .unwrap()
            .get_mut("requires-dist")
            .unwrap()
            .as_array_mut()
            .unwrap();
        requirement[0].as_table_mut().unwrap().insert(
            "extras".into(),
            vec![
                toml::Value::String("postgres".into()),
                toml::Value::String("s-3".into()),
            ]
            .into(),
        );
        backend.validate_uv_lock(&tv, &lock).unwrap();
    }

    #[test]
    fn only_exact_pins_outside_interpreter_markers_narrow_the_range() {
        for (requirement, pinned) in [
            ("demo==1.0.0", Some(("demo", "1.0.0"))),
            ("demo == 1.0.0", Some(("demo", "1.0.0"))),
            ("demo[extra,other]==1.0.0", Some(("demo", "1.0.0"))),
            ("  demo==1.0+local  ", Some(("demo", "1.0+local"))),
            // A surrounding clause cannot widen what `==` already pinned.
            ("demo==1.0.0,!=1.0.1", Some(("demo", "1.0.0"))),
            ("demo>=1.0.0,==1.0.0", Some(("demo", "1.0.0"))),
            // A marker on anything but the interpreter keeps the requirement
            // active across the whole range.
            (
                "demo==1.0.0; sys_platform == 'linux'",
                Some(("demo", "1.0.0")),
            ),
            // uv resolves these across the range on its own.
            ("demo", None),
            ("demo>=1.0.0", None),
            ("demo==1.0.*", None),
            ("demo===1.0.0", None),
            // The interpreter marker drops this below its own floor, so the
            // release's requirement is not the project's.
            ("demo==1.0.0; python_version < '3.10'", None),
            ("demo==1.0.0; python_full_version >= '3.12.1'", None),
            // There is no index entry to read metadata from.
            (
                "demo @ https://example.org/demo-1.0.0-py3-none-any.whl",
                None,
            ),
        ] {
            assert_eq!(pinned_requirement(requirement), pinned, "{requirement}");
        }
    }

    #[test]
    fn simple_index_python_requirement_preserves_quoted_angle_brackets() {
        for requirement in [">=3.10", "&gt;=3.10"] {
            for quote in ['"', '\''] {
                let html = format!(
                    "<a href=\"demo-1.0%2Blocal-py3-none-any.whl\" data-requires-python={quote}{requirement}{quote}>wheel</a>"
                );
                assert_eq!(
                    simple_index_python_requirement("demo", "1.0+local", &html).unwrap(),
                    ">=3.10"
                );
            }
        }
    }

    #[test]
    fn uv_indexes_preserve_private_registry_paths() {
        for (registry, index) in [
            ("https://pypi.org/pypi/{}/json", "https://pypi.org/simple/"),
            (
                "https://test.pypi.org/pypi/{}/json",
                "https://test.pypi.org/simple/",
            ),
            (
                "https://notpypi.org/pypi/{}/json",
                "https://notpypi.org/pypi/simple/",
            ),
            (
                "https://packages.example.com/pypi/{}/json",
                "https://packages.example.com/pypi/simple/",
            ),
            (
                "https://packages.example.com/pypi/simple/{}/",
                "https://packages.example.com/pypi/simple/",
            ),
        ] {
            assert_eq!(uv_index_url(registry).unwrap(), index);
        }
        let filename = PIPXBackend::distribution_filename_from_url(
            "https://example.com/demo-1.0%2Blocal-py3-none-any.whl#sha256=abc",
        )
        .unwrap();
        assert_eq!(
            PIPXBackend::version_from_distribution_filename("demo", &filename).as_deref(),
            Some("1.0+local")
        );
    }

    #[test]
    fn uv_lock_rejects_changed_root_and_unhashed_wheels() {
        let (backend, tv, lock) = fixture();
        backend.validate_uv_lock(&tv, &lock).unwrap();
        let mut wrong = lock.clone();
        wrong.project["project"]["dependencies"] =
            toml::Value::Array(vec!["another==1.0.0".into()]);
        assert!(backend.validate_uv_lock(&tv, &wrong).is_err());
        let mut unhashed = lock.clone();
        unhashed.graph["package"].as_array_mut().unwrap()[0]["wheels"]
            .as_array_mut()
            .unwrap()[0]
            .as_table_mut()
            .unwrap()
            .remove("hash");
        assert!(backend.validate_uv_lock(&tv, &unhashed).is_err());
        let mut source_only = lock;
        source_only.graph["package"].as_array_mut().unwrap()[0]
            .as_table_mut()
            .unwrap()
            .remove("wheels");
        assert!(backend.validate_uv_lock(&tv, &source_only).is_err());
    }

    #[test]
    fn locked_entry_points_use_the_distribution_name() {
        let (backend, tv, mut lock) = fixture();
        lock.project["project"]["dependencies"] =
            toml::Value::Array(vec!["azure-cli==1.0.0".into()]);
        assert_eq!(
            backend.locked_entry_point_packages(&tv, &lock).unwrap(),
            ["azure-cli"]
        );
    }

    #[test]
    fn uv_lock_identity_preserves_native_markers_and_ignores_key_order() {
        let (_, _, mut lock) = fixture();
        lock.graph.insert(
            "resolution-markers".into(),
            vec![
                "python_full_version < '3.12'",
                "python_full_version >= '3.12'",
            ]
            .into(),
        );
        let serialized = toml::to_string(&lock).unwrap();
        let reloaded: UvLock = toml::from_str(&serialized).unwrap();
        assert_eq!(lock, reloaded);
        assert_eq!(
            GraphRef::from(lock.clone()).identity(),
            GraphRef::from(reloaded).identity()
        );
        let mut changed = lock.clone();
        changed.graph.insert(
            "resolution-markers".into(),
            vec!["python_full_version < '3.13'"].into(),
        );
        assert_ne!(
            GraphRef::from(lock).identity(),
            GraphRef::from(changed).identity()
        );
    }

    #[test]
    fn uv_lock_rejects_credentials_and_local_sources() {
        for url in [
            "https://user:password@example.org/file.whl",
            "https://example.org/file.whl?token=secret",
            "file:///tmp/tool.whl",
        ] {
            assert!(validate_portable_urls(&url.into()).is_err());
        }
    }
}
