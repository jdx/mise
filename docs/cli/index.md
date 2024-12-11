# `mise`

**Usage**: `mise [FLAGS] [TASK] <SUBCOMMAND>`

- **Usage**: `mise [FLAGS] [TASK] <SUBCOMMAND>`

## Arguments

### `[TASK]`

Task to run.

Shorthand for `mise task run <TASK>`.

## Global Flags

### `-C --cd <DIR>`

Change directory before running command

### `-E --env... <ENV>`

Set the environment for loading `mise.<ENV>.toml`

### `-j --jobs <JOBS>`

How many jobs to run in parallel [default: 8]

### `-q --quiet`

Suppress non-error messages

### `--raw`

Read/write directly to stdin/stdout/stderr instead of by line

### `-v --verbose...`

Show extra output (use -vv for even more)

### `-y --yes`

Answer yes to all confirmation prompts

## Subcommands

- [`mise activate [FLAGS] [SHELL_TYPE]`](/cli/activate.md)
- [`mise alias [-p --plugin <PLUGIN>] [--no-header] <SUBCOMMAND>`](/cli/alias.md)
- [`mise alias get <PLUGIN> <ALIAS>`](/cli/alias/get.md)
- [`mise alias ls [--no-header] [TOOL]`](/cli/alias/ls.md)
- [`mise alias set <ARGS>…`](/cli/alias/set.md)
- [`mise alias unset <PLUGIN> <ALIAS>`](/cli/alias/unset.md)
- [`mise backends <SUBCOMMAND>`](/cli/backends.md)
- [`mise backends ls`](/cli/backends/ls.md)
- [`mise bin-paths [TOOL@VERSION]...`](/cli/bin-paths.md)
- [`mise cache <SUBCOMMAND>`](/cli/cache.md)
- [`mise cache clear [PLUGIN]...`](/cli/cache/clear.md)
- [`mise cache prune [--dry-run] [-v --verbose...] [PLUGIN]...`](/cli/cache/prune.md)
- [`mise completion [SHELL]`](/cli/completion.md)
- [`mise config [--no-header] [-J --json] <SUBCOMMAND>`](/cli/config.md)
- [`mise config generate [-o --output <OUTPUT>]`](/cli/config/generate.md)
- [`mise config get [-f --file <FILE>] [KEY]`](/cli/config/get.md)
- [`mise config ls [--no-header] [-J --json]`](/cli/config/ls.md)
- [`mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`](/cli/config/set.md)
- [`mise deactivate`](/cli/deactivate.md)
- [`mise doctor`](/cli/doctor.md)
- [`mise en [-s --shell <SHELL>] [DIR]`](/cli/en.md)
- [`mise env [FLAGS] [TOOL@VERSION]...`](/cli/env.md)
- [`mise exec [FLAGS] [TOOL@VERSION]... [COMMAND]...`](/cli/exec.md)
- [`mise fmt [-a --all]`](/cli/fmt.md)
- [`mise generate <SUBCOMMAND>`](/cli/generate.md)
- [`mise generate git-pre-commit [FLAGS]`](/cli/generate/git-pre-commit.md)
- [`mise generate github-action [FLAGS]`](/cli/generate/github-action.md)
- [`mise generate task-docs [FLAGS]`](/cli/generate/task-docs.md)
- [`mise implode [--config] [-n --dry-run]`](/cli/implode.md)
- [`mise install [FLAGS] [TOOL@VERSION]...`](/cli/install.md)
- [`mise latest [-i --installed] <TOOL@VERSION>`](/cli/latest.md)
- [`mise link [-f --force] <TOOL@VERSION> <PATH>`](/cli/link.md)
- [`mise ls [FLAGS] [PLUGIN]...`](/cli/ls.md)
- [`mise ls-remote [--all] [TOOL@VERSION] [PREFIX]`](/cli/ls-remote.md)
- [`mise outdated [FLAGS] [TOOL@VERSION]...`](/cli/outdated.md)
- [`mise plugins [FLAGS] <SUBCOMMAND>`](/cli/plugins.md)
- [`mise plugins install [FLAGS] [NEW_PLUGIN] [GIT_URL]`](/cli/plugins/install.md)
- [`mise plugins link [-f --force] <NAME> [DIR]`](/cli/plugins/link.md)
- [`mise plugins ls [-u --urls]`](/cli/plugins/ls.md)
- [`mise plugins ls-remote [-u --urls] [--only-names]`](/cli/plugins/ls-remote.md)
- [`mise plugins uninstall [-p --purge] [-a --all] [PLUGIN]...`](/cli/plugins/uninstall.md)
- [`mise plugins update [-j --jobs <JOBS>] [PLUGIN]...`](/cli/plugins/update.md)
- [`mise prune [FLAGS] [PLUGIN]...`](/cli/prune.md)
- [`mise registry [-b --backend <BACKEND>] [--hide-aliased] [NAME]`](/cli/registry.md)
- [`mise reshim [-f --force]`](/cli/reshim.md)
- [`mise run [FLAGS]`](/cli/run.md)
- [`mise self-update [FLAGS] [VERSION]`](/cli/self-update.md)
- [`mise set [--file <FILE>] [-g --global] [ENV_VARS]...`](/cli/set.md)
- [`mise settings [FLAGS] [KEY] [VALUE] <SUBCOMMAND>`](/cli/settings.md)
- [`mise settings add [-l --local] <KEY> <VALUE>`](/cli/settings/add.md)
- [`mise settings get [-l --local] <KEY>`](/cli/settings/get.md)
- [`mise settings ls [FLAGS] [KEY]`](/cli/settings/ls.md)
- [`mise settings set [-l --local] <KEY> <VALUE>`](/cli/settings/set.md)
- [`mise settings unset [-l --local] <KEY>`](/cli/settings/unset.md)
- [`mise shell [FLAGS] [TOOL@VERSION]...`](/cli/shell.md)
- [`mise sync <SUBCOMMAND>`](/cli/sync.md)
- [`mise sync node [FLAGS]`](/cli/sync/node.md)
- [`mise sync python <--pyenv>`](/cli/sync/python.md)
- [`mise tasks [FLAGS] [TASK] <SUBCOMMAND>`](/cli/tasks.md)
- [`mise tasks deps [--hidden] [--dot] [TASKS]...`](/cli/tasks/deps.md)
- [`mise tasks edit [-p --path] <TASK>`](/cli/tasks/edit.md)
- [`mise tasks info [-J --json] <TASK>`](/cli/tasks/info.md)
- [`mise tasks ls [FLAGS]`](/cli/tasks/ls.md)
- [`mise tasks run [FLAGS] [TASK] [ARGS]...`](/cli/tasks/run.md)
- [`mise tool [FLAGS] <BACKEND>`](/cli/tool.md)
- [`mise trust [FLAGS] [CONFIG_FILE]`](/cli/trust.md)
- [`mise uninstall [-a --all] [-n --dry-run] [INSTALLED_TOOL@VERSION]...`](/cli/uninstall.md)
- [`mise unset [-f --file <FILE>] [-g --global] [KEYS]...`](/cli/unset.md)
- [`mise upgrade [FLAGS] [TOOL@VERSION]...`](/cli/upgrade.md)
- [`mise use [FLAGS] [TOOL@VERSION]...`](/cli/use.md)
- [`mise version`](/cli/version.md)
- [`mise watch [-t --task... <TASK>] [-g --glob... <GLOB>] [ARGS]...`](/cli/watch.md)
- [`mise where <TOOL@VERSION>`](/cli/where.md)
- [`mise which [FLAGS] <BIN_NAME>`](/cli/which.md)
