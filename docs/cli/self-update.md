## `mise self-update [OPTIONS] [VERSION]`

```text
Updates mise itself

Uses the GitHub Releases API to find the latest release and binary.
By default, this will also update any installed plugins.
Uses the `GITHUB_API_TOKEN` environment variable if set for higher rate limits.

Usage: self-update [OPTIONS] [VERSION]

Arguments:
  [VERSION]
          Update to a specific version

Options:
  -f, --force
          Update even if already up to date

      --no-plugins
          Disable auto-updating plugins

  -y, --yes
          Skip confirmation prompt
```
