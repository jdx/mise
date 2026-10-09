---
description: "Lock resolved tool versions and artifact checksums for reproducible installations."
---

# Lockfile (mise.lock)

`mise.lock` records the exact versions that the requests in `mise.toml` resolved
to, with download URLs and checksums where the backend provides them. Commit it
next to `mise.toml` so every machine and CI job installs the same tools.

## Quick start {#overview}

In a project with tools in `mise.toml`:

```sh
mise lock             # resolve the configured tools and write mise.lock
mise install --locked # install exactly what mise.lock records
```

`mise lock` writes the lockfile without installing anything. Commit `mise.toml`,
`mise.lock`, and the `.mise/locks/` directory if mise created one for
[dependency graphs](#dependency-graphs). After pulling, teammates and CI run
`mise install --locked`.

To move a tool to the newest version its request allows:

```sh
mise lock --bump node
mise install --locked
```

Review the lockfile diff and run the project's checks before committing.
`mise.lock` covers tools, not your application's dependencies, so keep
`package-lock.json`, `uv.lock` and similar files as well.

A locked install is not an offline install. Private downloads, artifacts that
are not cached yet, and verification checks can still need the network and
[a token](/dev-tools/github-tokens.html). The file format is described in the
[mise.lock reference](/dev-tools/mise-lock-reference.html).

## Creating lockfiles {#enabling-lockfiles}

Run [`mise lock`](/cli/lock.html) to create a project lockfile. To have mise
create and update one whenever it installs or upgrades tools, set
[`lockfile`](/configuration/settings.html#lockfile) in the project's
`mise.toml`:

```toml [mise.toml]
[settings]
lockfile = true
```

Set it in your global config instead (`mise settings lockfile=true`) to
make that your default for every project.

The setting has three states:

- `true` in a config file: mise creates lockfiles and keeps them up to date.
- Unset: mise keeps existing lockfiles up to date but does not create new ones.
  `MISE_LOCKFILE=1` behaves the same way; only `lockfile = true` in a config
  file turns on automatic creation.
- `false`: lockfiles are off for that project. Combining it with
  `locked = true` is an error.

`mise lock` locks the tools of the current config root, including tools that
tasks declare in `tools`, in inherited task templates and in included task
files. It reads those definitions without running tasks, hooks or installers,
so task tools can be locked before a task first runs. Global tools are locked
only by `mise lock --global`; see [global lockfiles](#global-lockfiles).

Tools listed only in `.tool-versions` are not locked. Move them to `mise.toml`
first; [Migrating from asdf](/dev-tools/comparison-to-asdf.html) shows how to
convert the file.

The [`lockfile_mode`](/configuration/settings.html#lockfile_mode) setting
chooses how automatic updates rewrite the file; see
[complete lockfile generation](/dev-tools/mise-lock-reference.html#complete-lockfile-generation).

## Which lockfile a tool uses

Each config file writes to the lockfile beside it:

| Tool declared in                                                      | Lockfile                                                            | Commit it                                    |
| --------------------------------------------------------------------- | ------------------------------------------------------------------- | -------------------------------------------- |
| `mise.toml`                                                           | `mise.lock`                                                         | Yes                                          |
| `mise.<env>.toml`, such as `mise.test.toml`                           | `mise.<env>.lock`                                                   | Yes                                          |
| `mise.local.toml`                                                     | `mise.local.lock`                                                   | No; ignore it along with `mise.local.toml`   |
| `mise.<env>.local.toml`                                               | `mise.<env>.local.lock`                                             | No                                           |
| `~/.config/mise/config.toml` or `/etc/mise/config.toml`               | `mise.lock` in the same directory, created by `mise lock --global`  | Only if you keep that config in a repository |
| A monorepo subproject's `mise.toml` with `[monorepo] lockfile = true` | The lockfile of the same name at the monorepo root                  | Yes                                          |
| An idiomatic version file such as `.nvmrc`                            | The lockfile of the nearest `mise.toml` whose directory contains it | Yes                                          |
| `.tool-versions`                                                      | None                                                                |                                              |

The rule follows the config file's location: `.config/mise.toml` uses
`.config/mise.lock`, and the files in a `conf.d/` directory share one lockfile
in its parent directory.

### Environments {#environment-specific-lockfiles}

With `MISE_ENV=test`, `mise lock` writes `mise.lock` for the tools in
`mise.toml` and `mise.test.lock` for the tools in `mise.test.toml`. An
environment lockfile holds only the tools that its own config declares, so CI
jobs that do not set `MISE_ENV` depend only on `mise.lock`. Bumping a tool in
`mise.dev.lock` does not invalidate their caches. See
[config environments](/configuration/environments.html).

### Local lockfiles

Tools in `mise.local.toml` go to `mise.local.lock`:

```sh
mise use --path mise.local.toml node@24   # locked in mise.local.lock
mise use --path mise.toml node@22         # locked in mise.lock
```

`mise lock --local` refreshes `mise.local.lock` instead of `mise.lock`:

```sh
mise lock --local              # update mise.local.lock
mise lock --local node python  # update only these tools in mise.local.lock
```

If you ignore `mise.local.lock` in Git, also ignore its sidecar directory,
`.mise/locks/mise.local/`.

### Global lockfiles

A plain `mise lock` only locks the project's config root. `mise lock --global`
locks the tools in your global config and the system config:

```sh
mise lock --global   # writes ~/.config/mise/mise.lock (and /etc/mise/mise.lock)
```

If `~/.config/mise/config.toml` is a symlink into a dotfiles repository, such as
`~/dotfiles/mise.toml`, `mise lock` run inside the repository reports that there
is nothing to lock, because mise treats the file as global config. Run
`mise lock --global` from inside the repository to write `~/dotfiles/mise.lock`.
From other directories mise reads and writes `~/.config/mise/mise.lock`, so also
symlink that path to `~/dotfiles/mise.lock`. Sidecars follow the symlink target.

### Monorepos

In a [monorepo](/tasks/monorepo.html) (`monorepo_root = true`), set
`[monorepo] lockfile = true` to keep every subproject's tools in lockfiles at
the root: tools from `packages/api/mise.toml` go to the root `mise.lock`, and
environment and local variants go to root files such as `mise.ci.lock` and
`mise.local.lock`.

```toml [mise.toml]
monorepo_root = true

[monorepo]
config_roots = ["packages/api", "packages/web"]
lockfile = true
```

The next lock-aware command moves existing subproject lockfiles into the root
lockfile. Root entries win on conflicts, entries found only in a subproject are
kept, and the migrated subproject lockfiles and their sidecars are removed.

While the setting is unset, mise keeps a lockfile next to each subproject's
config. From mise 2026.12.0, monorepos that leave it unset and have `mise*.lock`
files get a warning, and from 2027.6.0 the unset default is root lockfiles.
Older mise versions do not understand root lockfiles for subproject tools, so a
team that needs to support them can keep the old layout:

```toml [mise.toml]
[monorepo]
lockfile = false
```

## Updating locked versions {#workflow}

| Goal                                                                 | Command                  |
| -------------------------------------------------------------------- | ------------------------ |
| Change the request in `mise.toml` and install it                     | `mise use node@26`       |
| Install the newest version the request allows and update `mise.lock` | `mise upgrade node`      |
| Move `mise.lock` to the newest allowed version without installing    | `mise lock --bump node`  |
| Lock an exact version without changing a prefix request              | `mise lock node@24.11.0` |
| Install the versions `mise.lock` records                             | `mise install --locked`  |

### Bumping locked versions

`mise lock --bump` re-resolves version requests such as `latest`, `lts` or a
prefix like `"24"` and writes the newest matching versions to the lockfile. It
does not install anything or change `mise.toml`. Exact pins resolve to
themselves and stay as they are; use [`mise upgrade --bump`](/cli/upgrade.html)
to rewrite pins in `mise.toml`.

```sh
# mise.toml has node = "24", locked at 24.10.0, and 24.11.0 is out
mise lock --bump             # mise.lock now has 24.11.0; mise.toml still says "24"
mise lock --bump node        # bump only node
mise lock --bump --dry-run   # show what would change without writing
```

For automated updates, run it on a schedule and open a pull request when the
lockfile changes. `--json` prints one object per tool whose locked version
changed, with `name`, `backend`, `lockfile`, `old_versions` and
`new_versions`, and suppresses the other output. Checksum and URL refreshes for
unchanged versions are not reported, and a tool removed from the config has an
empty `new_versions`. See [`mise lock`](/cli/lock.html) for details.

```sh
mise lock --bump --dry-run --json
```

```json
[
  {
    "name": "node",
    "backend": "core:node",
    "lockfile": "~/src/myproj/mise.lock",
    "old_versions": ["24.10.0"],
    "new_versions": ["24.11.0"]
  }
]
```

If the job runs on branches you do not control, such as a bot that bumps
`mise.lock` on pull requests, set `MISE_SAFE=1` so the project's config cannot
run code. [Safe mode](/security.html#safe-mode) refuses tasks, template
`exec()` and plugin installs, and skips hooks and `_.source` scripts, while
version resolution over HTTP keeps working:

```sh
MISE_SAFE=1 mise lock --bump --json
```

### Pinning a locked version

To lock a specific version while `mise.toml` keeps a prefix or `latest`:

```sh
# mise.toml has node = "latest" or node = "24"
mise upgrade node@24.11.0   # installs 24.11.0 and updates mise.lock
mise lock node@24.11.0      # updates mise.lock without installing
```

If the version falls outside the configured request, mise also rewrites the
request at the same precision. With `node = "22"`, `mise upgrade node@24.11.0`
changes `mise.toml` to `node = "24"` and locks `24.11.0`.

### Command behavior {#command-behavior-with-lockfiles}

These commands update an existing lockfile. Whether they create a new one
depends on the [`lockfile` setting](#enabling-lockfiles).

| Command                     | Installs | Updates `mise.toml`                        | Updates `mise.lock`                       |
| --------------------------- | -------- | ------------------------------------------ | ----------------------------------------- |
| `mise use node@24`          | Yes      | Yes (sets `node = "24"`)                   | Yes                                       |
| `mise install`              | Yes      | No                                         | Yes                                       |
| `mise install node`         | Yes      | No                                         | Yes (node's configured version)           |
| `mise install node@24.11.0` | Yes      | No                                         | No (a one-off install outside the config) |
| `mise upgrade`              | Yes      | No                                         | Yes                                       |
| `mise upgrade node`         | Yes      | No                                         | Yes (newest version within the request)   |
| `mise upgrade node@24.11.0` | Yes      | Only if the version is outside the request | Yes                                       |
| `mise upgrade --bump`       | Yes      | Yes (raises the request to match)          | Yes                                       |
| `mise lock`                 | No       | No                                         | Yes (all configured tools)                |
| `mise lock --bump`          | No       | No                                         | Yes (newest versions within the requests) |
| `mise lock node@24.11.0`    | No       | Only if the version is outside the request | Yes                                       |

### Switching to a new registry backend {#registry-backend-changes}

The registry sometimes moves a tool to a different backend, for example from
`github:jdx/communique` to `packslip:github.com/jdx/communique`. A tool
configured by its short name keeps the backend recorded in `mise.lock`, even
when `mise lock --bump` or `mise upgrade` picks a newer version, so a registry
update never changes where a locked tool installs from. `mise install` and
`mise lock` warn when this happens:

```text
mise WARN  communique is locked to github:jdx/communique, but the registry now installs it from packslip:github.com/jdx/communique. Run `mise backends switch communique` to switch.
```

[`mise backends switch`](/cli/backends/switch.html) moves the lock entries to the
registry's backend at the same versions, records the new backend's checksums and
URLs, and reinstalls installed versions from it:

```sh
mise backends switch communique   # switch one tool
mise backends switch --dry-run    # list every locked tool with a newer backend
```

## Strict lockfile mode {#strict-lockfile-mode}

`mise install --locked` fails instead of resolving a version when the lockfile
has no matching entry, which catches an incomplete lockfile in CI:

```sh
mise install --locked
MISE_LOCKED=1 mise install   # the same, set through the environment
```

For backends that record download URLs, the entry also needs a URL for the
current platform. Lockfiles that record only versions still work for most
backends that record no URL; see [backend support](#backend-support). npm and
PyPI tools in a format version 2 or newer lockfile also need their
[dependency graph](#dependency-graphs).

To make locked mode your personal default, set
[`locked = true`](/configuration/settings.html#locked) in your global config.
Locked mode reads the lockfile, so it fails when `lockfile = false`.

### Locking selected scopes

By default, `--locked`, `MISE_LOCKED=1` and `locked = true` apply to tools from
project, user-global and system config. Use
[`locked_scopes`](/configuration/settings.html#locked_scopes) to exclude config
scopes that intentionally contain rolling or distribution-managed tools:

```toml [~/.config/mise/config.toml]
[settings]
locked = true
locked_scopes = ["project"]
```

Valid scopes are `project`, `global` and `system`. Tools given on the command
line or in environment variables stay locked, because they do not belong to a
config scope. Excluding a scope relaxes locked mode for that scope; mise still
uses an existing lockfile there. If global tools should be locked and are
missing from the lockfile, run `mise lock --global`. `locked_scopes` is
global-only, so a project cannot weaken your locked-mode policy.

### Locking one configuration root

To require lock entries only for the tools of one config root, set
`tool_config.locked` instead:

```toml [mise.toml]
[tool_config]
locked = true

[tools]
node = "24"
```

The policy covers the tools declared by every config in that root, such as
`mise.toml` and `mise.local.toml`, each checked against its own lockfile. Tools
inherited from the global config or a parent config root keep their own
policy. The policy applies even when `locked_scopes` excludes its scope.

### Preparing platform entries

For backends that record URLs, add entries for every platform that installs the
tools:

```sh
mise lock                                    # refresh the platforms already in the file
mise lock --platform linux-x64,macos-arm64   # add or refresh specific platforms
```

Without `--platform`, `mise lock` uses
[`lockfile_platforms`](/configuration/settings.html#lockfile_platforms) plus the
current platform when that setting is set. Otherwise it refreshes the platforms
already in the lockfile, or writes the common platforms for a new lockfile.
[Locked tool stubs](/dev-tools/tool-stubs.html#locked-tool-stub) follow the
same rules, using their project's `mise.lock`.

## Lockfiles in CI {#ci-cd}

Commit entries for every runner platform and install with
`mise install --locked` (`jdx/mise-action` adds `--locked` itself when the
repository has a `mise.lock`); see
[Continuous integration](/continuous-integration.html).

## Backend support

What a lock entry records depends on the backend. Inspect the generated entry
for the tool and platform you care about.

| Backend                                                                                            | What `mise.lock` records                                                                                  |
| -------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| aqua, github, gitlab, forgejo, http, s3                                                            | A URL per platform and a checksum where the release provides one or mise can compute it                   |
| packslip                                                                                           | URL and checksum per platform, and the signer the project committed to; policy is checked at installation |
| conda                                                                                              | URL and checksum per platform, and the package's dependencies                                             |
| Core tools that download archives: Bun, Deno, Elixir, Erlang, Go, Java, Node.js, Python, Ruby, Zig | A URL per platform and a checksum where one is available                                                  |
| core:swift                                                                                         | URL and checksum per platform and Linux distro; locked mode does not require the URL                      |
| npm (embedded aube) and pypi (uv)                                                                  | The version, plus a [dependency graph](#dependency-graphs) in format version 2 and later                  |
| cargo, dotnet, gem, go, spinel, core:dotnet, core:rust                                             | The top-level version only; the language's installer resolves dependencies and build inputs               |
| vfox tool plugins                                                                                  | URLs that the plugin's hooks return, which locked mode requires                                           |
| asdf and vfox backend plugins                                                                      | The version only; the plugin performs the installation                                                    |
| spm                                                                                                | The version only                                                                                          |

Locked mode does not require a URL for `asdf`, `cargo`, `dotnet`, `gem`, `go`,
`npm`, `pypi` (`pipx`), `spinel`, `spm`, `ubi`, `core:dotnet`, `core:rust`,
`core:swift` and vfox backend plugins, because those backends do not record
one. The exemption covers artifact URLs only; npm and PyPI dependency graphs
have their own checks.

A `provenance` field is not proof that mise verified the bytes; see
[provenance and verification](#provenance-and-security).

## Dependency graphs

Format version 2 and later lock the transitive dependencies of npm and PyPI
tools, not only their top-level version. mise stores each graph in the package
manager's own format, `aube-lock.yaml` for npm tools installed with embedded aube
and `uv.lock` for PyPI tools when uv 0.12.10 or newer is installed, in a sidecar
directory (`.mise/locks/` by default). `mise.lock` records the directory and a
digest of the graph. Commit the sidecar directory with `mise.lock`;
`mise lock --sidecars` lists it.

`mise lock --bump npm:prettier` refreshes a tool's dependency graph even when its
top-level version does not change, and `mise install --locked` replays the
recorded graph without resolving dependencies again. Projects that lock
different graphs for the same package version get separate installations.
Lifecycle scripts can still produce different output on each machine. See
[native dependency sidecars](/dev-tools/mise-lock-reference.html#native-dependency-sidecars)
for the layout and for editing sidecars, and the [npm](/dev-tools/backends/npm.html)
and [PyPI](/dev-tools/backends/pypi.html) backend pages for installer limits.

## Provenance and verification {#provenance-and-security}

For supported backends, `mise lock` records provenance such as SLSA, Cosign,
Minisign or GitHub artifact attestations. mise verifies new provenance against
each target platform's artifact before it records it.

When an entry has both a checksum and provenance, `mise install` can skip
repeating the provenance check. That makes the lockfile a trust input: review
changes to it, and take it only from a trusted project source. Checksums are
still verified on every download.

To repeat provenance checks on every install, set
[`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance),
which [paranoid mode](/paranoid.html) also turns on:

```sh
MISE_LOCKED_VERIFY_PROVENANCE=1 mise install
```

This runs the supported checks again for the artifacts being installed. It does
not create provenance for releases that never published it, an installed tool
may not be downloaded again, and it is separate from packslip signer and
signed-list policy.

## Minimum release age

A version recorded in `mise.lock` installs even when it is newer than
[`minimum_release_age`](/security.html#minimum-release-age); the cutoff applies
only when mise selects a new version. npm and PyPI tools also apply it when they
resolve transitive dependencies that the lockfile does not record.

## Troubleshooting

### Checksum mismatch {#regenerating-checksums}

A checksum mismatch means the downloaded bytes differ from the recorded
artifact. Check the tool, platform, URL and backend options in the error and in
the lockfile. A vendor may have replaced an asset, a mirror may serve different
content, or the entry may describe another build.

After confirming that the upstream change is intentional, refresh only that
tool's entry and review the diff:

```sh
mise lock node
git diff -- mise.lock
```

Do not delete checksums or uninstall every tool to get past the error. If the
new artifact is unexpected, keep the existing lockfile and investigate the
release before accepting the new bytes.

### `mise lock` finds no tools

`No tools configured to lock` means the current config root declares no tools
that a lockfile can hold. When the global config declares tools, the message
says so; lock those with `mise lock --global`. Tools in `.tool-versions` are not
locked. See [which lockfile a tool uses](#which-lockfile-a-tool-uses).

### Lockfile conflicts

When branches change the same tools:

1. Resolve `mise.toml` first so it has the requests you want.
2. In `mise.lock`, keep either side of each conflicting entry, then run
   `mise lock` to rewrite the entries for the merged requests. Commit any
   sidecar changes it makes.
3. Run `mise install --locked` and the project's checks, then commit.
