---
description: "Install and remove openSUSE and SUSE Linux Enterprise packages with zypper from mise.toml."
---

# openSUSE and SUSE packages (zypper)

The `zypper` manager installs and removes packages on openSUSE and SUSE Linux
Enterprise with `zypper`. It uses [sudo](/bootstrap/packages/#sudo) when mise
is not running as root.

```toml
[bootstrap.packages]
"zypper:libopenssl-devel" = "latest"
"zypper:ripgrep" = "latest"
"zypper:bash" = "5.2.37-2.1" # version-release pin
```

```sh
mise bootstrap packages apply --manager zypper --dry-run
mise bootstrap packages apply --manager zypper
```

## Prerequisites

The manager is available on Linux when both `zypper` and `rpm` are on `PATH`. On
other machines, its entries show as
[`skipped`](/bootstrap/packages/#choose-platforms).

mise runs `zypper` directly and does not use `transactional-update`. On systems
with a read-only root filesystem, such as openSUSE MicroOS or Aeon, install
packages with `transactional-update pkg install` instead of declaring them
here.

## Package names

Use the package name, such as `libopenssl-devel`. mise checks installed state
with `rpm -q`, which looks up package names, not capabilities.

## Version pins

mise passes a pin to zypper as `name=version` or `name=version-release`. A
version-only pin accepts any release of that exact version; it does not match a
version prefix. A pinned install adds `--oldpackage`, so it can downgrade.

The pin above is only an example. Choose a version from the host's enabled
repositories; mise does not add repositories or fetch archived RPMs.

## What mise runs

| Operation                       | Command                                                                             |
| ------------------------------- | ----------------------------------------------------------------------------------- |
| Check installed state (no sudo) | `rpm -q <packages>`                                                                 |
| Install                         | `zypper --non-interactive install [--oldpackage] -- <packages>`                     |
| `apply --update`                | `zypper --non-interactive refresh` first                                            |
| Upgrade                         | `zypper --non-interactive refresh`, then the install command for installed packages |
| Remove                          | `zypper --non-interactive remove -- <packages>`                                     |

Without `--update`, zypper follows each repository's automatic refresh setting.
Your zypper dependency, license, and signature policies still apply, and
non-interactive mode fails when a question has no automatic answer.

If zypper exits with 102 (reboot required), mise prints a warning. If it exits
with 103 (zypper updated itself and must restart), mise retries the command up
to twice. Any other nonzero exit, including skipped repositories and failed RPM
scripts, is an error.

## Remove packages

```toml
[bootstrap.packages]
"zypper:ripgrep" = { state = "absent" }
```

`apply` removes an installed package with
`zypper --non-interactive remove`. An entry that is already absent needs
nothing. `mise bootstrap packages apply --dry-run` prints the `zypper remove`
command without running it. To see which dependent packages zypper would remove
as well, run that command yourself with zypper's own `--dry-run` option.
