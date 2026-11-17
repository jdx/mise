//! `mise lock --check`: whether each lockfile in scope already holds what a
//! locked install needs, read from the lockfile alone. Nothing is resolved
//! remotely and nothing is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use console::style;
use eyre::{Result, bail};
use mise_util::redactions::{redact_json, redact_output};

use super::{Lock, LockInputs, LockSetup, platform_tasks};
use crate::backend::Backend;
use crate::backend::platform_target::PlatformTarget;
use crate::file::display_path;
use crate::lockfile::{self, Lockfile, PlatformInfo};
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

/// The lockfile's entry for `tv` on `platform`. A locked install there picks
/// the entry recorded with that platform's lock options (Swift's record the
/// Linux distro, for one), which need not be the entry `tv` resolved to here.
fn locked_platform_info<'a>(
    backend: &dyn Backend,
    lockfile: &'a Lockfile,
    tv: &'a ToolVersion,
    platform: &Platform,
    current_options: Option<&BTreeMap<String, String>>,
) -> Option<&'a PlatformInfo> {
    let key = platform.to_key();
    match backend.lockfile_options(&tv.request, &PlatformTarget::new(platform.clone())) {
        Ok(options) if current_options != Some(&options) => lockfile
            .tools_for(&tv.ba().short)?
            .iter()
            .find(|entry| entry.version == tv.version && entry.options == options)?
            .platforms
            .get(&key),
        _ => tv.lock_platforms.get(&key),
    }
}

impl Lock {
    /// Check every lockfile target, then report and fail if any problem was found.
    pub(super) async fn check_lockfiles(
        &self,
        setup: &LockSetup<'_>,
        inputs: &LockInputs,
    ) -> Result<()> {
        let mut checks = Vec::with_capacity(inputs.lockfile_targets.len());
        for (lockfile_path, config_paths) in &inputs.lockfile_targets {
            let requests = self.check_requests(setup, lockfile_path, config_paths)?;
            if requests.is_empty() {
                continue;
            }
            checks.push(self.check_lockfile(setup, lockfile_path, requests).await?);
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
    ) -> Result<Vec<ToolRequest>> {
        let config = setup.config;
        let named = |ba: &crate::args::BackendArg| {
            self.tool.is_empty() || self.tool.iter().any(|tool| tool.ba.as_ref() == ba)
        };
        // `mise lock` also locks the Python a pipx tool selects, for the
        // tools it locks or, with `mise lock python`, for every tool.
        let python_named = self.tool.iter().any(|tool| tool.ba.short == "python");
        let mut pythons: Vec<ToolRequest> = Vec::new();
        let mut select_python = |ba: &crate::args::BackendArg, request: &ToolRequest| {
            if named(ba) || python_named {
                pythons.extend(crate::backend::pipx::selected_python_dependency(request)?);
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
        pythons.into_iter().for_each(&mut push);
        Ok(requests)
    }

    /// Resolve each request from the lockfile as `--locked` does, and check
    /// each platform's entry the way a locked install would.
    async fn check_lockfile(
        &self,
        setup: &LockSetup<'_>,
        lockfile_path: &Path,
        requests: Vec<ToolRequest>,
    ) -> Result<LockfileCheck> {
        let mut check = LockfileCheck {
            path: lockfile_path.to_path_buf(),
            tools: 0,
            entries: Vec::new(),
        };
        if !lockfile_path.exists() {
            check.push(None, None, vec!["the lockfile does not exist".to_string()]);
            return Ok(check);
        }
        let platforms = self.determine_target_platforms(lockfile_path)?;
        let lockfile = Lockfile::read(lockfile_path)?;
        let locked = ResolveOptions {
            use_locked_version: true,
            locked: true,
            offline: true,
            warn_not_in_lockfile: false,
            ..Default::default()
        };
        for request in requests {
            // `mise lock` skips a tool whose `os` option rules out every platform.
            if !platforms
                .iter()
                .any(|platform| request.is_os_supported_on(platform))
            {
                continue;
            }
            let resolved = match request.resolve_options(&locked) {
                Ok(opts) => ToolVersion::resolve(setup.config, request.clone(), &opts).await,
                Err(err) => Err(err),
            };
            let tv = match resolved {
                Ok(tv) if tv.resolved_from_lockfile() => tv,
                // Without its plugin a request resolves to itself, before
                // the lockfile is read only when it has no entry there.
                Ok(tv)
                    if tv.backend().is_ok_and(|backend| {
                        backend.plugin().is_some_and(|p| !p.is_installed())
                    }) =>
                {
                    check.push(
                        Some(&tv),
                        None,
                        vec![format!(
                            "{}@{} is not in the lockfile",
                            tv.ba().short,
                            tv.version
                        )],
                    );
                    continue;
                }
                // Linked, path and system versions are not locked.
                Ok(_) => continue,
                Err(err) => {
                    check.entries.push(CheckEntry {
                        lockfile: display_path(lockfile_path),
                        tool: Some(request.ba().short.clone()),
                        version: Some(request.version()),
                        platform: None,
                        provenance: None,
                        problems: vec![first_line(&err)],
                    });
                    continue;
                }
            };
            check.tools += 1;
            let backend = tv.backend().ok();
            if let Some(err) = backend
                .as_ref()
                .and_then(|backend| backend.validate_locked_graph(setup.config, &tv).err())
            {
                let problem = format!("{}@{}: {}", tv.ba().short, tv.version, first_line(&err));
                check.push(Some(&tv), None, vec![problem]);
            }
            let current_options = backend.as_ref().and_then(|backend| {
                backend
                    .lockfile_options(&tv.request, &PlatformTarget::from_current())
                    .ok()
            });
            for (_, tv, platform) in platform_tasks(&[(tv.ba().clone(), tv.clone())], &platforms) {
                let key = platform.to_key();
                // The entry a locked install on this platform would use.
                let entry = match &backend {
                    Some(backend) => locked_platform_info(
                        backend.as_ref(),
                        &lockfile,
                        &tv,
                        &platform,
                        current_options.as_ref(),
                    ),
                    None => tv.lock_platforms.get(&key),
                };
                let problem = backend
                    .as_ref()
                    .and_then(|backend| backend.locked_entry_error(&tv, &key, entry))
                    .map(|err| first_line(&err));
                check.push(Some(&tv), Some((key, entry)), problem.into_iter().collect());
            }
        }
        Ok(check)
    }
}
