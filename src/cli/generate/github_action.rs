use xx::file;

use crate::file::display_path;
use crate::git::Git;

/// Generate a GitHub Actions workflow that runs a mise task
///
/// The workflow runs the task named by --task on pull requests, tags, manual
/// dispatch, and pushes to the current branch. It prints YAML unless you pass
/// --write, which saves `.github/workflows/<NAME>.yml` in the repository root and
/// replaces an existing file. Define the task and review the triggers before
/// committing the workflow.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise generate github-action",
        help = "Print a workflow that runs the ci task"
    ),
    example(
        "mise generate github-action --write",
        help = "Save it as .github/workflows/ci.yml"
    ),
    example(
        "mise generate github-action --task lint --name lint --write",
        help = "Save a workflow that runs the lint task as .github/workflows/lint.yml"
    )
)]
pub(super) struct GithubAction {
    /// The task to run when the workflow is triggered
    #[usage(long, short, default = "ci")]
    task: String,
    /// Write the workflow to `.github/workflows/<NAME>.yml` instead of printing it
    #[usage(long, short)]
    write: bool,
    /// Name of the workflow and its file
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

jobs:
  {name}:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v7
      - uses: jdx/mise-action@v5
      - run: mise run {task}
"#
        ))
    }
}
