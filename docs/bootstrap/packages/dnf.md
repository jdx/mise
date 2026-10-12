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

## Package groups

A name starting with `@`, such as `"dnf:@development-tools"`, is a dnf package
group. mise checks it with `dnf group list --installed` instead of `rpm -q`,
installs it with `dnf install -y @<group>`, and upgrades it with
`dnf group upgrade`. Groups have no version, so use `"latest"`. Environment
groups (`@^name`) are not supported.

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

## Fedora Atomic hosts

On a host booted from an ostree image (Silverblue, Kinoite, Bazzite, Bluefin,
secureblue), `/usr` is read-only, so `dnf install` cannot work. mise detects
this from `/run/ostree-booted`. Builds inside a bootc Containerfile are not
booted from an image, so `dnf` runs normally there.

By default mise stops with an explanation, because layering packages is
discouraged on these systems and a config written for Fedora Workstation should
not silently layer dozens of them. Status checks still work, since `rpm -q` sees
the packages in the booted image. To let mise layer packages with `rpm-ostree`,
opt in:

```toml
[settings]
system_packages.ostree = "layer"
```

With `layer`:

- `apply` installs all missing packages in one `rpm-ostree install
--idempotent --allow-inactive` transaction (each call builds a new
  deployment), after `rpm-ostree refresh-md` with `--update`, and prints a
  reboot notice. It uses [sudo](/bootstrap/packages/#sudo) as usual.
- Packages already requested for the next deployment count as installed and show
  `pending reboot`, so the next run does not request them again.
- Version pins are skipped with a warning, since pinning a layered package works
  against base-image updates.
- `state = "absent"` runs `rpm-ostree uninstall`.
- `upgrade` prints a hint instead of upgrading anything: layered packages only
  update together with the base image, with `rpm-ostree upgrade`.

| Operation (`layer`) | Command                                                       |
| ------------------- | ------------------------------------------------------------- |
| Install             | `rpm-ostree install --idempotent --allow-inactive <packages>` |
| `apply --update`    | `rpm-ostree refresh-md`, then the install command             |
| Remove              | `rpm-ostree uninstall <packages>`                             |

## Remove packages

Outside layered ostree hosts, mise does not remove dnf packages. Deleting an entry leaves the package
installed; run `dnf remove` yourself.
