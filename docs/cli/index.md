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

### `-E --env… <ENV>`

Set the environment for loading `mise.<ENV>.toml`

### `-j --jobs <JOBS>`

How many jobs to run in parallel [default: 8]

### `--raw`

Read/write directly to stdin/stdout/stderr instead of by line

### `-y --yes`

Answer yes to all confirmation prompts

### `-q --quiet`

Suppress non-error messages

### `--silent`

Suppress all task output and mise non-error messages

### `-v --verbose…`

Show extra output (use -vv for even more)

## Flags

### `--output <OUTPUT>`

### `--no-config`

Do not load any config files

Can also use `MISE_NO_CONFIG=1`

## Subcommands

- [`mise activate [FLAGS] [SHELL_TYPE]`](/cli/activate.md)
- [`mise alias [-p --plugin <PLUGIN>] [--no-header] <SUBCOMMAND>`](/cli/alias.md)
- [`mise alias get <PLUGIN> <ALIAS>`](/cli/alias/get.md)
- [`mise alias ls [--no-header] [TOOL]`](/cli/alias/ls.md)
- [`mise alias set <ARGS>…`](/cli/alias/set.md)
- [`mise alias unset <PLUGIN> <ALIAS>`](/cli/alias/unset.md)
- [`mise backends <SUBCOMMAND>`](/cli/backends.md)
- [`mise backends ls`](/cli/backends/ls.md)
- [`mise bin-paths [TOOL@VERSION]…`](/cli/bin-paths.md)
- [`mise cache <SUBCOMMAND>`](/cli/cache.md)
- [`mise cache clear [PLUGIN]…`](/cli/cache/clear.md)
- [`mise cache prune [--dry-run] [-v --verbose…] [PLUGIN]…`](/cli/cache/prune.md)
- [`mise completion [--include-bash-completion-lib] [SHELL]`](/cli/completion.md)
- [`mise config [FLAGS] <SUBCOMMAND>`](/cli/config.md)
- [`mise config generate [-t --tool-versions <TOOL_VERSIONS>] [-o --output <OUTPUT>]`](/cli/config/generate.md)
- [`mise config get [-f --file <FILE>] [KEY]`](/cli/config/get.md)
- [`mise config ls [FLAGS]`](/cli/config/ls.md)
- [`mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`](/cli/config/set.md)
- [`mise deactivate`](/cli/deactivate.md)
- [`mise doctor [-J --json] <SUBCOMMAND>`](/cli/doctor.md)
- [`mise doctor path [-f --full]`](/cli/doctor/path.md)
- [`mise en [-s --shell <SHELL>] [DIR]`](/cli/en.md)
- [`mise env [FLAGS] [TOOL@VERSION]…`](/cli/env.md)
- [`mise exec [FLAGS] [TOOL@VERSION]… [-- COMMAND]…`](/cli/exec.md)
- [`mise fmt [FLAGS]`](/cli/fmt.md)
- [`mise generate <SUBCOMMAND>`](/cli/generate.md)
- [`mise generate bootstrap [FLAGS]`](/cli/generate/bootstrap.md)
- [`mise generate config [-t --tool-versions <TOOL_VERSIONS>] [-o --output <OUTPUT>]`](/cli/generate/config.md)
- [`mise generate devcontainer [FLAGS]`](/cli/generate/devcontainer.md)
- [`mise generate git-pre-commit [FLAGS]`](/cli/generate/git-pre-commit.md)
- [`mise generate github-action [FLAGS]`](/cli/generate/github-action.md)
- [`mise generate task-docs [FLAGS]`](/cli/generate/task-docs.md)
- [`mise generate task-stubs [-m --mise-bin <MISE_BIN>] [-d --dir <DIR>]`](/cli/generate/task-stubs.md)
- [`mise generate tool-stub [FLAGS] <OUTPUT>`](/cli/generate/tool-stub.md)
- [`mise implode [--config] [-n --dry-run]`](/cli/implode.md)
- [`mise install [FLAGS] [TOOL@VERSION]…`](/cli/install.md)
- [`mise install-into <TOOL@VERSION> <PATH>`](/cli/install-into.md)
- [`mise latest [-i --installed] <TOOL@VERSION>`](/cli/latest.md)
- [`mise link [-f --force] <TOOL@VERSION> <PATH>`](/cli/link.md)
- [`mise lock [FLAGS] [TOOL]…`](/cli/lock.md)
- [`mise ls [FLAGS] [INSTALLED_TOOL]…`](/cli/ls.md)
- [`mise ls-remote [--all] [TOOL@VERSION] [PREFIX]`](/cli/ls-remote.md)
- [`mise mcp`](/cli/mcp.md)
- [`mise outdated [FLAGS] [TOOL@VERSION]…`](/cli/outdated.md)
- [`mise plugins [FLAGS] <SUBCOMMAND>`](/cli/plugins.md)
- [`mise plugins install [FLAGS] [NEW_PLUGIN] [GIT_URL]`](/cli/plugins/install.md)
- [`mise plugins link [-f --force] <NAME> [DIR]`](/cli/plugins/link.md)
- [`mise plugins ls [-u --urls]`](/cli/plugins/ls.md)
- [`mise plugins ls-remote [-u --urls] [--only-names]`](/cli/plugins/ls-remote.md)
- [`mise plugins uninstall [-p --purge] [-a --all] [PLUGIN]…`](/cli/plugins/uninstall.md)
- [`mise plugins update [-j --jobs <JOBS>] [PLUGIN]…`](/cli/plugins/update.md)
- [`mise prune [FLAGS] [INSTALLED_TOOL]…`](/cli/prune.md)
- [`mise registry [-b --backend <BACKEND>] [--hide-aliased] [NAME]`](/cli/registry.md)
- [`mise reshim [-f --force]`](/cli/reshim.md)
- [`mise run [FLAGS]`](/cli/run.md)
- [`mise search [FLAGS] [NAME]`](/cli/search.md)
- [`mise self-update [FLAGS] [VERSION]`](/cli/self-update.md)
- [`mise set [--file <FILE>] [-g --global] [ENV_VAR]…`](/cli/set.md)
- [`mise settings [FLAGS] [SETTING] [VALUE] <SUBCOMMAND>`](/cli/settings.md)
- [`mise settings add [-l --local] <SETTING> <VALUE>`](/cli/settings/add.md)
- [`mise settings get [-l --local] <SETTING>`](/cli/settings/get.md)
- [`mise settings ls [FLAGS] [SETTING]`](/cli/settings/ls.md)
- [`mise settings set [-l --local] <SETTING> <VALUE>`](/cli/settings/set.md)
- [`mise settings unset [-l --local] <KEY>`](/cli/settings/unset.md)
- [`mise shell [FLAGS] <TOOL@VERSION>…`](/cli/shell.md)
- [`mise sync <SUBCOMMAND>`](/cli/sync.md)
- [`mise sync node [FLAGS]`](/cli/sync/node.md)
- [`mise sync python [--pyenv] [--uv]`](/cli/sync/python.md)
- [`mise sync ruby [--brew]`](/cli/sync/ruby.md)
- [`mise tasks [FLAGS] [TASK] <SUBCOMMAND>`](/cli/tasks.md)
- [`mise tasks add [FLAGS] <TASK> [-- RUN]…`](/cli/tasks/add.md)
- [`mise tasks deps [--hidden] [--dot] [TASKS]…`](/cli/tasks/deps.md)
- [`mise tasks edit [-p --path] <TASK>`](/cli/tasks/edit.md)
- [`mise tasks info [-J --json] <TASK>`](/cli/tasks/info.md)
- [`mise tasks ls [FLAGS]`](/cli/tasks/ls.md)
- [`mise tasks run [FLAGS] [TASK] [ARGS]…`](/cli/tasks/run.md)
- [`mise test-tool [FLAGS] [TOOLS]…`](/cli/test-tool.md)
- [`mise tool [FLAGS] <TOOL>`](/cli/tool.md)
- [`mise tool-stub <FILE> [ARGS]…`](/cli/tool-stub.md)
- [`mise trust [FLAGS] [CONFIG_FILE]`](/cli/trust.md)
- [`mise uninstall [-a --all] [-n --dry-run] [INSTALLED_TOOL@VERSION]…`](/cli/uninstall.md)
- [`mise unset [-f --file <FILE>] [-g --global] [ENV_KEY]…`](/cli/unset.md)
- [`mise unuse [FLAGS] <INSTALLED_TOOL@VERSION>…`](/cli/unuse.md)
- [`mise upgrade [FLAGS] [TOOL@VERSION]…`](/cli/upgrade.md)
- [`mise use [FLAGS] [TOOL@VERSION]…`](/cli/use.md)
- [`mise version [-J --json]`](/cli/version.md)
- [`mise watch [FLAGS] [TASK] [ARGS]…`](/cli/watch.md)
- [`mise where <TOOL@VERSION>`](/cli/where.md)
- [`mise which [FLAGS] [BIN_NAME]`](/cli/which.md)
