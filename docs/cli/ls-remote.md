# `mise ls-remote`

- **Usage**: `mise ls-remote [--all] [TOOL@VERSION] [PREFIX]`
- **Source code**: [`src/cli/ls-remote.rs`](https://github.com/jdx/mise/blob/main/src/cli/ls-remote.rs)

List runtime versions available for install.

Note that the results may be cached, run `mise cache clean` to clear the cache and get fresh results.

## Arguments

### `[TOOL@VERSION]`

Tool to get versions for

### `[PREFIX]`

The version prefix to use when querying the latest version
same as the first argument after the "@"

## Flags

### `--all`

Show all installed plugins and versions

Examples:

```
$ mise ls-remote node
18.0.0
20.0.0
```

```
$ mise ls-remote node@20
20.0.0
20.1.0
```

```
$ mise ls-remote node 20
20.0.0
20.1.0
```
