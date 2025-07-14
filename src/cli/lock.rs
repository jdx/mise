use crate::{Result, file};
use clap::Parser;
use eyre::Context;
use std::path::Path;

/// Create a lockfile
///
/// This command creates an empty mise.lock file in the current directory.
/// Lockfiles are used to pin tool versions for reproducible environments.
#[derive(Debug, Clone, Parser)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Lock {
    /// The lockfile to create
    #[clap(long, short, default_value = "mise.lock")]
    pub file: String,
}

impl Lock {
    pub async fn run(self) -> Result<()> {
        let lockfile_path = Path::new(&self.file);
        
        if lockfile_path.exists() {
            eprintln!("Lockfile {} already exists", self.file);
            return Ok(());
        }
        
        file::write(lockfile_path, "")
            .with_context(|| format!("Failed to create lockfile {}", self.file))?;
        
        eprintln!("Created lockfile {}", self.file);
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise lock</bold>
    Created lockfile mise.lock

    $ <bold>mise lock --file my-project.lock</bold>
    Created lockfile my-project.lock
"#
);