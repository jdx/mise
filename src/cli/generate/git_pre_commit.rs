use xx::file::display_path;

use crate::config::Settings;
use crate::file;
use crate::git::Git;

/// [experimental] Generate a git pre-commit hook
///
/// This command generates a git pre-commit hook that runs a mise task like `mise run pre-commit`
/// when you commit changes to your repository.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "pre-commit", after_long_help = AFTER_LONG_HELP)]
pub struct GitPreCommit {
    /// Which hook to generate (saves to .git/hooks/$hook)
    #[clap(long, default_value = "pre-commit")]
    hook: String,
    /// The task to run when the pre-commit hook is triggered
    #[clap(long, short, default_value = "pre-commit")]
    task: String,
    /// write to .git/hooks/pre-commit and make it executable
    #[clap(long, short)]
    write: bool,
}

impl GitPreCommit {
    pub fn run(self) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("generate git-pre-commit")?;
        let output = self.generate();
        if self.write {
            let path = Git::get_root()?.join(".git/hooks").join(&self.hook);
            file::write(&path, &output)?;
            file::make_executable(&path)?;
            miseprintln!("Wrote to {}", display_path(&path));
        } else {
            miseprintln!("{output}");
        }
        Ok(())
    }

    fn generate(&self) -> String {
        let task = &self.task;
        format!(
            r#"#!/bin/sh
mise run {task}
"#
        )
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate git-pre-commit --write --task=pre-commit</bold>
    $ <bold>git commit -m "feat: add new feature"</bold> <dim># runs `mise run pre-commit`</dim>
"#
);

#[cfg(test)]
mod tests {
    use test_log::test;

    use crate::file;
    use crate::git::Git;
    use crate::test::{cleanup, reset, setup_git_repo};

    #[test]
    fn test_git_pre_commit() {
        reset();
        setup_git_repo();
        assert_cli_snapshot!("generate", "pre-commit", "--task=testing123");
        cleanup();
    }
    #[test]
    fn test_git_pre_commit_write() {
        reset();
        setup_git_repo();
        assert_cli_snapshot!("generate", "pre-commit", "-w", "--hook", "testing123");
        let path = Git::get_root().unwrap().join(".git/hooks/testing123");
        assert_snapshot!(file::read_to_string(&path).unwrap());
        assert!(file::is_executable(&path));
        cleanup();
    }
}
