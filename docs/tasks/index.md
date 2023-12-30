# Tasks <Badge type="warning" text="experimental" />

You can define tasks in `.rtx.toml` files or as standalone shell scripts. These are useful for things like
running linters, tests, builders, servers, and other tasks that are specific to a project. Of course,
tasks launched with rtx will include the rtx environment—your tools and env vars defined in `.rtx.toml`.

Here's my favorite features about rtx's task runner:

- building dependencies in parallel—by default with no configuration required
- last-modified checking to avoid rebuilding when there are no changes—requires minimal config
- `rtx watch` to automatically rebuild on changes—no configuration required, but it helps
- ability to write tasks as actual bash script files and not inside yml/json/toml strings that lack syntax highlighting and linting/checking support

> [!WARNING]
>
> This is an experimental feature. It is not yet stable and will likely change. Some of the docs
> may not be implemented, may be implemented incorrectly, or the docs may need to be updated.
> Please give feedback early since while it's experimental it's much easier to change.

## Task Environment Variables

- `RTX_PROJECT_ROOT` - the root of the project, defaults to the directory of the `.rtx.toml` file
