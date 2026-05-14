use crate::cli::args::ToolArg;
use crate::cmd::cmd;
use crate::config::Config;
use crate::file::display_path;
use crate::registry::{REGISTRY, RegistryTool};
use crate::tera::{contains_template_syntax, get_tera, render_str};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::time;
use crate::{dirs, env, file};
use eyre::{Result, bail, eyre};
use std::path::{Path, PathBuf};
use std::{collections::BTreeSet, sync::Arc};
use tokio::task::JoinSet;

/// Test a tool installs and executes
#[derive(Debug, Clone, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TestTool {
    /// Tool(s) to test
    #[clap(required_unless_present_any = ["all", "all_config"])]
    pub tools: Option<Vec<ToolArg>>,
    /// Test every tool specified in registry/
    #[clap(long, short, conflicts_with = "tools", conflicts_with = "all_config")]
    pub all: bool,
    /// Number of tool tests to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_TEST_TOOL_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,
    /// Test all tools specified in config files
    #[clap(long, conflicts_with = "tools", conflicts_with = "all")]
    pub all_config: bool,
    /// Also test tools not defined in registry/, guessing how to test it
    #[clap(long)]
    pub include_non_defined: bool,
    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    pub raw: bool,
}

impl TestTool {
    pub async fn run(self) -> Result<()> {
        let mut errored = vec![];
        self.github_summary(vec![
            "Tool".to_string(),
            "Duration".to_string(),
            "Status".to_string(),
        ])?;
        self.github_summary(vec![
            "---".to_string(),
            "---".to_string(),
            "---".to_string(),
        ])?;
        let mut config = Config::get().await?;

        let target_tools = self.get_target_tools(&config).await?;
        let mut targets = vec![];
        for (i, (tool, rt)) in target_tools.into_iter().enumerate() {
            if *env::TEST_TRANCHE_COUNT > 0 && (i % *env::TEST_TRANCHE_COUNT) != *env::TEST_TRANCHE
            {
                continue;
            }
            let (cmd, expected, tools) = if let Some(test) = rt.test.as_ref() {
                (
                    test.cmd.to_string(),
                    test.expected.to_string(),
                    test.tools.iter().map(|tool| tool.to_string()).collect(),
                )
            } else if self.include_non_defined {
                (
                    format!("{} --version", tool.short),
                    "__TODO__".to_string(),
                    vec![],
                )
            } else {
                continue;
            };
            targets.push(TestToolTarget {
                index: targets.len(),
                tool,
                cmd,
                expected,
                tools,
            });
        }

        if self.run_in_process(targets.len()) {
            let mut cleaned_any = false;
            for target in &targets {
                cleaned_any |= self.clean(&target.tool)?;
            }
            if cleaned_any {
                config = Config::reset().await?;
            }
        }

        let results = self.test_targets(config, targets).await?;
        for result in results {
            if !result.output.is_empty() {
                miseprintln!("{}", result.output);
            }
            if let Some(err) = result.error {
                if !result.status_logged {
                    error!("{}: {}", result.tool, err);
                }
                errored.push(result.tool.clone());
                self.github_summary(vec![
                    result.tool,
                    time::format_duration(result.duration).to_string(),
                    ":x:".to_string(),
                ])?;
            } else {
                if !result.status_logged {
                    info!("{}: OK", result.tool);
                }
                self.github_summary(vec![
                    result.tool,
                    time::format_duration(result.duration).to_string(),
                    ":white_check_mark:".to_string(),
                ])?;
            };
        }
        if let Ok(github_summary) = env::var("GITHUB_STEP_SUMMARY") {
            let mut content: String = "\n".into();
            if !errored.is_empty() {
                content.push_str(&format!("**Failed Tools**: {}\n", errored.join(", ")));
            }
            file::append(github_summary, content)?;
        }
        if !errored.is_empty() {
            bail!("tools failed: {}", errored.join(", "));
        }
        Ok(())
    }

    async fn test_targets(
        &self,
        config: Arc<Config>,
        targets: Vec<TestToolTarget>,
    ) -> Result<Vec<TestToolResult>> {
        if self.run_in_process(targets.len()) {
            let mut results = vec![];
            for target in targets {
                let tool = target.tool.short.clone();
                let start = std::time::Instant::now();
                let result = match self
                    .test(
                        &config,
                        &target.tool,
                        &target.cmd,
                        &target.expected,
                        &target.tools,
                    )
                    .await
                {
                    Ok(output) => TestToolResult {
                        index: target.index,
                        tool,
                        duration: start.elapsed(),
                        output,
                        error: None,
                        status_logged: false,
                    },
                    Err(err) => TestToolResult {
                        index: target.index,
                        tool,
                        duration: start.elapsed(),
                        output: String::new(),
                        error: Some(format!("{err:?}")),
                        status_logged: false,
                    },
                };
                results.push(result);
            }
            return Ok(results);
        }

        let jobs = self.jobs();
        let mut jset = JoinSet::new();
        let mut results = targets.iter().map(|_| None).collect::<Vec<_>>();

        for target in targets {
            while jset.len() >= jobs {
                Self::collect_child_result(&mut jset, &mut results).await?;
            }
            let this = self.clone();
            jset.spawn(async move {
                tokio::task::spawn_blocking(move || this.test_child(target))
                    .await
                    .map_err(|err| eyre!("task panicked: {err}"))?
            });
        }

        while !jset.is_empty() {
            Self::collect_child_result(&mut jset, &mut results).await?;
        }

        Ok(results.into_iter().flatten().collect())
    }

    async fn collect_child_result(
        jset: &mut JoinSet<Result<TestToolResult>>,
        results: &mut [Option<TestToolResult>],
    ) -> Result<()> {
        if let Some(result) = jset.join_next().await {
            let result = result??;
            let index = result.index;
            results[index] = Some(result);
        }
        Ok(())
    }

    fn test_child(&self, target: TestToolTarget) -> Result<TestToolResult> {
        let start = std::time::Instant::now();
        let exe = std::env::current_exe()?;
        let mut args = vec![
            "test-tool".to_string(),
            "--jobs=1".to_string(),
            target.tool.to_string(),
        ];
        if self.include_non_defined {
            args.insert(1, "--include-non-defined".to_string());
        }

        let res = cmd(exe, args)
            .env_remove("GITHUB_STEP_SUMMARY")
            .env_remove("TEST_TRANCHE")
            .env_remove("TEST_TRANCHE_COUNT")
            .stderr_to_stdout()
            .stdout_capture()
            .unchecked()
            .run()?;
        let output = String::from_utf8(res.stdout)?.trim_end().to_string();
        let error = match res.status.code() {
            Some(0) => None,
            Some(code) => Some(format!("command failed: exit code {code}")),
            None => Some("command failed: terminated by signal".to_string()),
        };
        Ok(TestToolResult {
            index: target.index,
            tool: target.tool.short,
            duration: start.elapsed(),
            output,
            error,
            status_logged: true,
        })
    }

    async fn get_target_tools(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<(ToolArg, &RegistryTool)>> {
        if let Some(tools) = &self.tools {
            let mut targets = Vec::new();
            let mut not_found = Vec::new();
            for tool_arg in tools {
                if let Some(rt) = REGISTRY.get(tool_arg.short.as_str()) {
                    targets.push((tool_arg.clone(), rt));
                } else {
                    not_found.push(tool_arg.short.clone());
                }
            }
            if !not_found.is_empty() {
                bail!("tools not found: {}", not_found.join(", "));
            }
            Ok(targets)
        } else if self.all {
            REGISTRY
                .iter()
                .filter(|(short, rt)| rt.short == *short) // Filter out aliases
                .map(|(short, rt)| short.parse().map(|s| (s, rt)))
                .collect()
        } else if self.all_config {
            let ts = ToolsetBuilder::new().build(config).await?;
            let config_tools = ts
                .versions
                .keys()
                .map(|t| t.short.clone())
                .collect::<BTreeSet<_>>();
            let mut targets = Vec::new();
            for tool in config_tools {
                if let Some(rt) = REGISTRY.get(tool.as_str()) {
                    targets.push((tool.parse()?, rt));
                }
            }
            Ok(targets)
        } else {
            unreachable!()
        }
    }

    fn github_summary(&self, parts: Vec<String>) -> Result<()> {
        if let Ok(github_summary) = env::var("GITHUB_STEP_SUMMARY") {
            file::append(github_summary, format!("| {} |\n", parts.join(" | ")))?;
        }
        Ok(())
    }

    fn jobs(&self) -> usize {
        if self.raw {
            1
        } else {
            self.jobs.unwrap_or(4).max(1)
        }
    }

    fn run_in_process(&self, target_count: usize) -> bool {
        self.jobs() == 1 || target_count <= 1
    }

    fn clean(&self, tool: &ToolArg) -> Result<bool> {
        // First, clean all backend data by removing directories
        let pr = crate::ui::multi_progress_report::MultiProgressReport::get()
            .add(&format!("cleaning {}", tool.short));

        let mut cleaned_any = false;

        // Remove entire installs directory for this tool
        if tool.ba.installs_path.exists() {
            info!(
                "Removing installs directory: {}",
                tool.ba.installs_path.display()
            );
            file::remove_all(&tool.ba.installs_path)?;
            cleaned_any = true;
        }

        // Clear cache directory (contains metadata)
        if tool.ba.cache_path.exists() {
            info!("Removing cache directory: {}", tool.ba.cache_path.display());
            file::remove_all(&tool.ba.cache_path)?;
            cleaned_any = true;
        }

        // Clear downloads directory
        if tool.ba.downloads_path.exists() {
            info!(
                "Removing downloads directory: {}",
                tool.ba.downloads_path.display()
            );
            file::remove_all(&tool.ba.downloads_path)?;
            cleaned_any = true;
        }

        pr.finish();
        Ok(cleaned_any)
    }

    async fn test(
        &self,
        config: &Arc<Config>,
        tool: &ToolArg,
        cmd: &str,
        expected: &str,
        test_tools: &[String],
    ) -> Result<String> {
        let mut config = config.clone();
        let mut args = vec![tool.clone()];
        args.extend(
            tool.ba
                .backend()?
                .get_all_dependencies(false)?
                .into_iter()
                .map(|ba| ba.to_string().parse())
                .collect::<Result<Vec<ToolArg>>>()?,
        );
        args.extend(
            test_tools
                .iter()
                .map(|tool| tool.parse())
                .collect::<Result<Vec<ToolArg>>>()?,
        );
        let mut ts = ToolsetBuilder::new()
            .with_args(&args)
            .with_default_to_latest(true)
            .build(&config)
            .await?;
        let opts = InstallOptions {
            missing_args_only: false,
            jobs: self.jobs,
            raw: self.raw,
            ..Default::default()
        };
        let (_, missing) = ts.install_missing_versions(&mut config, &opts).await?;
        ts.notify_missing_versions(missing);
        let tv = if let Some(tv) = ts
            .versions
            .get(tool.ba.as_ref())
            .and_then(|tvl| tvl.versions.first())
        {
            tv.clone()
        } else {
            warn!("no versions found for {tool}");
            return Ok(String::new());
        };
        let backend = tv.backend()?;
        let env = ts.env_with_path(&config).await?;
        let mut which_parts = cmd.split_whitespace().collect::<Vec<_>>();
        let cmd = which_parts.remove(0);
        let mut which_cmd = backend
            .which(&config, &tv, cmd)
            .await?
            .unwrap_or(PathBuf::from(cmd));
        if cfg!(windows) && which_cmd == Path::new("which") {
            which_cmd = PathBuf::from("where");
        }
        let cmd = format!("{} {}", which_cmd.display(), which_parts.join(" "));
        info!("$ {cmd}");
        let mut cmd = if cfg!(windows) {
            cmd!("cmd", "/C", cmd)
        } else {
            cmd!("sh", "-c", cmd)
        };
        cmd = cmd.stderr_to_stdout().stdout_capture();
        for (k, v) in env.iter() {
            cmd = cmd.env(k, v);
        }
        let res = cmd.unchecked().run()?;
        match res.status.code() {
            Some(0) => {}
            Some(code) => {
                if code == 127 {
                    // Show captured stdout (which may include stderr via 2>&1)
                    // to help diagnose dynamic linker or missing library errors
                    if let Ok(stdout) = String::from_utf8(res.stdout.clone()) {
                        let stdout = stdout.trim();
                        if !stdout.is_empty() {
                            info!("command output:\n{stdout}");
                        }
                    }
                    let bin_dirs = backend.list_bin_paths(&config, &tv).await?;
                    for bin_dir in &bin_dirs {
                        let bins = file::ls(bin_dir)?
                            .into_iter()
                            .filter(|p| file::is_executable(p))
                            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                            .collect::<Vec<_>>();
                        info!(
                            "available bins in {}\n{}",
                            display_path(bin_dir),
                            bins.join("\n")
                        );
                    }
                }
                return Err(eyre!("command failed: exit code {}", code));
            }
            None => return Err(eyre!("command failed: terminated by signal")),
        }
        let expected = if contains_template_syntax(expected) {
            let mut ctx = config.tera_ctx.clone();
            ctx.insert("version", &tv.version);
            let mut tera = get_tera(dirs::CWD.as_ref().map(|d| d.as_path()));
            render_str(&mut tera, expected, &ctx)?
        } else {
            expected.to_string()
        };
        let stdout = String::from_utf8(res.stdout)?;
        let clean_stdout = console::strip_ansi_codes(&stdout);
        if !clean_stdout.contains(&expected) {
            return Err(eyre!(
                "expected output not found: {expected}, got: {clean_stdout}"
            ));
        }
        Ok(stdout.trim_end().to_string())
    }
}

struct TestToolTarget {
    index: usize,
    tool: ToolArg,
    cmd: String,
    expected: String,
    tools: Vec<String>,
}

struct TestToolResult {
    index: usize,
    tool: String,
    duration: std::time::Duration,
    output: String,
    error: Option<String>,
    status_logged: bool,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise test-tool ripgrep</bold>
"#
);
