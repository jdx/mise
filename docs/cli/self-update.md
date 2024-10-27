# `mise self-update`

**Usage**: `mise self-update [FLAGS] [VERSION]`

**Source code**: [`src/cli/self-update.rs`](https://github.com/jdx/mise/blob/main/src/cli/self-update.rs)

Updates mise itself.

Uses the GitHub Releases API to find the latest release and binary.
By default, this will also update any installed plugins.
Uses the `GITHUB_API_TOKEN` environment variable if set for higher rate limits.

This command is not available if mise is installed via a package manager.

## Arguments

### `[VERSION]`

Update to a specific version

## Flags

### `-f --force`

Update even if already up to date

### `--no-plugins`

Disable auto-updating plugins

### `-y --yes`

Skip confirmation prompt
