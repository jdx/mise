//! Portable, wheel-only Python tool environments. uv owns dependency resolution;
//! mise owns lock persistence, interpreter selection, and executable exposure.
use super::*;
use crate::lockfile::UvLock;
use std::path::PathBuf;

const MIN_UV_VERSION: &str = "0.12.10";
const PROJECT_NAME: &str = "mise-pypi-tool-environment";

impl PIPXBackend {
    pub(crate) fn uv_lock_allowed(&self, tv: &ToolVersion) -> bool {
        Settings::get().pypi.uvx != Some(false)
            && !PipxOptions::new(&tv.request.options()).uvx_disabled()
            && matches!(
                self.tool_name().parse::<PipxRequest>(),
                Ok(PipxRequest::Pypi(_))
            )
    }

    fn validate_lock_options(&self, tv: &ToolVersion) -> Result<()> {
        let raw = tv.request.options();
        let opts = PipxOptions::new(&raw);
        if [opts.uvx_args(), opts.pipx_args()]
            .into_iter()
            .flatten()
            .any(|s| !s.trim().is_empty())
        {
            bail!(
                "pypi:{} dependency locking does not support uvx_args or pipx_args",
                self.tool_name()
            );
        }
        if !self.uv_lock_allowed(tv) {
            bail!(
                "pypi:{} has a uv dependency graph; use uv with a PyPI package to replay it",
                self.tool_name()
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

    pub(crate) async fn bind_uv_python(
        &self,
        config: &Arc<Config>,
        tv: &mut ToolVersion,
    ) -> Result<()> {
        let Some(python) = self.spawnable_dependency(config, None, "python").await else {
            // Resolve on a clean machine before the install graph installs Python.
            tv.uv_python = None;
            return Ok(());
        };
        let identity = CmdLineRunner::new(&python).args(["-I", "-c", "import sys, sysconfig; print((sys.implementation.name, sys.version_info[:3], sysconfig.get_config_var('SOABI'), sysconfig.get_platform(), sys.executable))"]).read().await?;
        tv.uv_python = Some((python, identity.trim().to_owned()));
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

    async fn uv_lock_command(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        uv: &Path,
        project: &Path,
    ) -> Result<CmdLineRunner<'static>> {
        let registry = self.get_registry_url(config).await?;
        let base = registry.split("{}").next().unwrap_or(&registry);
        let base = base
            .trim_end_matches('/')
            .trim_end_matches("/pypi")
            .trim_end_matches("/simple");
        let index = format!("{base}/simple/");
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

    pub(crate) async fn resolve_uv_lock(&self, tv: &ToolVersion) -> Result<UvLock> {
        self.validate_lock_options(tv)?;
        let config = Config::get().await?;
        let uv = self.lock_uv_program(&config).await?;
        let registry = self.get_registry_url(&config).await?;
        // JSON release metadata supplies the root's Python constraint without
        // running a build backend. Simple-only indexes expose it on wheel links.
        let requires_python = if registry.ends_with("/json") {
            let url = registry.replace("{}", &format!("{}/{}", self.tool_name(), tv.version));
            let metadata: Value = HTTP_FETCH.json(&url).await?;
            metadata
                .pointer("/info/requires_python")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        } else {
            let html = HTTP_FETCH
                .get_html(registry.replace("{}", &self.tool_name()))
                .await?;
            let links = regex!(r#"(?is)<a\s+[^>]*>"#);
            let href = regex!(r#"(?i)href\s*=\s*["']([^"']+)["']"#);
            let python = regex!(r#"(?i)data-requires-python\s*=\s*["']([^"']*)["']"#);
            let mut constraints = std::collections::BTreeSet::new();
            for link in links.find_iter(&html) {
                let Some(url) = href.captures(link.as_str()).and_then(|c| c.get(1)) else {
                    continue;
                };
                let filename = url
                    .as_str()
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("");
                if Self::version_from_distribution_filename(&self.tool_name(), filename).as_deref()
                    == Some(&tv.version)
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
                    "pypi:{} requires consistent Python metadata on published wheels to generate a portable lock",
                    self.tool_name()
                );
            }
            constraints.into_iter().next().unwrap()
        };
        let requires_python = if requires_python.trim().is_empty() {
            ">=3.8".to_string()
        } else {
            format!(">=3.8,{requires_python}")
        };
        let requirement = self.lock_requirement(tv)?;
        let mut project = toml::Table::new();
        project.insert(
            "project".into(),
            toml::toml! {
                name = PROJECT_NAME
                version = "0.0.0"
                requires-python = requires_python
                dependencies = [requirement]
            }
            .into(),
        );
        let temp = tempfile::tempdir()?;
        crate::file::write(
            temp.path().join("pyproject.toml"),
            toml::to_string(&project)?,
        )?;
        self.uv_lock_command(&config, tv, &uv, temp.path())
            .await?
            .args(["lock", "--no-build", "--no-config", "--no-python-downloads"])
            .args(Self::uv_exclude_newer_args(tv.before_date))
            .execute()?;
        let graph: toml::Table =
            crate::file::read_to_string(temp.path().join("uv.lock"))?.parse()?;
        if let Some(requires_python) = graph.get("requires-python") {
            project
                .get_mut("project")
                .unwrap()
                .as_table_mut()
                .unwrap()
                .insert("requires-python".into(), requires_python.clone());
        }
        let lock = UvLock { project, graph };
        self.validate_uv_lock(tv, &lock)?;
        Ok(lock)
    }

    pub(crate) fn validate_uv_lock(&self, tv: &ToolVersion, lock: &UvLock) -> Result<()> {
        self.validate_lock_options(tv)?;
        let project = lock
            .project
            .get("project")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| eyre!("missing uv project"))?;
        let expected = vec![toml::Value::String(self.lock_requirement(tv)?)];
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
                let requirement = requirements
                    .first()
                    .ok_or_else(|| eyre!("empty uv root requirements"))?;
                let raw = tv.request.options();
                let extras = PipxOptions::new(&raw)
                    .extras()
                    .unwrap_or_default()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(Self::normalize_package_name)
                    .collect::<std::collections::BTreeSet<_>>();
                let locked_extras = requirement
                    .get("extras")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect::<std::collections::BTreeSet<_>>();
                if requirements.len() != 1
                    || requirement.get("name").and_then(toml::Value::as_str)
                        != Some(Self::normalize_package_name(&self.tool_name()).as_str())
                    || requirement.get("specifier").and_then(toml::Value::as_str)
                        != Some(format!("=={}", tv.version).as_str())
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
            .ok_or_else(|| eyre!("missing uv lock"))?;
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
            project.join("pyproject.toml"),
            toml::to_string(&lock.project)?,
        )?;
        crate::file::write(project.join("uv.lock"), toml::to_string(&lock.graph)?)?;
        ctx.pr
            .set_message("installing frozen Python dependencies".to_owned());
        self.uv_lock_command(&ctx.config, tv, &uv, &project)
            .await?
            .args([
                "sync",
                "--frozen",
                "--no-build",
                "--no-cache",
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
        let names = CmdLineRunner::new(python).args(["-I", "-c", "import importlib.metadata, json, sys; print(json.dumps([e.name for e in importlib.metadata.distribution(sys.argv[1]).entry_points if e.group in ('console_scripts', 'gui_scripts')]))", &self.tool_name()]).read().await?;
        let names: Vec<String> = serde_json::from_str(names.trim())?;
        if names.is_empty() {
            bail!("pypi:{} exposes no executable scripts", self.tool_name());
        }
        let bin = tv.install_path().join("bin");
        crate::file::create_dir_all(&bin)?;
        for name in names {
            if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
                bail!("invalid Python entry point name");
            }
            let name = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name
            };
            crate::file::make_symlink_or_copy(&scripts.join(&name), &bin.join(&name))?;
        }
        Ok(())
    }
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
        (backend, tv, UvLock { project, graph })
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
        assert_eq!(lock.identity(), reloaded.identity());
        let mut changed = lock.clone();
        changed.graph.insert(
            "resolution-markers".into(),
            vec!["python_full_version < '3.13'"].into(),
        );
        assert_ne!(lock.identity(), changed.identity());
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
