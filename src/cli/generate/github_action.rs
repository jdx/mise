use xx::file;

use crate::config::Settings;
use crate::file::display_path;
use crate::git::Git;

/// [experimental] Generate a GitHub Action workflow file
///
/// This command generates a GitHub Action workflow file that runs a mise task like `mise run ci`
/// when you push changes to your repository.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct GithubAction {
    /// the name of the workflow to generate
    #[clap(long, default_value = "ci")]
    name: String,
    /// The task to run when the workflow is triggered
    #[clap(long, short, default_value = "ci")]
    task: String,
    /// write to .github/workflows/$name.yml
    #[clap(long, short)]
    write: bool,
}

impl GithubAction {
    pub async fn run(self) -> eyre::Result<()> {
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
    $ <bold>git push</bold> <dim># runs `mise run ci` on GitHub</dim>
"#
);
