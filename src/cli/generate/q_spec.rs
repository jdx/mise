use crate::cli::Cli;
use crate::file;
use std::path::PathBuf;
use std::fs;

/// [experimental] Generate an Amazon Q spec file
///
/// This command generates a Amazon Q spec file that can be compiled to generate autocomplete
/// for Amazon Q CLI through `@withfig/autocomplete-tools` npm package
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct QSpec {
    /// path to output to
    #[clap(long, short, default_value = "~/.fig/autocomplete/src/mise.ts")]
    output: Option<PathBuf>,
}

impl QSpec {
    pub fn run(self) -> eyre::Result<()> {
        let mut out = vec![];
        clap_complete::generate(
            clap_complete_fig::Fig,
            &mut Cli::command(),
            "mise",
            &mut out,
        );
        if let Some(output) = &self.output {
            if let Some(parent) = output.parent() { fs::create_dir_all(parent)? };
            file::write(output, &out)?;
        }

        // TODO: Add automatically generator object for mise tasks
        return Ok(());
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate q-spec -o completions/mise.ts</bold>
    $ <bold>A new spec file is created that can be used for Amazon CloudWhisperer CLI</bold>
"#
);