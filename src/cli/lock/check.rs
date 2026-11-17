//! `mise lock --check`: whether each lockfile in scope already holds what a
//! locked install needs, read from the lockfile alone. Nothing is resolved
//! remotely and nothing is written.

use std::path::{Path, PathBuf};

use console::style;
use eyre::{Result, bail};
use mise_util::redactions::{redact_json, redact_output};

use super::{Lock, LockInputs, LockSetup};
use crate::backend::platform_target::PlatformTarget;
use crate::config::{Settings, SettingsExt};
use crate::file::display_path;
use crate::lockfile::{self, PlatformInfo};
use crate::platform::Platform;
use crate::toolset::{ResolveOptions, ToolRequest, ToolSource, ToolVersion};

/// One row of `mise lock --check --json`: a tool's entry on one platform,
/// or, with no platform, a problem with the tool or lockfile as a whole.
#[derive(Debug, serde::Serialize)]
struct CheckEntry {
    lockfile: String,
    tool: Option<String>,
    version: Option<String>,
    platform: Option<String>,
    /// How a locked install verifies the download: the provenance type the
    /// entry records, `checksum only`, or `none`. Only platform entries have one.
    provenance: Option<String>,
    problems: Vec<String>,
}

/// How a locked install verifies the artifact of `entry`. This only
/// describes the entry; whether it is enough is not judged here.
fn provenance(entry: Option<&PlatformInfo>) -> String {
    match entry {
        Some(PlatformInfo {
            provenance: Some(provenance),
            ..
        }) => provenance.to_string(),
        Some(PlatformInfo {
            checksum: Some(_), ..
        }) => "checksum only".to_string(),
        _ => "none".to_string(),
    }
}

/// What checking one lockfile found.
struct LockfileCheck {
    path: PathBuf,
    tools: usize,
    entries: Vec<CheckEntry>,
}

impl LockfileCheck {
    fn problems(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().flat_map(|entry| &entry.problems)
    }

    fn push(
        &mut self,
        tool: Option<&ToolVersion>,
        platform: Option<(String, Option<&PlatformInfo>)>,
        problems: Vec<String>,
    ) {
        let (platform, provenance) = match platform {
            Some((key, entry)) => (Some(key), Some(provenance(entry))),
            None => (None, None),
        };
        self.entries.push(CheckEntry {
            lockfile: display_path(&self.path),
            tool: tool.map(|tv| tv.ba().short.clone()),
            version: tool.map(|tv| tv.version.clone()),
            platform,
            provenance,
            problems,
        });
    }

    fn print(&self) -> Result<()> {
        let problems = self.problems().count();
        let platform_entries = || self.entries.iter().filter(|e| e.platform.is_some());
        if problems == 0 {
            miseprintln!(
                "{} {} is up to date ({}, {})",
                style("✓").green(),
                style(display_path(&self.path)).cyan(),
                count(self.tools, "tool version", "tool versions"),
                count(
                    platform_entries().count(),
                    "platform entry",
                    "platform entries"
                ),
            );
        } else {
            miseprintln!(
                "{} {} has {}",
                style("✗").red(),
                style(display_path(&self.path)).cyan(),
                count(problems, "problem", "problems"),
            );
        }
        for entry in platform_entries() {
            let (Some(tool), Some(version), Some(platform), Some(provenance)) = (
                &entry.tool,
                &entry.version,
                &entry.platform,
                &entry.provenance,
            ) else {
                continue;
            };
            miseprintln!(
                "  {}@{version} for {}: {provenance}",
                style(redact_output(tool)).bold(),
                style(platform).blue(),
            );
        }
        Ok(())
    }

    /// A row for `request`, which did not resolve to a lockfile entry.
    fn push_unresolved(
        &mut self,
        request: &ToolRequest,
        platform: Option<String>,
        problem: String,
    ) {
        self.entries.push(CheckEntry {
            lockfile: display_path(&self.path),
            tool: Some(request.ba().short.clone()),
            version: Some(request.version()),
            platform,
            provenance: None,
            problems: vec![problem],
        });
    }
}

/// The first line of an error, without color: the hints that follow it are
/// replaced by one hint for the whole check.
fn first_line(err: &eyre::Report) -> String {
    let message = console::strip_ansi_codes(&err.to_string()).into_owned();
    message.lines().next().unwrap_or_default().to_string()
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Lock {
    /// Check every lockfile target, then report and fail if any problem was found.
    pub(super) async fn check_lockfiles(
        &self,
        setup: &LockSetup<'_>,
        inputs: &LockInputs,
    ) -> Result<()> {
        let mut checks = Vec::with_capacity(inputs.lockfile_targets.len());
        let mut configured: Vec<crate::args::BackendArg> = Vec::new();
        let mut targets = Vec::with_capacity(inputs.lockfile_targets.len());
        for (lockfile_path, config_paths) in &inputs.lockfile_targets {
            let platforms = self.determine_target_platforms(lockfile_path)?;
            let requests = self.check_requests(
                setup,
                lockfile_path,
                config_paths,
                &platforms,
                &mut configured,
            )?;
            targets.push((lockfile_path, platforms, requests));
        }
        // As `mise lock` does, refuse a tool argument that names nothing
        // configured: a typo would otherwise pass as a clean check.
        let missing: Vec<String> = self
            .tool
            .iter()
            .filter(|arg| !configured.iter().any(|ba| tool_arg_names(&arg.ba, ba)))
            .map(|arg| arg.ba.to_string())
            .collect();
        if !missing.is_empty() {
            bail!(
                "cannot check {}: not configured in this project. Add it with `mise use`, or remove it from the arguments",
                redact_output(&missing.join(", "))
            );
        }
        for (lockfile_path, platforms, requests) in targets {
            if requests.is_empty() {
                continue;
            }
            checks.push(
                self.check_lockfile(setup, lockfile_path, &platforms, requests)
                    .await?,
            );
        }

        let problem_count: usize = checks.iter().map(|check| check.problems().count()).sum();
        if self.json {
            let entries: Vec<&CheckEntry> =
                checks.iter().flat_map(|check| &check.entries).collect();
            let entries = redact_json(serde_json::to_value(&entries)?);
            miseprintln!("{}", serde_json::to_string_pretty(&entries)?);
        } else if checks.is_empty() {
            miseprintln!("{} No tools configured to lock", style("!").yellow());
        } else {
            for check in &checks {
                check.print()?;
            }
        }
        if problem_count == 0 {
            return Ok(());
        }
        let mut message = String::new();
        for check in checks
            .iter()
            .filter(|check| check.problems().next().is_some())
        {
            message.push_str(&format!(
                "{} would fail a locked install:\n",
                display_path(&check.path)
            ));
            for problem in check.problems() {
                message.push_str(&format!("  - {}\n", redact_output(problem)));
            }
        }
        message.push_str("hint: run `mise lock` to update the lockfile");
        bail!(message)
    }

    /// The requests `mise lock` would lock into `lockfile_path`: the
    /// configured tools of the config files that write it, plus the tools of
    /// tasks declared there, as named by the tool arguments.
    fn check_requests(
        &self,
        setup: &LockSetup<'_>,
        lockfile_path: &Path,
        config_paths: &[PathBuf],
        platforms: &[Platform],
        configured: &mut Vec<crate::args::BackendArg>,
    ) -> Result<Vec<ToolRequest>> {
        let config = setup.config;
        let named = |ba: &crate::args::BackendArg| {
            self.tool.is_empty() || self.tool.iter().any(|tool| tool_arg_names(&tool.ba, ba))
        };
        // `mise lock` also locks the Python a pipx tool selects, for the
        // tools it locks or, with `mise lock python`, for every tool. It
        // resolves only a tool whose `os` option allows a locked platform,
        // and only such a tool's Python comes with it.
        let python_named = self.tool.iter().any(|tool| tool.ba.short == "python");
        let mut pythons: Vec<ToolRequest> = Vec::new();
        let mut select_python = |ba: &crate::args::BackendArg, request: &ToolRequest| {
            configured.push(ba.clone());
            let locked_somewhere = platforms
                .iter()
                .any(|platform| request.is_os_supported_on(platform));
            if (named(ba) || python_named)
                && locked_somewhere
                && let Some(python) = crate::backend::pipx::selected_python_dependency(request)?
            {
                configured.push(python.ba().as_ref().clone());
                pythons.push(python);
            }
            eyre::Ok(())
        };
        let mut requests: Vec<ToolRequest> = Vec::new();
        let mut push = |request: ToolRequest| {
            let duplicate = requests.iter().any(|seen| {
                seen.ba().full() == request.ba().full()
                    && seen.version() == request.version()
                    && seen.options() == request.options()
                    && seen.source() == request.source()
            });
            if !duplicate {
                requests.push(request);
            }
        };
        for (path, cf) in setup.effective_config_files.iter() {
            let source = cf.source();
            let idiomatic_here = source.is_idiomatic_version_file()
                && lockfile::lockfile_path_for_tool_source(config, &source).is_some_and(
                    |(source_lockfile, _)| {
                        lockfile::same_file_path(&source_lockfile, lockfile_path)
                    },
                );
            if !(config_paths.contains(path) || idiomatic_here) {
                continue;
            }
            for (ba, tool_requests, _) in cf.to_tool_request_set()?.iter() {
                for request in tool_requests {
                    select_python(ba, request)?;
                }
                if named(ba) {
                    tool_requests.iter().cloned().for_each(&mut push);
                }
            }
        }
        for task in setup.tasks.values() {
            let Some(config_path) =
                self.task_config_path(task, config, config_paths, setup.effective_config_files)
            else {
                continue;
            };
            let (task_lockfile_path, _) = lockfile::lockfile_path_for_config(
                &config_path,
                config.monorepo_lockfile_root().as_deref(),
            );
            if !lockfile::same_file_path(&task_lockfile_path, lockfile_path) {
                continue;
            }
            let tools = task.tool_args().map_err(|err| {
                err.wrap_err(format!("failed to parse tools for task `{}`", task.name))
            })?;
            for tool in tools {
                let Some(request) = tool.tvr.as_ref() else {
                    continue;
                };
                let request = ToolRequest::new_with_options(
                    tool.ba.clone(),
                    &request.version(),
                    request.options(),
                    ToolSource::MiseToml(config_path.clone()),
                )?;
                select_python(&tool.ba, &request)?;
                if named(&tool.ba) {
                    push(request);
                }
            }
        }
        // Tool stubs that read their entries from this lockfile, as `mise
        // lock` locks them: no config file declares them.
        for (path, stub) in super::live_tool_stubs(lockfile_path, config_paths)? {
            let skip = |err: eyre::Report| {
                warn!(
                    "skipping tool stub {}: not a valid tool stub",
                    display_path(&path)
                );
                debug!("tool stub {}: {err:#}", display_path(&path));
            };
            let stub = match stub {
                Ok(stub) => stub,
                Err(err) => {
                    skip(err);
                    continue;
                }
            };
            if Settings::safe_mode() && stub.has_tool_options() {
                warn!(
                    "skipping tool stub {} in safe mode: its tool options apply only outside safe mode",
                    display_path(&path)
                );
                continue;
            }
            let request = match stub.to_tool_request(&path) {
                Ok(request) => request,
                Err(err) => {
                    skip(err);
                    continue;
                }
            };
            select_python(request.ba(), &request)?;
            if named(request.ba()) {
                push(request);
            }
        }
        pythons.into_iter().for_each(&mut push);
        Ok(requests)
    }

    /// Resolve each request from the lockfile as `--locked` does, and check
    /// each platform's entry the way a locked install there would.
    async fn check_lockfile(
        &self,
        setup: &LockSetup<'_>,
        lockfile_path: &Path,
        platforms: &[Platform],
        requests: Vec<ToolRequest>,
    ) -> Result<LockfileCheck> {
        let mut check = LockfileCheck {
            path: lockfile_path.to_path_buf(),
            tools: 0,
            entries: Vec::new(),
        };
        let config = setup.config;
        let requests: Vec<ToolRequest> = requests
            .into_iter()
            .filter(|request| {
                // `mise lock` skips a tool whose `os` option rules out every platform.
                platforms
                    .iter()
                    .any(|platform| request.is_os_supported_on(platform))
                    // A locked install enforces the lockfile only for the
                    // scopes in `locked_scopes`, never for the global override
                    // config, unless `tool_config.locked` says so.
                    && (config.invocation_locked_for(request.source(), true)
                        || request.tool_config_locked(config, true))
            })
            .collect();
        if !lockfile_path.exists() {
            // System and path versions need no entry.
            if requests.iter().any(|request| {
                !matches!(
                    request,
                    ToolRequest::System { .. } | ToolRequest::Path { .. }
                )
            }) {
                check.push(None, None, vec!["the lockfile does not exist".to_string()]);
            }
            return Ok(check);
        }
        let locked = ResolveOptions {
            use_locked_version: true,
            locked: true,
            offline: true,
            warn_not_in_lockfile: false,
            ..Default::default()
        };
        let host_target = PlatformTarget::from_current();
        for request in requests {
            let resolved = match request.resolve_options(&locked) {
                Ok(opts) => ToolVersion::resolve(config, request.clone(), &opts).await,
                Err(err) => Err(err),
            };
            let host = match resolved {
                Ok(tv) if tv.resolved_from_lockfile() => Ok(tv),
                // Without its plugin a request resolves to itself, before
                // the lockfile is read only when it has no entry there.
                // Offline, `sub-N:latest` has no base to subtract from and
                // resolves to itself too, where a locked install needs an entry.
                Ok(tv)
                    if matches!(tv.request, ToolRequest::Sub { .. })
                        || tv.backend().is_ok_and(|backend| {
                            backend.plugin().is_some_and(|p| !p.is_installed())
                        }) =>
                {
                    Err(not_in_lockfile(&tv.request, &tv.version))
                }
                // Linked, path and system versions are not locked.
                Ok(_) => continue,
                Err(err) => Err(first_line(&err)),
            };
            // A platform whose lock options differ from this host's (Swift's
            // record the Linux distro, a GitHub tool's can hold a
            // per-platform `asset_pattern`) reads the entry recorded with
            // its own options, as a locked install there would. The others
            // read the entry resolved here.
            let basis = host.as_ref().map_or(&request, |tv| &tv.request);
            let backend = basis.backend().ok();
            let lock_options = |target: &PlatformTarget| {
                backend
                    .as_ref()
                    .and_then(|backend| backend.lockfile_options(basis, target).ok())
            };
            let host_options = lock_options(&host_target);
            let variants: Vec<Platform> = platforms
                .iter()
                .filter(|platform| request.is_os_supported_on(platform))
                .flat_map(|platform| match (&host, &backend) {
                    (Ok(tv), Some(backend)) => backend.platform_variants_for(tv, platform),
                    _ => vec![platform.clone()],
                })
                .collect();
            let mut rows = Vec::with_capacity(variants.len());
            let mut own_entries = false;
            for platform in variants {
                let target = PlatformTarget::new(platform.clone());
                let options = lock_options(&target);
                let entry = if options.is_none() || options == host_options {
                    host.clone()
                } else {
                    own_entries = true;
                    match ToolVersion::resolve_locked_for_target(config, &request, &target).await {
                        Ok(Some(tv)) => Ok(tv),
                        Ok(None) => Err(not_in_lockfile(&request, &request.version())),
                        Err(err) => Err(first_line(&err)),
                    }
                };
                rows.push((platform, entry));
            }
            // A tool no platform reads its own entry for fails as a whole.
            if let (Err(problem), false) = (&host, own_entries) {
                check.push_unresolved(&request, None, problem.clone());
                continue;
            }
            let Some(first) = host
                .as_ref()
                .ok()
                .or_else(|| rows.iter().find_map(|(_, entry)| entry.as_ref().ok()))
            else {
                for (platform, entry) in rows {
                    if let Err(problem) = entry {
                        let key = platform.to_key();
                        check.push_unresolved(
                            &request,
                            Some(key.clone()),
                            format!("{problem} on platform {key}"),
                        );
                    }
                }
                continue;
            };
            check.tools += 1;
            if let Some(err) = first
                .backend()
                .ok()
                .and_then(|backend| backend.validate_locked_graph(config, first).err())
            {
                let problem = format!(
                    "{}@{}: {}",
                    first.ba().short,
                    first.version,
                    first_line(&err)
                );
                check.push(Some(first), None, vec![problem]);
            }
            for (platform, entry) in rows {
                let key = platform.to_key();
                let tv = match entry {
                    Ok(tv) => tv,
                    Err(problem) => {
                        check.push_unresolved(
                            &request,
                            Some(key.clone()),
                            format!("{problem} on platform {key}"),
                        );
                        continue;
                    }
                };
                let locked_platform = tv.lock_platforms.get(&key);
                let target = PlatformTarget::new(platform);
                let problem = tv
                    .backend()
                    .ok()
                    .and_then(|backend| {
                        backend
                            .locked_entry_error(&tv, &key, locked_platform)
                            .or_else(|| {
                                backend.locked_artifacts_error(&tv, &target, locked_platform)
                            })
                    })
                    .map(|err| first_line(&err));
                check.push(
                    Some(&tv),
                    Some((key, locked_platform)),
                    problem.into_iter().collect(),
                );
            }
        }
        Ok(check)
    }
}

/// Whether the tool argument `arg` names the configured backend `ba`, as
/// `mise lock` matches them: a bare name by its short name, any argument by
/// its full backend.
fn tool_arg_names(arg: &crate::args::BackendArg, ba: &crate::args::BackendArg) -> bool {
    (!arg.short.contains(':') && arg.short == ba.short) || arg.full() == ba.full()
}

fn not_in_lockfile(request: &ToolRequest, version: &str) -> String {
    format!("{}@{version} is not in the lockfile", request.ba().short)
}
