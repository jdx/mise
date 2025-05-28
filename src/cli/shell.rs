use color_eyre::eyre::{Result, eyre};
use console::style;
use heck::ToShoutySnakeCase;
use indoc::formatdoc;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::env;
use crate::shell::get_shell;
use crate::toolset::{InstallOptions, ToolSource, ToolsetBuilder};

/// Sets a tool version for the current session.
///
/// Only works in a session where mise is already activated.
///
/// This works by setting environment variables for the current shell session
/// such as `MISE_NODE_VERSION=20` which is "eval"ed as a shell function created by `mise activate`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "sh", after_long_help = AFTER_LONG_HELP)]
pub struct Shell {
    /// Tool(s) to use
    #[clap(value_name = "TOOL@VERSION", required = true)]
    tool: Vec<ToolArg>,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,

    /// Removes a previously set version
    #[clap(long, short)]
    unset: bool,
}

impl Shell {
    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        if !env::is_activated() {
            err_inactive()?;
        }

        let shell = get_shell(None).expect("no shell detected");

        if self.unset {
            for ta in &self.tool {
                let op = shell.unset_env(&format!(
                    "MISE_{}_VERSION",
                    ta.ba.short.to_shouty_snake_case()
                ));
                print!("{op}");
            }
            return Ok(());
        }

        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&config)
            .await?;
        let opts = InstallOptions {
            force: false,
            jobs: self.jobs,
            raw: self.raw,
            ..Default::default()
        };
        ts.install_missing_versions(&mut config, &opts).await?;
        ts.notify_if_versions_missing(&config).await;

        for (p, tv) in ts.list_current_installed_versions(&config) {
            let source = &ts.versions.get(p.ba().as_ref()).unwrap().source;
            if matches!(source, ToolSource::Argument) {
                let k = format!("MISE_{}_VERSION", p.id().to_shouty_snake_case());
                let op = shell.set_env(&k, &tv.version);
                print!("{op}");
            }
        }

        Ok(())
    }
}

fn err_inactive() -> Result<()> {
    Err(eyre!(formatdoc!(
        r#"
                mise is not activated in this shell session.
                Please run `{}` first in your shell rc file.
                "#,
        style("mise activate").yellow()
    )))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise shell node@20</bold>
    $ <bold>node -v</bold>
    v20.0.0
"#
);
