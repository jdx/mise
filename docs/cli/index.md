# `mise [flags] [subcommand]`

## Global Flags

### `-C --cd <DIR>`

Change directory before running command

### `-P --profile <PROFILE>`

Set the profile (environment)

### `-q --quiet`

Suppress non-error messages

### `-v --verbose...`

Show extra output (use -vv for even more)

### `-y --yes`

Answer yes to all confirmation prompts

## Subcommands

* [`mise activate [--shims] [-q --quiet] [SHELL_TYPE]`](/cli/activate.md)
* [`mise alias [-p --plugin <PLUGIN>] [--no-header] [subcommand]`](/cli/alias.md)
* [`mise alias get <PLUGIN> <ALIAS>`](/cli/alias/get.md)
* [`mise alias ls [--no-header] [PLUGIN]`](/cli/alias/ls.md)
* [`mise alias set <args>…`](/cli/alias/set.md)
* [`mise alias unset <PLUGIN> <ALIAS>`](/cli/alias/unset.md)
* [`mise backends [subcommand]`](/cli/backends.md)
* [`mise backends ls`](/cli/backends/ls.md)
* [`mise bin-paths`](/cli/bin-paths.md)
* [`mise cache [subcommand]`](/cli/cache.md)
* [`mise cache clear [PLUGIN]...`](/cli/cache/clear.md)
* [`mise cache prune [--dry-run] [-v --verbose...] [PLUGIN]...`](/cli/cache/prune.md)
* [`mise completion [SHELL]`](/cli/completion.md)
* [`mise config [--no-header] [subcommand]`](/cli/config.md)
* [`mise config generate [-o --output <OUTPUT>]`](/cli/config/generate.md)
* [`mise config get [-f --file <FILE>] [KEY]`](/cli/config/get.md)
* [`mise config ls [--no-header]`](/cli/config/ls.md)
* [`mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`](/cli/config/set.md)
* [`mise current [PLUGIN]`](/cli/current.md)
* [`mise deactivate`](/cli/deactivate.md)
* [`mise direnv [subcommand]`](/cli/direnv.md)
* [`mise direnv activate`](/cli/direnv/activate.md)
* [`mise doctor`](/cli/doctor.md)
* [`mise env [-J --json] [-s --shell <SHELL>] [TOOL@VERSION]...`](/cli/env.md)
* [`mise exec [flags] [TOOL@VERSION]... [COMMAND]...`](/cli/exec.md)
* [`mise generate [subcommand]`](/cli/generate.md)
* [`mise generate git-pre-commit [flags]`](/cli/generate/git-pre-commit.md)
* [`mise generate github-action [flags]`](/cli/generate/github-action.md)
* [`mise generate task-docs [flags]`](/cli/generate/task-docs.md)
* [`mise implode [--config] [-n --dry-run]`](/cli/implode.md)
* [`mise install [flags] [TOOL@VERSION]...`](/cli/install.md)
* [`mise latest [-i --installed] <TOOL@VERSION>`](/cli/latest.md)
* [`mise link [-f --force] <TOOL@VERSION> <PATH>`](/cli/link.md)
* [`mise ls [flags] [PLUGIN]...`](/cli/ls.md)
* [`mise ls-remote [--all] [TOOL@VERSION] [PREFIX]`](/cli/ls-remote.md)
* [`mise outdated [flags] [TOOL@VERSION]...`](/cli/outdated.md)
* [`mise plugins [flags] [subcommand]`](/cli/plugins.md)
* [`mise plugins install [flags] [NEW_PLUGIN] [GIT_URL]`](/cli/plugins/install.md)
* [`mise plugins link [-f --force] <NAME> [PATH]`](/cli/plugins/link.md)
* [`mise plugins ls [flags]`](/cli/plugins/ls.md)
* [`mise plugins ls-remote [-u --urls] [--only-names]`](/cli/plugins/ls-remote.md)
* [`mise plugins uninstall [-p --purge] [-a --all] [PLUGIN]...`](/cli/plugins/uninstall.md)
* [`mise plugins update [-j --jobs <JOBS>] [PLUGIN]...`](/cli/plugins/update.md)
* [`mise prune [flags] [PLUGIN]...`](/cli/prune.md)
* [`mise registry`](/cli/registry.md)
* [`mise reshim`](/cli/reshim.md)
* [`mise run [flags]`](/cli/run.md)
* [`mise self-update [flags] [VERSION]`](/cli/self-update.md)
* [`mise set [--file <FILE>] [-g --global] [ENV_VARS]...`](/cli/set.md)
* [`mise settings [--keys] [subcommand]`](/cli/settings.md)
* [`mise settings get <SETTING>`](/cli/settings/get.md)
* [`mise settings ls [--keys]`](/cli/settings/ls.md)
* [`mise settings set <SETTING> <VALUE>`](/cli/settings/set.md)
* [`mise settings unset <SETTING>`](/cli/settings/unset.md)
* [`mise shell [flags] [TOOL@VERSION]...`](/cli/shell.md)
* [`mise sync [subcommand]`](/cli/sync.md)
* [`mise sync node [flags]`](/cli/sync/node.md)
* [`mise sync python <--pyenv>`](/cli/sync/python.md)
* [`mise tasks [flags] [subcommand]`](/cli/tasks.md)
* [`mise tasks deps [--hidden] [--dot] [TASKS]...`](/cli/tasks/deps.md)
* [`mise tasks edit [-p --path] <TASK>`](/cli/tasks/edit.md)
* [`mise tasks info [-J --json] <TASK>`](/cli/tasks/info.md)
* [`mise tasks ls [flags]`](/cli/tasks/ls.md)
* [`mise tasks run [flags] [TASK] [ARGS]...`](/cli/tasks/run.md)
* [`mise trust [flags] [CONFIG_FILE]`](/cli/trust.md)
* [`mise uninstall [-a --all] [-n --dry-run] [INSTALLED_TOOL@VERSION]...`](/cli/uninstall.md)
* [`mise unset [-f --file <FILE>] [-g --global] [KEYS]...`](/cli/unset.md)
* [`mise upgrade [flags] [TOOL@VERSION]...`](/cli/upgrade.md)
* [`mise usage`](/cli/usage.md)
* [`mise use [flags] [TOOL@VERSION]...`](/cli/use.md)
* [`mise version`](/cli/version.md)
* [`mise watch [-t --task... <TASK>] [-g --glob... <GLOB>] [ARGS]...`](/cli/watch.md)
* [`mise where <TOOL@VERSION>`](/cli/where.md)
* [`mise which [flags] <BIN_NAME>`](/cli/which.md)
