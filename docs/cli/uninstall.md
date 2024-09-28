# `mise uninstall [args] [flags]`

Removes runtime versions

## Arguments

### `[INSTALLED_TOOL@VERSION]...`

Tool(s) to remove

## Flags

### `-a --all`

Delete all installed versions

### `-n --dry-run`

Do not actually delete anything

Examples:

    mise uninstall node@18.0.0 # will uninstall specific version
    mise uninstall node        # will uninstall current node version
    mise uninstall --all node@18.0.0 # will uninstall all node versions
