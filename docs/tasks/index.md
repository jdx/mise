# Tasks <Badge type="warning" text="experimental" />

You can define tasks in `.mise.toml` files or as standalone shell scripts. These are useful for things like
running linters, tests, builders, servers, and other tasks that are specific to a project. Of course,
tasks launched with mise will include the mise environment—your tools and env vars defined in `.mise.toml`.

Here's my favorite features about mise's task runner:

- building dependencies in parallel—by default with no configuration required
- last-modified checking to avoid rebuilding when there are no changes—requires minimal config
- `mise watch` to automatically rebuild on changes—no configuration required, but it helps
- ability to write tasks as actual bash script files and not inside yml/json/toml strings that lack syntax highlighting and linting/checking support

::: warning
This is an experimental feature. It is not yet stable and will likely change. Some of the docs
may not be implemented, may be implemented incorrectly, or the docs may need to be updated.
Please give feedback early since while it's experimental it's much easier to change.
:::

## Task Environment Variables

- `root` - the root of the project, defaults to the directory of the `.mise.toml` file
