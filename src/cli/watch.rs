use std::process::exit;

use console::style;
use eyre::{eyre, Result};

use crate::cli::args::ForgeArg;
use crate::cmd;
use crate::config::{Config, Settings};
use crate::toolset::ToolsetBuilder;

/// [experimental] Run a tasks watching for changes
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "w", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Watch {
    /// Tasks to run
    #[clap(short, long, verbatim_doc_comment, default_value = "default")]
    task: Vec<String>,

    /// Extra arguments
    #[clap(allow_hyphen_values = true)]
    args: Vec<String>,

    /// Files to watch
    /// Defaults to sources from the tasks(s)
    #[clap(short, long, verbatim_doc_comment)]
    glob: Vec<String>,
    // /// Change to this directory before executing the command
    // #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    // pub cd: Option<PathBuf>,
    //
    // /// Don't actually run the tasks(s), just print them in order of execution
    // #[clap(long, short = 'n', verbatim_doc_comment)]
    // pub dry_run: bool,
    //
    // /// Force the tasks to run even if outputs are up to date
    // #[clap(long, short, verbatim_doc_comment)]
    // pub force: bool,
    //
    // /// Print stdout/stderr by line, prefixed with the tasks's label
    // /// Defaults to true if --jobs > 1
    // /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    // #[clap(long, short, verbatim_doc_comment, overrides_with = "interleave")]
    // pub prefix: bool,
    //
    // /// Print directly to stdout/stderr instead of by line
    // /// Defaults to true if --jobs == 1
    // /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    // #[clap(long, short, verbatim_doc_comment, overrides_with = "prefix")]
    // pub interleave: bool,
    //
    // /// Tool(s) to also add
    // /// e.g.: node@20 python@3.10
    // #[clap(short, long, value_name = "TOOL@VERSION")]
    // pub tool: Vec<ToolArg>,
    //
    // /// Number of tasks to run in parallel
    // /// [default: 4]
    // /// Configure with `jobs` config or `MISE_JOBS` env var
    // #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    // pub jobs: Option<usize>,
    //
    // /// Read/write directly to stdin/stdout/stderr instead of by line
    // /// Configure with `raw` config or `MISE_RAW` env var
    // #[clap(long, short, verbatim_doc_comment)]
    // pub raw: bool,
}

impl Watch {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        let ts = ToolsetBuilder::new().build(&config)?;
        settings.ensure_experimental("`mise watch`")?;
        if let Err(err) = which::which("watchexec") {
            let watchexec: ForgeArg = "watchexec".into();
            if !ts.versions.contains_key(&watchexec) {
                eprintln!("{}: {}", style("Error").red().bold(), err);
                eprintln!("{}: Install watchexec with:", style("Hint").bold());
                eprintln!("  mise use -g watchexec@latest");
                exit(1);
            }
        }
        let tasks = self
            .task
            .iter()
            .map(|t| {
                config
                    .tasks_with_aliases()?
                    .get(t)
                    .cloned()
                    .ok_or_else(|| eyre!("Tasks not found: {t}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut args = vec![];
        let globs = if self.glob.is_empty() {
            tasks
                .iter()
                .flat_map(|t| t.sources.clone())
                .collect::<Vec<_>>()
        } else {
            self.glob.clone()
        };
        if !globs.is_empty() {
            args.push("-f".to_string());
            args.extend(itertools::intersperse(globs, "-f".to_string()).collect::<Vec<_>>());
        }
        args.extend(self.args.clone());
        args.extend(["--".to_string(), "mise".to_string(), "run".to_string()]);
        for arg in itertools::intersperse(tasks.iter().map(|t| t.name.as_str()), ":::") {
            args.push(arg.to_string());
        }
        info!("$ watchexec {}", args.join(" "));
        let mut cmd = cmd::cmd("watchexec", &args);
        for (k, v) in ts.env_with_path(&config)? {
            cmd = cmd.env(k, v);
        }
        if let Some(root) = &config.project_root {
            cmd = cmd.dir(root);
        }
        cmd.run()?;
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    $ <bold>mise watch -t build</bold>
    Runs the "build" tasks. Will re-run the tasks when any of its sources change.
    Uses "sources" from the tasks definition to determine which files to watch.

    $ <bold>mise watch -t build --glob src/**/*.rs</bold>
    Runs the "build" tasks but specify the files to watch with a glob pattern.
    This overrides the "sources" from the tasks definition.

    $ <bold>mise run -t build --clear</bold>
    Extra arguments are passed to watchexec. See `watchexec --help` for details.
"#
);
