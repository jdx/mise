# `mise lock`

- **Usage**: `mise lock [FLAGS] [TOOL]…`
- **Source code**: [`src/cli/lock.rs`](https://github.com/jdx/mise/blob/main/src/cli/lock.rs)

Update lockfile checksums and URLs for all specified platforms

Updates checksums and download URLs for all platforms already specified in the lockfile.
If no lockfile exists, shows what would be created based on the current configuration.
This allows you to refresh lockfile data for platforms other than the one you're currently on.
Operates on the lockfile in the current config root. Use TOOL arguments to target specific tools.

## Arguments

### `[TOOL]…`

Tool(s) to update in lockfile
e.g.: node python
If not specified, all tools in lockfile will be updated

## Flags

### `-p --platform… <PLATFORM>`

Comma-separated list of platforms to target
e.g.: linux-x64,macos-arm64,windows-x64
If not specified, all platforms already in lockfile will be updated

### `-f --force`

Update all tools even if lockfile data already exists

### `-n --dry-run`

Show what would be updated without making changes

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
[default: 4]

Examples:
  
  $ mise lock                           Update lockfile in current directory for all platforms
  $ mise lock node python              Update only node and python
  $ mise lock --platform linux-x64     Update only linux-x64 platform
  $ mise lock --dry-run                Show what would be updated or created
  $ mise lock --force                  Re-download and update even if data exists
