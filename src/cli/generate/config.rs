use std::path::PathBuf;

use crate::Result;
use crate::cli::edit::Edit;

/// Generate a mise.toml file
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise generate config             # generate mise.toml interactively
mise generate config .mise.toml  # generate a specific file
mise generate config -g          # generate the global config file
mise generate config -y          # skip interactive editor
mise generate config -y --force  # replace an existing file with the default template
mise generate config -n          # preview without writing"###
    )
)]
pub(super) struct Config {
    /// Generate the global config file (~/.config/mise/config.toml)
    // Declared here as well as on `Edit`: this command parses its own arguments before handing
    // them over, so the conflict does not carry across on its own.
    #[usage(long, short = 'g', conflicts = "path")]
    global: bool,
    /// Show what would be generated without writing to file
    #[usage(long, short = 'n')]
    dry_run: bool,
    /// Overwrite an existing file with the default template when not editing interactively
    #[usage(long, short)]
    force: bool,
    /// Path to the config file to create
    #[usage(verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
    /// Path to a .tool-versions file to import tools from
    #[usage(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    tool_versions: Option<PathBuf>,
}

impl Config {
    pub(super) async fn run(self) -> Result<()> {
        Edit::new(
            self.global,
            self.dry_run,
            self.force,
            self.path,
            self.tool_versions,
        )
        .run()
        .await
    }
}
