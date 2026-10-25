---
description: "Install and upgrade Windows packages with Chocolatey from mise.toml, including version pins."
---

# Chocolatey packages (choco)

The `choco` manager installs and upgrades Windows packages with
[Chocolatey](https://chocolatey.org), which installs software machine-wide.

```toml
[bootstrap.packages]
"choco:git" = "latest"
"choco:docker-engine" = "latest"
"choco:visualstudio2022buildtools" = "117.12.0"
```

```sh
mise bootstrap packages apply --manager choco --dry-run
mise bootstrap packages apply --manager choco
```

## Prerequisites

Install Chocolatey first. The manager is available on Windows when `choco.exe`
is on `PATH`. On other machines, `choco:` entries show as
[`skipped`](/bootstrap/packages/#choose-platforms), so one config can hold
packages for several platforms.

Chocolatey installs into machine-wide locations, so `apply` and `upgrade` must
run in an elevated (administrator) terminal. mise does not bypass UAC or
elevate Chocolatey itself, and it reports Chocolatey failures unchanged.
`status` only reads Chocolatey's local package list and works without elevation.

[`mise bootstrap remote`](/bootstrap/remote.html) needs a POSIX shell on the
target and cannot drive a native Windows host, so run `mise bootstrap` on the
Windows machine itself. For per-user apps, use the
[`scoop`](/bootstrap/packages/scoop.html) manager; for the Windows Package
Manager, use [`winget`](/bootstrap/packages/winget.html).

## Package names

Use the package ID that `choco search` shows or that appears in the
[community repository](https://community.chocolatey.org/packages). mise passes
the ID as given and matches installed packages by exact ID, never a fuzzy match.
Chocolatey IDs are case-insensitive; spell each ID the same way everywhere to
avoid [conflicting declarations](/bootstrap/packages/#semantics).

## Version pins

mise passes a pin to Chocolatey as `--version` for both `apply` and `upgrade`.
It must match Chocolatey's version string exactly. A pin below the installed
version downgrades the package, so mise adds `--allow-downgrade`. Run
`choco search <id> --exact --all-versions` to list the available versions. A
`"latest"` entry is satisfied by any installed version; use `upgrade` to move it
to the newest.

## What mise runs

| Operation             | Command                                                                      |
| --------------------- | ---------------------------------------------------------------------------- |
| Check installed state | `choco list --local-only --exact --limit-output <id>`                        |
| Install               | `choco install <id> [--version <pin> --allow-downgrade] --yes --no-progress` |
| Upgrade               | `choco upgrade <id> [--version <pin> --allow-downgrade] --yes --no-progress` |

mise runs Chocolatey once per package. Chocolatey 2.x lists only local
packages and may reject `--local-only`, so mise retries the query without it. Chocolatey has no separate metadata
refresh, so `--update` changes nothing. A package that installs but asks for a
reboot (exit code 1641 or 3010) counts as installed, and mise warns that the
reboot is still pending.

## Remove packages

mise does not support `state = "absent"`, `import`, or `prune` for Chocolatey
packages. Deleting an entry leaves the package installed; run
`choco uninstall` yourself.
