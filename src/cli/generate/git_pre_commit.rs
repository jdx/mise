use xx::file::display_path;

use crate::config::Settings;
use crate::file;
use crate::git::Git;

/// [experimental] Generate a git pre-commit hook
///
/// This command generates a git pre-commit hook that runs a mise task like `mise run pre-commit`
/// when you commit changes to your repository.
///
/// Staged files are passed to the task via appended arguments
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "pre-commit", after_long_help = AFTER_LONG_HELP)]
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
            if path.exists() {
                let old_path = path.with_extension("old");
                miseprintln!(
                    "Moving existing hook to {:?}",
                    old_path.file_name().unwrap()
                );
                file::rename(&path, path.with_extension("old"))?;
            }
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
            r#"#! /bin/sh
set -eu

PIPE=$(mktemp -u "mise.{task}.XXXXXXXX")
mkfifo -m 600 "${{PIPE}}"

cleanup() {{
  rm -f "${{PIPE}}"
}}
trap 'cleanup' EXIT INT TERM

git diff-index --cached --name-only HEAD > "${{PIPE}}" &
while read -r ARG; do
  set -- "$@" "${{ARG}}"
done < "${{PIPE}}"

exec mise run "{task}" "$@"
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
