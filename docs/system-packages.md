# System Packages <Badge type="warning" text="experimental" />

mise can ensure machine-global system packages are installed via the
`[system.packages]` section of `mise.toml`:

```toml
[system.packages]
apt = ["libssl-dev", "build-essential"]
brew = ["postgresql@17", "ffmpeg"]
```

System packages are intentionally separate from [`[tools]`](/configuration.html):
they are not version-pinned per-project, do not get shims, and are managed by
the OS package manager. Use them for shared libraries and build dependencies
that dev tools need (`libssl-dev`, `postgresql`, `ffmpeg`), not for the dev
tools themselves — those belong in `[tools]`.

## Semantics

- **Declarative and additive** — entries merge across the
  [config hierarchy](/configuration.html) (global → project) as a union. A
  project can add packages on top of the global list but not remove them.
- **OS-filtered** — entries for a manager that isn't available on the current
  machine are silently skipped, so the same config works across platforms.
  `apt` entries are ignored on macOS, `brew` entries on Linux.
- **Manual installation only** — mise never installs system packages
  implicitly. `mise install` will print a one-time hint when packages are
  missing, but only `mise system install` ever installs anything.
- **Unknown managers are ignored with a warning** so configs using managers
  from newer mise versions still parse.

## Commands

```sh
mise system status            # table of requested vs installed packages
mise system status --json     # machine-readable
mise system status --missing  # exit 1 if anything is missing (CI check)

mise system install           # install whatever is missing (prompts first)
mise system install --dry-run # print the commands without running them
mise system install --yes     # skip the confirmation prompt
mise system install --manager apt
```

`mise doctor` also reports configured system packages and warns when any are
missing.

## apt (Linux)

Package state is checked with `dpkg-query`; missing packages are installed
with `apt-get install -y`. Entries pass through to apt verbatim, so apt's
native syntax works:

```toml
[system.packages]
apt = [
  "libssl-dev",
  "curl=8.5.0-2ubuntu10", # version pin
  "gcc:arm64",            # architecture qualifier
]
```

If `/var/lib/apt/lists` is empty (fresh containers), mise runs
`apt-get update` first automatically. Force a refresh with
`mise system install --update`.

### sudo

When not running as root, mise elevates with `sudo`, which prompts for your
password as usual. This is the only place mise ever elevates privileges, and
it only happens during an explicit `mise system install`:

- already root (containers, CI): no sudo, commands run directly
- interactive terminal: `sudo apt-get install ...` with a normal sudo prompt
- non-interactive without passwordless sudo: mise errors and prints the exact
  command to run manually — it never hangs waiting for a password
- the full command line is logged before it runs

Set [`system_packages.sudo = false`](/configuration/settings.html) to forbid
elevation entirely; mise will print the command for you to run yourself
instead.

## brew (macOS)

::: warning
Not yet implemented — coming in a future release.
:::

On macOS, mise will install [Homebrew](https://brew.sh) formulae _without
requiring brew to be installed_, by downloading bottles directly into
`/opt/homebrew`. If brew is already installed, mise delegates to it instead.
