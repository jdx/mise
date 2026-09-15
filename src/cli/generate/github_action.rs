use xx::file;

use crate::file::display_path;
use crate::git::Git;

/// Generate a GitHub Action workflow file
///
/// This command generates a GitHub Action workflow file that runs a mise task like `mise run ci`
/// on pull requests, tags, manual dispatch, and pushes to the current Git branch.
/// Prints YAML by default; `--write` saves it under .github/workflows. Define
/// the selected task and review the generated triggers before committing.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise generate github-action --task=ci
mise generate github-action --write --task=ci
git add .github/workflows/ci.yml"###,
        help = r###"Preview before writing the workflow"###
    )
)]
pub(super) struct GithubAction {
    /// The task to run when the workflow is triggered
    #[usage(long, short, default = "ci")]
    task: String,
    /// Write to .github/workflows/$name.yml
    #[usage(long, short)]
    write: bool,
    /// The name of the workflow to generate
    #[usage(long, default = "ci")]
    name: String,
}

impl GithubAction {
    pub(super) async fn run(self) -> eyre::Result<()> {
        let output = self.generate()?;
        if self.write {
            let path = Git::get_root()?
                .join(".github/workflows")
                .join(format!("{}.yml", self.name));
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
      - uses: actions/checkout@v6
      - uses: jdx/mise-action@v3
      - run: mise run {task}
"#
        ))
    }
}
