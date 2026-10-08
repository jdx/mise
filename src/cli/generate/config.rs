use std::path::PathBuf;

use crate::Result;
use crate::cli::edit::Edit;

/// Generate a mise.toml file
///
/// Same as `mise edit`, which has the details. It opens the interactive editor, or
/// without an interactive terminal (or with --yes) writes a commented starter
/// template. That template replaces PATH if the file already exists.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise generate config",
        help = "Edit mise.toml in the current directory"
    ),
    example(
        "mise generate config -n",
        help = "Print the result instead of writing it"
    )
)]
pub(super) struct Config {
    /// Generate the global config file (~/.config/mise/config.toml)
    // Declared here as well as on `Edit`: this command parses its own arguments before handing
    // them over, so the conflict does not carry across on its own.
    #[usage(long, short = 'g', conflicts = "path")]
    global: bool,
    /// Print the result instead of writing it to the file
    #[usage(long, short = 'n')]
    dry_run: bool,
    /// Config file to edit or create; defaults to mise.toml
    #[usage(verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
    /// Copy the tools from this .tool-versions file into the config, without opening the editor
    #[usage(long, short, value_name = "FILE", verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    tool_versions: Option<PathBuf>,
}

impl Config {
    pub(super) async fn run(self) -> Result<()> {
        Edit::new(self.global, self.dry_run, self.path, self.tool_versions)
            .run()
            .await
    }
}
