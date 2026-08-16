use crate::file::display_path;

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
    /// mise flags to embed in the generated hook, given after `--`
    ///
    /// These are inserted between `mise` and `run`, so the hook carries the same context you
    /// would pass on the command line. Useful when the config is not at the repository root,
    /// since git runs hooks from the top level: `-- -C subdir` makes the hook find it.
    #[clap(last = true, value_name = "MISE_ARG", verbatim_doc_comment)]
    mise_args: Vec<String>,
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
        // Quoted rather than joined with spaces so an argument that contains one — a
        // `-C` pointing at a directory with a space, say — survives as a single word.
        let mise_args = if self.mise_args.is_empty() {
            String::new()
        } else {
            format!(" {}", shell_words::join(&self.mise_args))
        };
        // `"$@"` forwards whatever git passes the hook. `pre-commit` is called with no
        // arguments so it is unaffected, but every other `--hook` target gets some — a
        // `commit-msg` hook is handed the path to the message file, for instance — and
        // without this they are dropped before the task ever sees them.
        format!(
            r#"#!/bin/sh
STAGED="$(git diff-index --cached --name-only -z HEAD | xargs -0)"
export STAGED
export MISE_PRE_COMMIT=1
exec mise{mise_args} run {task} "$@"
"#
        )
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate git-pre-commit --write --task=pre-commit</bold>
    $ <bold>git commit -m "feat: add new feature"</bold> <dim># runs `mise run pre-commit`</dim>

    <dim># config lives in a subdirectory, so the hook has to change into it first</dim>
    $ <bold>mise generate git-pre-commit --write -- -C subdir</bold>
"#
);

#[cfg(test)]
mod tests {
    use super::GitPreCommit;

    fn generate(task: &str, hook: &str) -> String {
        generate_with_args(task, hook, &[])
    }

    fn generate_with_args(task: &str, hook: &str, mise_args: &[&str]) -> String {
        GitPreCommit {
            task: task.to_string(),
            write: false,
            hook: hook.to_string(),
            mise_args: mise_args.iter().map(|s| s.to_string()).collect(),
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

    /// Frozen copy of the hook as it was before the passthrough existed.
    const HOOK_WITHOUT_MISE_ARGS: &str = r#"#!/bin/sh
STAGED="$(git diff-index --cached --name-only -z HEAD | xargs -0)"
export STAGED
export MISE_PRE_COMMIT=1
exec mise run pre-commit "$@"
"#;

    /// Passing nothing must leave the hook byte-identical, so the ones already written
    /// stay valid. Compared whole rather than by substring: an extra or duplicated line
    /// would slip past a `contains` check.
    #[test]
    fn no_mise_args_leaves_the_hook_unchanged() {
        assert_eq!(generate("pre-commit", "pre-commit"), HOOK_WITHOUT_MISE_ARGS);
    }

    /// git runs hooks from the repository root, so a config kept in a subdirectory is only
    /// reachable if the hook carries the flag that gets mise there (discussion #4304).
    #[test]
    fn mise_args_are_inserted_before_run() {
        let out = generate_with_args("lint", "pre-commit", &["-C", "subdir", "-E", "ci"]);
        assert!(
            out.contains(r#"exec mise -C subdir -E ci run lint "$@""#),
            "{out}"
        );
    }

    /// What matters is that a shell reading the hook sees the same argument boundaries we
    /// were given — not which quoting style `shell_words::join` happened to pick. Splitting
    /// the line back apart asserts the property instead of the spelling.
    #[test]
    fn an_argument_containing_a_space_stays_one_word() {
        let out = generate_with_args("lint", "pre-commit", &["-C", "my dir"]);
        let exec_line = out
            .lines()
            .find(|line| line.starts_with("exec mise"))
            .expect("generated hook should exec mise");
        let words = shell_words::split(exec_line).expect("exec line should be valid shell");
        assert_eq!(words, ["exec", "mise", "-C", "my dir", "run", "lint", "$@"]);
    }

    /// The tests above build the struct directly, so they cannot see the question this
    /// feature actually turns on: `-C` and `-E` are global flags, and without `last = true`
    /// clap would bind them to `Cli` instead of handing them to the hook — leaving the
    /// generated script silently unchanged. Parse a real command line to pin that down.
    #[test]
    fn passthrough_args_reach_the_command_through_the_parser() {
        use clap::CommandFactory;

        let matches = crate::cli::Cli::command()
            .try_get_matches_from([
                "mise",
                "generate",
                "git-pre-commit",
                "--task",
                "lint",
                "--",
                "-C",
                "subdir",
                "-E",
                "ci",
            ])
            .expect("the documented invocation should parse");
        let matches = matches
            .subcommand_matches("generate")
            .and_then(|m| m.subcommand_matches("git-pre-commit"))
            .expect("git-pre-commit should be the resolved subcommand");

        let mise_args: Vec<&str> = matches
            .get_many::<String>("mise_args")
            .expect("passthrough args should be captured")
            .map(String::as_str)
            .collect();
        assert_eq!(mise_args, ["-C", "subdir", "-E", "ci"]);

        // Same tokens, not consumed as the global flags they look like.
        assert!(matches.get_one::<std::path::PathBuf>("cd").is_none());
        assert!(matches.get_many::<String>("env").is_none());
        assert_eq!(
            matches.get_one::<String>("task").map(String::as_str),
            Some("lint")
        );
    }
}
