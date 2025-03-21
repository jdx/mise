use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::install_context::InstallContext;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use clap::ValueHint;
use eyre::{Result, eyre};
use std::path::PathBuf;

/// Install a tool version to a specific path
///
/// Used for building a tool to a directory for use outside of mise
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct InstallInto {
    /// Tool to install
    /// e.g.: node@20
    #[clap(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// Path to install the tool into
    #[clap(value_hint = ValueHint::DirPath)]
    path: PathBuf,
}

impl InstallInto {
    pub fn run(self) -> Result<()> {
        let config = Config::get();
        let ts = ToolsetBuilder::new()
            .with_args(&[self.tool.clone()])
            .build(&config)?;
        let mut tv = ts
            .versions
            .get(&self.tool.ba)
            .ok_or_else(|| eyre!("Tool not found"))?
            .versions
            .first()
            .unwrap()
            .clone();
        let backend = tv.backend()?;
        let mpr = MultiProgressReport::get();
        let install_ctx = InstallContext {
            ts: &ts,
            pr: mpr.add(&tv.style()),
            force: true,
        };
        tv.install_path = Some(self.path.clone());
        backend.install_version(install_ctx, tv)?;
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # install node@20.0.0 into ./mynode
    $ <bold>mise install-into node@20.0.0 ./mynode && ./mynode/bin/node -v</bold>
    20.0.0
"#
);
