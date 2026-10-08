---
description: "Install Homebrew formulae into the standard Homebrew prefix, with or without Homebrew installed."
---

# Homebrew formulae (brew)

The `brew` manager installs [Homebrew](https://brew.sh) formulae into the
standard Homebrew prefix. mise downloads and installs them itself, so Homebrew
does not need to be installed, and an existing Homebrew installation sees and
manages what mise installs. For apps and fonts, use
[`brew-cask`](/bootstrap/packages/brew-cask.html).

```toml
[bootstrap.packages]
"brew:postgresql@17" = "latest"
"brew:ffmpeg" = "latest"
"brew:imagemagick" = "latest"
```

```sh
mise bootstrap packages apply --manager brew --dry-run
mise bootstrap packages apply --manager brew
```

The dry run lists each formula it would install, dependencies first, and any
sudo command needed to create the prefix. mise installs each formula's runtime
dependencies with it.

## Supported platforms

| Platform               | Prefix                       |
| ---------------------- | ---------------------------- |
| macOS on Apple Silicon | `/opt/homebrew`              |
| Linux x86_64           | `/home/linuxbrew/.linuxbrew` |
| Linux arm64            | `/home/linuxbrew/.linuxbrew` |

The `brew` manager is unavailable on Intel Macs, where its entries are skipped.
[`brew-cask`](/bootstrap/packages/brew-cask.html#supported-platforms) does run
there. Linux arm64 bottles exist for most, but not all, of homebrew/core; mise
builds the rest [from source](#source-formulae).

## The prefix and PATH {#the-prefix}

If the prefix does not exist, mise creates it with Homebrew's standard layout,
using sudo to create the directory and give it to your user. It then installs
formulae as that user, so run mise as the user who should own the prefix.

Linked commands go into `<prefix>/bin`. These packages get no mise shims, and
mise does not change the current shell's `PATH`, so add the directory in your
shell startup file:

::: code-group

```sh [macOS]
export PATH="/opt/homebrew/bin:$PATH"
```

```sh [Linux]
export PATH="/home/linuxbrew/.linuxbrew/bin:$PATH"
```

:::

## Formula names

Use the formula name as Homebrew spells it, such as `brew:jq` or
`brew:openssl@3`. `brew:homebrew/core/jq` names the same formula. Formulae from
other taps use their fully-qualified name; see
[Third-party taps](#third-party-taps).

An alias such as `postgres`, or the old name of a renamed formula, installs the
canonical formula. Status cannot track the alias, so mise warns and prints the
canonical name to use instead.

## Version pins {#choose-a-formula-version}

Homebrew publishes bottles only for a formula's current version, so mise cannot
install an older one. To stay on a major version, declare a versioned formula
such as `"brew:postgresql@17" = "latest"`; the `@17` is part of the formula
name. If you put a version in the value, such as `"brew:jq" = "1.7"`, status
reports a `version mismatch` while another version is installed, and `apply`
and `upgrade` skip the entry with a warning.

## Upgrade {#upgrades}

`mise bootstrap packages upgrade --manager brew` installs the current bottle of
each configured formula whose installed version differs from Homebrew's API,
then repoints the links to it, as `brew upgrade` does.

## Third-party taps

Declare a formula from another tap with the fully-qualified name you would pass
to Homebrew:

```toml
[bootstrap.packages]
"brew:railwaycat/emacsmacport/emacs-mac" = "latest"
```

mise reads the tap's published API metadata when it has some. Otherwise it
reads the tap's Ruby definitions directly and builds those formulae
[from source](#source-formulae). Casks from a tap without API metadata are read
from the tap's Ruby definitions the same way and then installed as usual. A
definition that uses a Homebrew feature mise does not implement fails with an
error that names the feature.

mise looks for the tap at `https://github.com/<owner>/homebrew-<tap>.git`. For a
tap in a repository with another name, add its URL under
`[bootstrap.brew.taps]`. Only GitHub taps are supported.

```toml
[bootstrap.brew.taps]
"acme/tools" = "https://github.com/acme/tools.git"

[bootstrap.packages]
"brew:acme/tools/widget" = "latest"
"brew-cask:acme/tools/widget-app" = "latest"
```

[`mise bootstrap packages brew tap`](/cli/bootstrap/packages/brew/tap.html) and
[`untap`](/cli/bootstrap/packages/brew/untap.html) edit
`[bootstrap.brew.taps]` in your global config by default. Pass `--local` to
write the project's `mise.toml`, or `--path` for another file. Neither command
touches a Homebrew installation.

```sh
mise bootstrap packages brew tap railwaycat/emacsmacport
mise bootstrap packages brew tap acme/tools https://github.com/acme/tools.git --local
mise bootstrap packages brew untap acme/tools
```

## Keg-only formulae

Keg-only formulae, such as `openssl@3`, are not linked into `<prefix>/bin` or
`<prefix>/lib`. Point compilers and build flags at `<prefix>/opt/<formula>`
instead. As with Homebrew, a formula that is keg-only only because macOS
already provides it, such as `curl`, is linked normally on Linux.

### Locate an installed formula

[`mise bootstrap packages where`](/cli/bootstrap/packages/where.html) prints a
formula's `<prefix>/opt/<formula>` directory, which works for keg-only formulae
too. Append `/bin` to use its commands:

```sh
if package_root="$(mise bootstrap packages where brew:unzip)"; then
  export PATH="$package_root/bin:$PATH"
fi
```

The lookup works for formulae installed by mise or by Homebrew, with no
declaration needed. The `opt` path follows later upgrades. A library-only
formula has a root but may have no `bin` directory.

Use the canonical formula name. `brew:owner/tap/unzip` and
`brew:homebrew/core/unzip` both look up the installed `unzip`, without checking
which tap it came from, and aliases are not resolved. `where` does not look up
casks or packages from other managers.

If the formula is not installed or its `opt` link is broken, `where` prints
nothing on stdout, explains the problem on stderr, and exits nonzero. Install
it with `mise bootstrap packages apply brew:unzip`. `where` reads only the
prefix and ignores mise config files, so it works even when a `mise.toml` is
untrusted or invalid.

## Use alongside Homebrew {#coexistence-with-a-real-homebrew}

mise installs bottles into the Cellar the way Homebrew does and writes
Homebrew-compatible receipts, so `brew list`, `brew upgrade`, and
`brew uninstall` work on formulae mise installed. In the other direction, mise
reads the prefix directly, so formulae Homebrew installed count as installed.

mise never overwrites files in the prefix that it did not create. A link
conflict fails with a list of the conflicting files. If a configured formula's
`opt` link or linked-keg record is missing, status reports `needs repair`, and
`apply` restores the link without reinstalling the formula.

## Import and prune {#importing-and-pruning}

[`mise bootstrap packages import --manager brew`](/cli/bootstrap/packages/import.html)
records installed formulae in `[bootstrap.packages]`, much like
[`brew bundle dump`](https://docs.brew.sh/Brew-Bundle-and-Brewfile). By default
it imports formulae installed on request; `--all` adds their dependencies:

```toml
[bootstrap.packages]
"brew:ffmpeg" = "latest"
"brew:postgresql@17" = "latest"
```

Formulae from other taps are written with their fully-qualified names, and mise
adds a `[bootstrap.brew.taps]` entry for each tap. It uses the URL already in
your config, or else `https://github.com/<owner>/homebrew-<tap>.git`.

[`mise bootstrap packages prune --manager brew`](/cli/bootstrap/packages/prune.html)
is the explicit cleanup, similar to `brew bundle cleanup`. It removes linked
formulae that are neither declared nor needed by a declared formula or cask,
in the current configuration or any trusted config file mise tracks, including
formulae Homebrew installed. For each, it removes the keg, its `opt` link, and
its links in the prefix:

```sh
mise bootstrap packages prune --manager brew --dry-run
mise bootstrap packages prune --manager brew
```

Deleting a `brew:` entry does not uninstall the formula until you prune.

## Formulae without a bottle {#source-formulae}

mise builds a formula from source when no bottle exists for your platform. It
installs a Ruby through mise (or uses the one you configured) to evaluate the
formula, downloads the formula and its source archive pinned to the checksums
in Homebrew's API, installs the build dependencies as bottles, and then runs
the formula's install steps against the prefix. The result gets a
Homebrew-compatible receipt, as a bottle does.

Source builds need Xcode Command Line Tools on macOS, or gcc and make on Linux.
mise implements the commonly used parts of Homebrew's formula language, such as
configure, CMake, and Meson builds, resources, and patches. A formula that uses
something else, such as `virtualenv_install_with_resources` or a VCS download,
fails with a `formula uses ...` error instead of building it incorrectly.

## How mise installs a formula {#how-pouring-works}

For each formula, dependencies first, mise downloads the bottle from ghcr.io
and checks its sha256 against Homebrew's API, unpacks it into the Cellar,
rewrites Homebrew's placeholder paths, re-signs changed binaries on macOS,
writes a Homebrew-compatible receipt, and links the keg into the prefix.
Keg-only formulae get only the `opt` link. mise never runs `brew`.

## Troubleshooting

| Symptom                                   | What to do                                                                                                                |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Link conflict                             | Find out which program owns each listed file before you move or delete it. Running `apply` again does not overwrite them. |
| `formula uses ...` or unsupported feature | mise cannot build this formula. Install it with Homebrew, or remove the entry.                                            |
| Installed, but the command is not found   | Check that `<prefix>/bin` is on `PATH`, and whether the formula is [keg-only](#keg-only-formulae).                        |
| `needs repair` in status                  | Run `mise bootstrap packages apply` to restore the formula's links.                                                       |

mise does not implement `brew services`.
