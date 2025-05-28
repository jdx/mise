use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::file::display_path;
use crate::registry::REGISTRY;
use crate::tera::get_tera;
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::time;
use crate::{dirs, env, file};
use eyre::{Result, bail, eyre};
use std::path::PathBuf;
use std::{collections::BTreeSet, sync::Arc};

/// Test a tool installs and executes
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TestTool {
    /// Tool name to test
    #[clap(required_unless_present_any = ["all", "all_config"])]
    pub tool: Option<ToolArg>,
    /// Test every tool specified in registry.toml
    #[clap(long, short, conflicts_with = "tool", conflicts_with = "all_config")]
    pub all: bool,
    /// Test all tools specified in config files
    #[clap(long, conflicts_with = "tool", conflicts_with = "all")]
    pub all_config: bool,
    /// Also test tools not defined in registry.toml, guessing how to test it
    #[clap(long)]
    pub include_non_defined: bool,
    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,
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
        let ts = ToolsetBuilder::new().build(&config).await?;
        let tools: BTreeSet<String> = ts.versions.keys().map(|t| t.short.clone()).collect();
        let mut found = false;
        for (i, (short, rt)) in REGISTRY.iter().enumerate() {
            if *env::TEST_TRANCHE_COUNT > 0 && (i % *env::TEST_TRANCHE_COUNT) != *env::TEST_TRANCHE
            {
                continue;
            }
            let mut tool: ToolArg = short.parse()?;
            if let Some(t) = &self.tool {
                if t.short != tool.short {
                    continue;
                }
                found = true;
                tool = t.clone();
            }
            if self.all_config && !tools.contains(rt.short) {
                continue;
            }
            if self.all && rt.short != *short {
                // means this is an alias
                continue;
            }
            let (cmd, expected) = if let Some(test) = &rt.test {
                (test.0.to_string(), test.1)
            } else if self.include_non_defined || self.tool.is_some() {
                (format!("{} --version", tool.short), "__TODO__")
            } else {
                continue;
            };
            let start = std::time::Instant::now();
            match self.test(&mut config, &tool, &cmd, expected).await {
                Ok(_) => {
                    info!("{}: OK", tool.short);
                    self.github_summary(vec![
                        tool.short.clone(),
                        time::format_duration(start.elapsed()).to_string(),
                        ":white_check_mark:".to_string(),
                    ])?;
                }
                Err(err) => {
                    error!("{}: {:?}", tool.short, err);
                    errored.push(tool.short.clone());
                    self.github_summary(vec![
                        tool.short.clone(),
                        time::format_duration(start.elapsed()).to_string(),
                        ":x:".to_string(),
                    ])?;
                }
            };
        }
        if !found && self.tool.is_some() {
            bail!("{} not found", self.tool.unwrap().short);
        }
        if !errored.is_empty() {
            bail!("tools failed: {}", errored.join(", "));
        }
        Ok(())
    }

    fn github_summary(&self, parts: Vec<String>) -> Result<()> {
        if let Ok(github_summary) = env::var("GITHUB_STEP_SUMMARY") {
            file::append(github_summary, format!("| {} |\n", parts.join(" | ")))?;
        }
        Ok(())
    }

    async fn test(
        &self,
        config: &mut Arc<Config>,
        tool: &ToolArg,
        cmd: &str,
        expected: &str,
    ) -> Result<()> {
        let mut args = vec![tool.clone()];
        args.extend(
            tool.ba
                .backend()?
                .get_all_dependencies(false)?
                .into_iter()
                .map(|ba| ba.to_string().parse())
                .collect::<Result<Vec<ToolArg>>>()?,
        );
        let mut ts = ToolsetBuilder::new()
            .with_args(&args)
            .with_default_to_latest(true)
            .build(config)
            .await?;
        let opts = InstallOptions {
            missing_args_only: false,
            jobs: self.jobs,
            raw: self.raw,
            ..Default::default()
        };
        ts.install_missing_versions(config, &opts).await?;
        ts.notify_if_versions_missing(config).await;
        let tv = if let Some(tv) = ts
            .versions
            .get(tool.ba.as_ref())
            .and_then(|tvl| tvl.versions.first())
        {
            tv.clone()
        } else {
            warn!("no versions found for {tool}");
            return Ok(());
        };
        let backend = tv.backend()?;
        let env = ts.env_with_path(config).await?;
        let mut which_parts = cmd.split_whitespace().collect::<Vec<_>>();
        let cmd = which_parts.remove(0);
        let mut which_cmd = backend
            .which(config, &tv, cmd)
            .await?
            .unwrap_or(PathBuf::from(cmd));
        if cfg!(windows) && which_cmd == PathBuf::from("which") {
            which_cmd = PathBuf::from("where");
        }
        let cmd = format!("{} {}", which_cmd.display(), which_parts.join(" "));
        info!("$ {cmd}");
        let mut cmd = if cfg!(windows) {
            cmd!("cmd", "/C", cmd)
        } else {
            cmd!("sh", "-c", cmd)
        };
        cmd = cmd.stdout_capture();
        for (k, v) in env.iter() {
            cmd = cmd.env(k, v);
        }
        let res = cmd.unchecked().run()?;
        match res.status.code() {
            Some(0) => {}
            Some(code) => {
                if code == 127 {
                    let bin_dirs = backend.list_bin_paths(config, &tv).await?;
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
        let mut ctx = config.tera_ctx.clone();
        ctx.insert("version", &tv.version);
        let mut tera = get_tera(dirs::CWD.as_ref().map(|d| d.as_path()));
        let expected = tera.render_str(expected, &ctx)?;
        let stdout = String::from_utf8(res.stdout)?;
        miseprintln!("{}", stdout.trim_end());
        if !stdout.contains(&expected) {
            return Err(eyre!(
                "expected output not found: {expected}, got: {stdout}"
            ));
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise test-tool ripgrep</bold>
"#
);
