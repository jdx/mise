# `mise cache prune`

**Usage**: `mise cache prune [--dry-run] [-v --verbose...] [PLUGIN]...`

**Source code**: [`src/cli/cache/prune.rs`](https://github.com/jdx/mise/blob/main/src/cli/cache/prune.rs)

**Aliases**: `p`

Removes stale mise cache files

By default, this command will remove files that have not been accessed in 30 days.
Change this with the MISE_CACHE_PRUNE_AGE environment variable.

## Arguments

### `[PLUGIN]...`

Plugin(s) to clear cache for e.g.: node, python

## Flags

### `--dry-run`

Just show what would be pruned

### `-v --verbose...`

Show pruned files
