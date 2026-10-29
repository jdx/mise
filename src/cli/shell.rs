use color_eyre::eyre::{Result, bail, eyre};
use console::style;
use indoc::formatdoc;

use crate::args::ToolArg;
use crate::config::Config;
use crate::env;
use crate::shell::{EXAMPLE_SHELL, require_shell};
use crate::toolset::{
    InstallOptions, ToolSource, ToolsetBuilder, tool_env_var_name, tool_from_env_var_name,
};

/// Set a tool version for the current shell session
///
/// Installs the version if needed, then sets `MISE_<TOOL>_VERSION`, such as
/// `MISE_NODE_VERSION`, to the resolved version in the current shell. That
/// overrides config files until you close the shell or run
/// `mise shell --unset <TOOL>`, or `mise shell --unset --all` to remove every
/// version set this way. Requires `mise activate`: the shell function it
/// defines applies the variable that this command prints.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    visible_alias = "sh",
    example(
        r###"mise shell node@20
node -v
v20.19.5"###,
        help = "Use node 20 in this shell"
    ),
    example(
        r###"mise shell --unset node"###,
        help = "Go back to the version the config requests"
    ),
    example(
        r###"mise shell --unset --all"###,
        help = "Drop every version set with `mise shell` or `MISE_<TOOL>_VERSION`"
    )
)]
pub(crate) struct Shell {
    /// Tools to set, such as node@20
    #[usage(value_name = "TOOL@VERSION", required_unless = "all")]
    tool: Vec<ToolArg>,

    /// Number of jobs to run in parallel (default: the `jobs` setting)
    #[usage(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Remove a version set earlier with `mise shell`
    #[usage(long, short)]
    unset: bool,

    /// With `--unset`, remove every `MISE_<TOOL>_VERSION` in this shell
    ///
    /// This includes any you exported yourself, not only those set by `mise shell`.
    #[usage(long, verbatim_doc_comment, conflicts = ["tool"])]
    all: bool,

    /// Connect the install commands' stdin, stdout, and stderr to the terminal
    ///
    /// Implies `--jobs=1`.
    #[usage(long, overrides = "jobs")]
    raw: bool,
}

impl Shell {
    pub(crate) async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        if !env::is_activated() {
            err_inactive()?;
        }

        let shell = require_shell(
            None,
            &format!("Re-run `mise activate {EXAMPLE_SHELL}` in your shell rc file."),
        )?;

        if self.all && !self.unset {
            bail!("--all can only be used with --unset");
        }

        if self.unset {
            if self.all {
                // Match on the name alone: `tool_env_vars()` also drops variables whose
                // value is not valid UTF-8, which would leave them set.
                for (key, _) in env::vars_os() {
                    let Some(key) = key.to_str().filter(|k| tool_from_env_var_name(k).is_some())
                    else {
                        continue;
                    };
                    miseprint!("{}", shell.unset_env(key))?;
                }
                return Ok(());
            }
            for ta in &self.tool {
                let op = shell.unset_env(&tool_env_var_name(&ta.ba.short));
                miseprint!("{op}")?;
            }
            return Ok(());
        }

        for ta in &self.tool {
            if ta.tvr.is_none() {
                bail!(
                    "no version specified for tool {tool}\nuse `mise shell {tool}@VERSION` to set a version",
                    tool = ta.ba.short,
                );
            }
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
        let (_, missing) = ts.install_missing_versions(&mut config, &opts).await?;
        ts.notify_missing_versions(missing);

        for (p, tv) in ts.list_current_installed_versions(&config) {
            let source = &ts.versions.get(p.ba().as_ref()).unwrap().source;
            if matches!(source, ToolSource::Argument) {
                let k = tool_env_var_name(p.id());
                let op = shell.set_env(&k, &tv.version);
                miseprint!("{op}")?;
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
