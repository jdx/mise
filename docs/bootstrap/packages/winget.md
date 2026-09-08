---
description: Converge local Windows packages with WinGet during mise bootstrap.
---

# WinGet

Windows applications and packages via the Windows Package Manager CLI.

```toml
[bootstrap.packages]
"winget:BurntSushi.ripgrep.MSVC" = "latest"
"winget:Microsoft.PowerToys" = "0.101.0"
```

Use the package identifier shown by `winget search`, not a display name. mise
passes the identifier to WinGet with `--id` and `--exact`, so bootstrap never
accepts an ambiguous fuzzy match.

## Commands

```sh
mise bootstrap packages use winget:BurntSushi.ripgrep.MSVC
mise bootstrap packages status
mise bootstrap packages apply --manager winget
mise bootstrap packages apply --manager winget --update
mise bootstrap packages upgrade --manager winget
```

`mise bootstrap packages status` runs `winget list --id <ID> --exact` and
compares the installed version with an optional pin as an opaque string.
`"latest"` is satisfied by any installed version; use the upgrade command to
move it to the newest version available from the configured WinGet sources.

Apply and upgrade operations run silently with interaction disabled and accept
the package and source agreements exposed by WinGet. An installer may still
require Windows elevation through UAC. mise does not bypass UAC or try to
elevate the WinGet process itself, and it surfaces WinGet failures unchanged.

`--update` refreshes WinGet sources before applying missing packages. Upgrade
always refreshes the sources first. Declarative removal, package import, and
prune are not supported by the initial WinGet integration.

## Availability and scope

The manager is available only on Windows when `winget.exe` is on `PATH`.
Shared configs may contain `winget:` entries alongside Linux or macOS package
entries; unavailable managers are reported as skipped and do not block the
other platform's bootstrap.

This support is local to the Windows machine running `mise bootstrap`.
[`mise bootstrap remote`](/bootstrap/remote.html) still requires a POSIX-shell
target; native Windows SSH/PowerShell targets are not supported yet. Scoop and
Chocolatey are also outside this first implementation.
