## `mise cache prune [OPTIONS] [PLUGIN]...`

**Aliases:** `p`

```text
Removes stale mise cache files

By default, this command will remove files that have not been accessed in 30 days.
Change this with the MISE_CACHE_PRUNE_AGE environment variable.

Usage: cache prune [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to clear cache for e.g.: node, python

Options:
      --dry-run
          Just show what would be pruned

  -v, --verbose...
          Show pruned files
```
