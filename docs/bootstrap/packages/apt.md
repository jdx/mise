---
description: "Install Debian and Ubuntu packages with apt-get from mise.toml, including version pins."
---

# Debian and Ubuntu packages (apt)

The `apt` manager installs packages on Debian, Ubuntu, and other Debian-based
distributions with `apt-get`. It checks installed state with `dpkg-query` and
uses [sudo](/bootstrap/packages/#sudo) when mise is not running as root.

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:curl" = "8.5.0-2ubuntu10" # version pin
"apt:gcc:arm64" = "latest"     # architecture qualifier
```

```sh
mise bootstrap packages apply --manager apt --dry-run
mise bootstrap packages apply --manager apt
```

## Prerequisites

The manager is available on Linux when `apt-get` is on `PATH`. On other
machines, its entries show as
[`skipped`](/bootstrap/packages/#choose-platforms).

mise sets `DEBIAN_FRONTEND=noninteractive`, so debconf questions take their
defaults. A maintainer script that reads from the terminal directly can still
prompt.

## Package names

Use the package name as `apt-get install` takes it. Append `:<arch>` to select
an architecture, as in `gcc:arm64`. The machine's dpkg architectures and apt
sources must already provide that architecture; the declaration does not enable
multiarch. To install from another repository, declare it in
[`[bootstrap.apt.sources]`](#sources).

## Repositories and PPAs {#sources}

Declare extra repositories in `[bootstrap.apt.sources]`. mise adds them before
it installs or upgrades `apt:` packages, so a package can come from a vendor's
repository or a newer release can come from a PPA:

```toml
[bootstrap.apt.sources]
git-core = { ppa = "git-core/ppa" }
fish = { ppa = "fish-shell/release-4" }

[bootstrap.apt.sources.docker]
uris = "https://download.docker.com/linux/ubuntu"
components = "stable"
key = "https://download.docker.com/linux/ubuntu/gpg"
fingerprint = "9DC8 5822 9FC7 DD38 854A E2D8 8D81 803C 0EBF CD88"

[bootstrap.packages]
"apt:git" = "latest"
"apt:fish" = "latest"
"apt:docker-ce-cli" = "latest"
```

For each source, mise writes `/etc/apt/sources.list.d/mise-<name>.sources`
with the signing key embedded in its `Signed-By` field, so apt trusts the key
only for that repository and deleting the file removes both. Inline keys need
apt 2.4 or newer (Ubuntu 22.04, Debian 12). A key mise wrote to
`/etc/apt/keyrings/mise-<name>.gpg` in an earlier version is removed the next
time the source is applied. When it adds or
changes a source, mise runs `apt-get update` once before installing. Without
`--yes`, mise asks before it adds or changes any source, as it does before
installing packages. If apt
could not fetch a source's lists, mise warns and runs `apt-get update` again on
the next apply. Names may
contain letters, digits, `_`, `-`, and `.`.

A `ppa` entry takes the PPA as `owner/archive`, with or without the `ppa:`
prefix. mise reads the PPA's signing key fingerprint from Launchpad and
downloads that key from `keyserver.ubuntu.com`. Add `fingerprint` to pin the key
and skip the Launchpad lookup. Without one, mise downloads the key again
whenever the entry's source changes, such as when it points at another PPA. PPAs are built for Ubuntu releases, so mise adds
them only on Ubuntu and on distributions that set `UBUNTU_CODENAME` in
`/etc/os-release`, such as Linux Mint. On other releases it skips them, so the
same config also works on Debian.

Any other repository needs `uris`, `key`, and `fingerprint`:

| Field           | Value                                                                                                                   |
| --------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `uris`          | The repository URL, as the vendor's install instructions give it                                                        |
| `suites`        | Defaults to the release codename, such as `noble`; on Ubuntu derivatives such as Linux Mint, the Ubuntu base's codename |
| `components`    | Defaults to `main`, or to none when `suites` is a path ending in `/`                                                    |
| `architectures` | Optional, such as `amd64`, for a repository that publishes only some architectures                                      |
| `key`           | An http(s) URL of the signing key, ASCII-armored or binary                                                              |
| `fingerprint`   | The key's full fingerprint, or a list of them; spaces are ignored                                                       |

mise keeps only the keys whose fingerprints you declared from what it
downloads, and fails if one is missing. Once a matching key is in the source file, mise
leaves it alone and does not download it again. If the key changes or no
longer matches, the next apply replaces it.

A declared source also replaces a package that was already installed from
somewhere else. If an image ships Ubuntu's `git` and you declare the `git-core`
PPA, `mise bootstrap` installs the PPA's `git`, and status shows the old one as
`version mismatch` until it does. mise records in its state directory which
packages it has seen installed from a declared source, so each package moves
once per source. The record belongs to the user running mise, so running both
`mise` and `sudo mise` can move a package one more time. A later upload to the same source is left to
`mise bootstrap packages upgrade`, as for any `latest` package. A version pin or
an architecture-qualified name opts a package out. A dry run on a machine that
does not have the source yet cannot show the move, because apt has no package
lists for the source until it is added.

When a tool's [system dependencies](/configuration/settings.html#system_deps)
are installed automatically, mise adds declared sources only when it will not
ask first. A prompted install uses the sources `mise bootstrap` already added.

mise does not remove sources. Deleting an entry leaves its files in place;
delete `/etc/apt/sources.list.d/mise-<name>.sources` yourself.
To install a repository definition that mise should not generate, write it with
[`[bootstrap.files]`](/bootstrap/files.html#files-before-packages) instead.

## Version pins

mise passes a pin to apt as `name=version`. Versions are specific to a
distribution release, so the `8.5.0-2ubuntu10` pin above is only an example.
Run `apt-cache policy curl` on the target to see the candidates, and pin one
that its sources offer. mise cannot install a version your apt sources no
longer carry.

## Package lists {#metadata-refresh}

mise runs `apt-get update` before installing when `/var/lib/apt/lists` holds
no package lists (fresh containers), or when a simulated install
(`apt-get --simulate`, which needs no root) fails against the current lists,
for example because of a package or pinned version the lists do not contain. If
the simulation succeeds, mise installs without refreshing. To force a refresh
anyway:

```sh
mise bootstrap packages apply --update
```

## What mise runs

| Operation                       | Command                                                                  |
| ------------------------------- | ------------------------------------------------------------------------ |
| Check installed state (no sudo) | `dpkg-query -W <packages>`                                               |
| Install                         | `apt-get install -y -- <packages>`, after `apt-get update` when needed   |
| `apply --update`                | `apt-get update` first                                                   |
| Add or change a source          | `install -m 0644 /dev/stdin <file>`, then `apt-get update`               |
| Upgrade                         | `apt-get update`, then `apt-get install -y --only-upgrade -- <packages>` |

`upgrade` acts only on configured packages that are installed. apt still
installs any new dependencies the upgrades need.

## Remove packages

mise does not remove apt packages. Deleting an entry leaves the package
installed; run `apt-get remove` yourself.
