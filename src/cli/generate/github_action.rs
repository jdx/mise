use xx::file;

use crate::config::Settings;
use crate::file::display_path;
use crate::git::Git;

/// [experimental] Generate a Github Action workflow file
///
/// This command generates a Github Action workflow file that runs a mise task like `mise run ci`
/// when you push changes to your repository.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct GithubAction {
    /// the name of the workflow to generate
    #[clap(long, short, default_value = "ci")]
    name: String,
    /// The task to run when the workflow is triggered
    #[clap(long, short, default_value = "ci")]
    task: String,
    /// write to .github/workflows/$name.yml
    #[clap(long, short)]
    write: bool,
}

impl GithubAction {
    pub fn run(self) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("generate github-action")?;
        let output = self.generate()?;
        if self.write {
            let path = Git::get_root()?
                .join(".github/workflows")
                .join(format!("{}.yml", &self.name));
            file::write(&path, &output)?;
            miseprintln!("Wrote to {}", display_path(&path));
        } else {
            miseprintln!("{output}");
        }
        Ok(())
    }

    fn generate(&self) -> eyre::Result<String> {
        let branch = Git::new(Git::get_root()?).current_branch()?;
        let name = &self.name;
        let task = &self.task;
        Ok(format!(
            r#"name: {name}

on:
  workflow_dispatch:
  pull_request:
  push:
    tags: ["*"]
    branches: ["{branch}"]

concurrency:
  group: ${{{{ github.workflow }}}}-${{{{ github.ref }}}}
  cancel-in-progress: true

env:
  MISE_EXPERIMENTAL: true

jobs:
  {name}:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4
      - uses: jdx/mise-action@v2
      - run: mise run {task}
"#
        ))
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate github-action --write --task=ci</bold>
    $ <bold>git commit -m "feat: add new feature"</bold>
    $ <bold>git push</bold> <dim># runs `mise run ci` on Github</dim>
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::file;
    use crate::git::Git;
    use crate::test::{cleanup, reset, setup_git_repo};

    #[test]
    fn test_github_action() {
        reset();
        setup_git_repo();
        assert_cli_snapshot!("generate", "github-action");
        cleanup();
    }
    #[test]
    fn test_github_action_write() {
        reset();
        setup_git_repo();
        assert_cli_snapshot!(
            "generate",
            "github-action",
            "-w",
            "-ttesting123",
            "-n=testing123"
        );
        let path = Git::get_root()
            .unwrap()
            .join(".github/workflows/testing123.yml");
        let contents = file::read_to_string(path).unwrap();
        assert_snapshot!(contents);
        cleanup();
    }
}
