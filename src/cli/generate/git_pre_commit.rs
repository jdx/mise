use xx::file::display_path;

use crate::config::Settings;
use crate::file;
use crate::git::Git;

/// Generate a git pre-commit hook
///
/// This command generates a git pre-commit hook that runs a mise task like `mise run pre-commit`
/// when you commit changes to your repository.
///
/// Staged files are passed to the task as `STAGED`.
///
/// For more advanced pre-commit functionality, see mise's sister project: https://hk.jdx.dev/
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "pre-commit", after_long_help = AFTER_LONG_HELP)]
pub struct GitPreCommit {
    /// The task to run when the pre-commit hook is triggered
    #[clap(long, short, default_value = "pre-commit")]
    task: String,
    /// write to .git/hooks/pre-commit and make it executable
    #[clap(long, short)]
    write: bool,
    /// Which hook to generate (saves to .git/hooks/$hook)
    #[clap(long, default_value = "pre-commit")]
    hook: String,
}

impl GitPreCommit {
    pub async fn run(self) -> eyre::Result<()> {
        let output = self.generate();
        if self.write {
            let quiet = Settings::get().quiet;
            let path = Git::get_path("hooks")?.join(&self.hook);
            if path.exists() {
                let old_path = path.with_extension("old");
                if !quiet {
                    miseprintln!(
                        "Moving existing hook to {:?}",
                        old_path.file_name().unwrap()
                    );
                }
                file::rename(&path, path.with_extension("old"))?;
            }
            file::write(&path, &output)?;
            file::make_executable(&path)?;
            if !quiet {
                miseprintln!("Wrote to {}", display_path(&path));
            }
        } else {
            miseprintln!("{output}");
        }
        Ok(())
    }

    fn generate(&self) -> String {
        let task = &self.task;
        // `"$@"` forwards whatever git passes the hook. `pre-commit` is called with no
        // arguments so it is unaffected, but every other `--hook` target gets some — a
        // `commit-msg` hook is handed the path to the message file, for instance — and
        // without this they are dropped before the task ever sees them.
        format!(
            r#"#!/bin/sh
STAGED="$(git diff-index --cached --name-only -z HEAD | xargs -0)"
export STAGED
export MISE_PRE_COMMIT=1
exec mise run {task} "$@"
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
    use super::GitPreCommit;

    fn generate(task: &str, hook: &str) -> String {
        GitPreCommit {
            task: task.to_string(),
            write: false,
            hook: hook.to_string(),
        }
        .generate()
    }

    #[test]
    fn forwards_hook_arguments_to_the_task() {
        // Without the passthrough a `commit-msg` hook cannot reach the message file git
        // hands it, which is the whole point of that hook.
        let out = generate("lint-commit-msg", "commit-msg");
        assert!(
            out.contains(r#"exec mise run lint-commit-msg "$@""#),
            "hook arguments must reach the task:\n{out}"
        );
    }

    #[test]
    fn pre_commit_output_is_unchanged_in_substance() {
        let out = generate("pre-commit", "pre-commit");
        assert!(out.starts_with("#!/bin/sh\n"), "{out}");
        assert!(out.contains("export MISE_PRE_COMMIT=1"), "{out}");
        assert!(out.contains("STAGED="), "{out}");
    }
}
