---
description: "Install Fedora, RHEL, Rocky Linux, and AlmaLinux packages with dnf from mise.toml."
---

# Fedora and RHEL packages (dnf)

The `dnf` manager installs packages on Fedora, RHEL, CentOS Stream, Rocky
Linux, AlmaLinux, and other distributions that use `dnf`. It uses
[sudo](/bootstrap/packages/#sudo) when mise is not running as root.

```toml
[bootstrap.packages]
"dnf:openssl-devel" = "latest"
"dnf:postgresql-server" = "latest"
"dnf:bash" = "5.2.26-3.fc40" # version-release pin
```

```sh
mise bootstrap packages apply --manager dnf --dry-run
mise bootstrap packages apply --manager dnf
```

## Prerequisites

The manager is available on Linux when `dnf` is on `PATH`. Systems that have
only `yum`, such as CentOS 7, are not supported. On other machines, its entries
show as [`skipped`](/bootstrap/packages/#choose-platforms).

## Package names

Use the package name, such as `openssl-devel`, not a capability or file path.
mise checks installed state with `rpm -q`, which looks up package names.

## Version pins

mise passes a pin to dnf as `name-version` or `name-version-release`. A
version-only pin, such as `"5.2.26"`, accepts any release of that version. The
Fedora pin above is only an example: release strings differ between
distributions and releases. Pick a version from the target's enabled
repositories; mise does not add a repository or fetch an archived RPM.

## What mise runs

| Operation                       | Command                               |
| ------------------------------- | ------------------------------------- |
| Check installed state (no sudo) | `rpm -q <packages>`                   |
| Install                         | `dnf install -y <packages>`           |
| `apply --update`                | `dnf install -y --refresh <packages>` |
| Upgrade                         | `dnf upgrade -y --refresh <packages>` |

Without `--update`, dnf refreshes its metadata when its own cache expires.
`upgrade` acts only on configured packages that are installed.

## Remove packages

mise does not remove dnf packages. Deleting an entry leaves the package
installed; run `dnf remove` yourself.
