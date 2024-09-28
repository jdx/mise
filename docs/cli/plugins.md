# `mise plugins [flags] [subcommand]`

Manage plugins

## Flags

### `-c --core`

The built-in plugins only
Normally these are not shown

### `--user`

List installed plugins

This is the default behavior but can be used with --core
to show core and user plugins

### `-u --urls`

Show the git url for each plugin
e.g.: <https://github.com/asdf-vm/asdf-nodejs.git>

## Subcommands

* [`mise plugins install [args] [flags]`](/cli/plugins/install.md)
* [`mise plugins link [args] [flags]`](/cli/plugins/link.md)
* [`mise plugins ls [flags]`](/cli/plugins/ls.md)
* [`mise plugins ls-remote [-u --urls] [--only-names]`](/cli/plugins/ls-remote.md)
* [`mise plugins uninstall [args] [flags]`](/cli/plugins/uninstall.md)
* [`mise plugins update [PLUGIN]... [-j --jobs <JOBS>]`](/cli/plugins/update.md)
