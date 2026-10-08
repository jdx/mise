---
description: "Install and upgrade Windows packages with WinGet from mise.toml, including version pins."
---

# WinGet packages (winget)

The `winget` manager installs and upgrades Windows packages with the Windows
Package Manager, `winget`.

```toml
[bootstrap.packages]
"winget:BurntSushi.ripgrep.MSVC" = "latest"
"winget:Microsoft.PowerToys" = "0.101.0"
```

```sh
mise bootstrap packages apply --manager winget --dry-run
mise bootstrap packages apply --manager winget
```

## Prerequisites

The manager is available on Windows when `winget.exe` is on `PATH`. On other
machines, `winget:` entries show as
[`skipped`](/bootstrap/packages/#choose-platforms), so one config can hold
packages for several platforms.

Installs and upgrades run silently with interaction disabled, and mise accepts
the package and source agreements that WinGet presents. An installer may still
ask for elevation through a UAC prompt. mise does not bypass UAC or elevate
WinGet itself, and it reports WinGet failures unchanged.

[`mise bootstrap remote`](/bootstrap/remote.html) needs a POSIX shell on the
target and cannot drive a native Windows host, so run `mise bootstrap` on the
Windows machine itself. For Scoop apps, use the
[`scoop`](/bootstrap/packages/scoop.html) manager. Chocolatey is not supported.

## Package names

Use the package ID that `winget search` shows, not its display name. mise
passes the ID with `--id` and `--exact`, so it never accepts a fuzzy match.
WinGet IDs are case-insensitive; spell each ID the same way everywhere to avoid
[conflicting declarations](/bootstrap/packages/#semantics).

## Version pins

mise passes a pin to WinGet as `--version` for both `apply` and `upgrade`. It
must match WinGet's version string exactly. Run
`winget show --id Microsoft.PowerToys --versions` to list the available
versions. A `"latest"` entry is satisfied by any installed version; use
`upgrade` to move it to the newest.

## What mise runs

| Operation             | Command                                                             |
| --------------------- | ------------------------------------------------------------------- |
| Check installed state | `winget list --id <id> --exact`                                     |
| Install               | `winget install --id <id> --exact [--version <pin>] --silent ...`   |
| `apply --update`      | `winget source update` first                                        |
| Upgrade               | `winget source update`, then `winget upgrade --id <id> --exact ...` |

mise runs WinGet once per package.

## Remove packages

mise does not support `state = "absent"`, `import`, or `prune` for WinGet
packages. Deleting an entry leaves the package installed; run
`winget uninstall` yourself.
