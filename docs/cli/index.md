# `[flags] [subcommand]`

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

* [`mise activate [args] [flags]`](/cli/activate.md)
* [`mise alias [flags] [subcommand]`](/cli/alias.md)
* [`mise alias get <PLUGIN> <ALIAS>`](/cli/alias/get.md)
* [`mise alias ls [PLUGIN] [--no-header]`](/cli/alias/ls.md)
* [`mise alias set [args]`](/cli/alias/set.md)
* [`mise alias unset <PLUGIN> <ALIAS>`](/cli/alias/unset.md)
* [`mise backends [subcommand]`](/cli/backends.md)
* [`mise backends ls`](/cli/backends/ls.md)
* [`mise bin-paths`](/cli/bin-paths.md)
* [`mise cache [subcommand]`](/cli/cache.md)
* [`mise cache clear [PLUGIN]...`](/cli/cache/clear.md)
* [`mise cache prune [args] [flags]`](/cli/cache/prune.md)
* [`mise completion [args] [flags]`](/cli/completion.md)
* [`mise config [flags] [subcommand]`](/cli/config.md)
* [`mise config generate [-o --output <OUTPUT>]`](/cli/config/generate.md)
* [`mise config get [KEY] [-f --file <FILE>]`](/cli/config/get.md)
* [`mise config ls [--no-header]`](/cli/config/ls.md)
* [`mise config set [args] [flags]`](/cli/config/set.md)
* [`mise current [PLUGIN]`](/cli/current.md)
* [`mise deactivate`](/cli/deactivate.md)
* [`mise direnv [subcommand]`](/cli/direnv.md)
* [`mise direnv activate`](/cli/direnv/activate.md)
* [`mise doctor`](/cli/doctor.md)
* [`mise env [args] [flags]`](/cli/env.md)
* [`mise exec [args] [flags]`](/cli/exec.md)
* [`mise generate [subcommand]`](/cli/generate.md)
* [`mise generate git-pre-commit [flags]`](/cli/generate/git-pre-commit.md)
* [`mise generate github-action [flags]`](/cli/generate/github-action.md)
* [`mise generate task-docs [flags]`](/cli/generate/task-docs.md)
* [`mise implode [--config] [-n --dry-run]`](/cli/implode.md)
* [`mise install [args] [flags]`](/cli/install.md)
* [`mise latest [args] [flags]`](/cli/latest.md)
* [`mise link [args] [flags]`](/cli/link.md)
* [`mise ls [args] [flags]`](/cli/ls.md)
* [`mise ls-remote [args] [flags]`](/cli/ls-remote.md)
* [`mise outdated [args] [flags]`](/cli/outdated.md)
* [`mise plugins [flags] [subcommand]`](/cli/plugins.md)
* [`mise plugins install [args] [flags]`](/cli/plugins/install.md)
* [`mise plugins link [args] [flags]`](/cli/plugins/link.md)
* [`mise plugins ls [flags]`](/cli/plugins/ls.md)
* [`mise plugins ls-remote [-u --urls] [--only-names]`](/cli/plugins/ls-remote.md)
* [`mise plugins uninstall [args] [flags]`](/cli/plugins/uninstall.md)
* [`mise plugins update [PLUGIN]... [-j --jobs <JOBS>]`](/cli/plugins/update.md)
* [`mise prune [args] [flags]`](/cli/prune.md)
* [`mise registry`](/cli/registry.md)
* [`mise reshim`](/cli/reshim.md)
* [`mise run [flags]`](/cli/run.md)
* [`mise self-update [args] [flags]`](/cli/self-update.md)
* [`mise set [args] [flags]`](/cli/set.md)
* [`mise settings [flags] [subcommand]`](/cli/settings.md)
* [`mise settings get <SETTING>`](/cli/settings/get.md)
* [`mise settings ls [--keys]`](/cli/settings/ls.md)
* [`mise settings set <SETTING> <VALUE>`](/cli/settings/set.md)
* [`mise settings unset <SETTING>`](/cli/settings/unset.md)
* [`mise shell [args] [flags]`](/cli/shell.md)
* [`mise sync [subcommand]`](/cli/sync.md)
* [`mise sync node [flags]`](/cli/sync/node.md)
* [`mise sync python [--pyenv]`](/cli/sync/python.md)
* [`mise tasks [flags] [subcommand]`](/cli/tasks.md)
* [`mise tasks deps [args] [flags]`](/cli/tasks/deps.md)
* [`mise tasks edit <TASK> [-p --path]`](/cli/tasks/edit.md)
* [`mise tasks info <TASK> [-J --json]`](/cli/tasks/info.md)
* [`mise tasks ls [flags]`](/cli/tasks/ls.md)
* [`mise tasks run [args] [flags]`](/cli/tasks/run.md)
* [`mise trust [args] [flags]`](/cli/trust.md)
* [`mise uninstall [args] [flags]`](/cli/uninstall.md)
* [`mise unset [args] [flags]`](/cli/unset.md)
* [`mise upgrade [args] [flags]`](/cli/upgrade.md)
* [`mise usage`](/cli/usage.md)
* [`mise use [args] [flags]`](/cli/use.md)
* [`mise version`](/cli/version.md)
* [`mise watch [args] [flags]`](/cli/watch.md)
* [`mise where <TOOL@VERSION>`](/cli/where.md)
* [`mise which [args] [flags]`](/cli/which.md)
