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

## COPR repositories {#copr}

Declare [COPR](https://copr.fedorainfracloud.org/) projects in
`[bootstrap.dnf.repos]`. mise enables them before it installs or upgrades `dnf:`
packages, so a package can come from a COPR without a `pre-packages` hook:

```toml
[bootstrap.dnf.repos]
ghostty = { copr = "scottames/ghostty" }
caddy = { copr = "@caddy/caddy" }

[bootstrap.packages]
"dnf:ghostty" = "latest"
"dnf:caddy" = "latest"
```

`copr` takes the project as `owner/project`, or `@group/project` for a group,
with or without a `copr:` prefix. The entry's name only identifies it, so a more
local config can replace an entry of the same name. Names may contain letters,
digits, `_`, `-`, and `.`.

mise enables a COPR the way `dnf copr enable` does: it downloads the repo file
COPR publishes for this release and writes it unchanged to
`/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:<owner>:<project>.repo`. A
group owner is written `group_<name>` there. It is the same file the copr plugin
writes, so:

- a COPR you already enabled with `dnf copr enable` counts as enabled, and mise
  leaves it alone
- `dnf copr list`, `dnf copr disable`, and `dnf copr remove` work on COPRs mise
  enabled
- the copr plugin does not need to be installed, and the same config works on
  [Fedora Atomic hosts](#fedora-atomic-hosts) that layer packages

mise downloads the file only when it is missing or the repository in it is
disabled, so a run where every COPR is already enabled does not use the network.
The file names COPR's signing key, and dnf imports it the first time it installs
a package from the repository. Without `--yes`, mise asks before it enables any
COPR, as it does before installing packages.

mise picks the COPR build target, the chroot, from `/etc/os-release`, the same
way the copr plugin does, reading the base release from `PLATFORM_ID` when it is
set. Fedora and distributions based on it, such as Bazzite, use
`fedora-<version>`, and Rawhide uses `fedora-rawhide`. RHEL, CentOS Stream, and
rebuilds such as Rocky Linux, AlmaLinux, Oracle Linux, and Alibaba Cloud Linux use
`epel-<major version>`.
Amazon Linux uses `amazonlinux-<version>`. If the project has no build for this
release, mise names the chroots it does build for. A COPR that cannot be enabled
is reported with the run's other failures, and the other repositories and
packages are still applied. On an ostree host that does not
[layer packages](#fedora-atomic-hosts), mise does not enable COPRs, since it
installs nothing there.

mise does not disable or remove repositories. Deleting an entry leaves the COPR
enabled; run `dnf copr remove <owner>/<project>` or delete its file yourself.

## Version pins

mise passes a pin to dnf as `name-version` or `name-version-release`. A
version-only pin, such as `"5.2.26"`, accepts any release of that version. The
Fedora pin above is only an example: release strings differ between
distributions and releases. Pick a version from the target's enabled
repositories; mise does not fetch an archived RPM.

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
- `upgrade` fails with a hint to run `rpm-ostree upgrade`: layered packages only
  update together with the base image, so mise cannot upgrade them one by one.

| Operation (`layer`) | Command                                                       |
| ------------------- | ------------------------------------------------------------- |
| Install             | `rpm-ostree install --idempotent --allow-inactive <packages>` |
| `apply --update`    | `rpm-ostree refresh-md`, then the install command             |
| Remove              | `rpm-ostree uninstall <packages>`                             |

## Remove packages

Outside layered ostree hosts, mise does not remove dnf packages. Deleting an entry leaves the package
installed; run `dnf remove` yourself.
