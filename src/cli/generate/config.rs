use std::path::PathBuf;

use crate::Result;
use crate::cli::edit::Edit;

/// Generate a mise.toml file
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(super) struct Config {
    /// Generate the global config file (~/.config/mise/config.toml)
    // Declared here as well as on `Edit`: this command parses its own arguments before handing
    // them over, so the conflict does not carry across on its own.
    #[usage(long, short = 'g', conflicts = "path")]
    global: bool,
    /// Show what would be generated without writing to file
    #[usage(long, short = 'n')]
    dry_run: bool,
    /// Path to the config file to create
    #[usage(verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
    /// Path to a .tool-versions file to import tools from
    #[usage(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    tool_versions: Option<PathBuf>,
}

impl Config {
    pub(super) async fn run(self) -> Result<()> {
        Edit::new(self.global, self.dry_run, self.path, self.tool_versions)
            .run()
            .await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate config</bold>             <dim># generate mise.toml interactively</dim>
    $ <bold>mise generate config .mise.toml</bold>  <dim># generate a specific file</dim>
    $ <bold>mise generate config -g</bold>          <dim># generate the global config file</dim>
    $ <bold>mise generate config -y</bold>          <dim># skip interactive editor</dim>
    $ <bold>mise generate config -n</bold>          <dim># preview without writing</dim>
"#
);
