---
description: "Lock resolved tool versions and artifact checksums for reproducible installations."
socialDescription: "Lock resolved tool versions and artifact checksums for reproducible installations."
---

# mise.lock Lockfile

`mise.toml` records the versions a project accepts; `mise.lock` records the
concrete versions those requests resolved to. Supported backends also record
artifact URLs, checksums, and verification metadata. Commit both files so other
machines can use the same resolutions.

## Quick start {#overview}

For a project with tools configured in `mise.toml`:

```sh
mise lock             # resolve configured tools without installing them
mise install --locked # install using the recorded resolutions
```

Commit `mise.toml`, `mise.lock`, and any
[native dependency sidecars](#native-dependency-sidecars). After pulling the
project, teammates and CI can run `mise install --locked`.

To update a tool within its configured version range:

```sh
mise lock --bump node
mise install --locked
```

Review the lockfile diff and run the project's checks before committing.
Keep application lockfiles such as `package-lock.json` and `uv.lock` too:
`mise.lock` manages development tools, not the application's dependencies.

The metadata recorded varies by [backend](#backend-support).
Locked installation is not offline installation: private downloads, uncached
artifacts, and verification checks can still need network access and
[authentication](/dev-tools/github-tokens.html).

## Enabling Lockfiles

Run `mise lock` to create a project lockfile explicitly. To create and maintain
one automatically as tools are installed or upgraded, configure:

```toml [mise.toml]
[settings]
lockfile = true
```

For a personal default across projects, use:

```sh
mise settings set lockfile=true
```

When the setting is unset, mise updates existing lockfiles but does not create
new ones automatically. `MISE_LOCKFILE=1` retains that existing-file behavior for
compatibility; it is not equivalent to explicitly configuring `lockfile = true`
in TOML. Global lockfiles are created only with `mise lock --global`.

## Updating tools {#workflow}

### Initial setup

Run `mise lock`, then `mise install --locked`, as shown in the
[quick start](#overview).

### Daily usage

Run `mise install` to install the recorded versions, or `mise install --locked`
to require complete entries for supported backends. Use `mise upgrade` to
install newer versions within the configured ranges and update the lockfile.

### Updating versions

To change a request in `mise.toml` and install it:

```sh
mise use node@26
```

### Bumping Locked Versions

`mise lock --bump` re-resolves fuzzy version selectors (like `latest`, `lts`, or
prefixes like `"22"`) against the latest matching versions and updates the
lockfile — without installing anything and without modifying `mise.toml`.
Exactly pinned versions are left unchanged (use [`mise upgrade --bump`](/cli/upgrade.html)
to rewrite pins in `mise.toml`).

```sh
# mise.toml has node = "22" locked at 22.14.0; 22.15.0 was released since
mise lock --bump             # lockfile now pins 22.15.0, mise.toml still says "22"
mise lock --bump node        # only bump node
mise lock --bump --dry-run   # show what would change without writing
```

This is designed for automated dependency updates: run it on a schedule in CI
and open a PR when the lockfile changes. `--json` prints the changes as
machine-readable output (and suppresses the human-readable messages). Only
version-level changes are reported — checksum/URL refreshes for unchanged
versions produce no entries — and version lists keep config/lockfile order
rather than being sorted. Tools removed from config are reported with an
empty `new_versions`:

```sh
mise lock --bump --dry-run --json
```

```json
[
  {
    "name": "node",
    "backend": "core:node",
    "lockfile": "~/src/myproj/mise.lock",
    "old_versions": ["22.14.0"],
    "new_versions": ["22.15.0"]
  }
]
```

::: tip Run bump automation in safe mode
When the job runs against configuration you don't control — most commonly a bot bumping
`mise.lock` on pull request branches — set [`MISE_SAFE=1`](/security.html#safe-mode) so the
project's config cannot execute code. Safe mode refuses template `exec()`, `_.source` scripts,
hooks, tasks, asdf plugin scripts, and plugin installs, while `--bump` version resolution over
HTTP-based backends keeps working:

```sh
MISE_SAFE=1 mise lock --bump --json
```

:::

### Pinning a Locked Version

You can pin a specific version in the lockfile while keeping a fuzzy specifier in `mise.toml`:

```sh
# mise.toml has node = "latest" or node = "22"
mise upgrade node@22.15.0   # installs 22.15.0 and updates mise.lock
mise lock node@22.15.0      # updates mise.lock without reinstalling
```

If the version doesn't match the current config prefix, the config is updated automatically. For example, if `mise.toml` has `node = "20"` and you run `mise upgrade node@22.15.0`, the config is bumped to `node = "22"` (preserving the same precision level) and the lockfile is set to `22.15.0`.

## Command Behavior with Lockfiles

These commands update an existing lockfile. Automatic creation follows the
[`lockfile` setting](#enabling-lockfiles); `mise lock` creates one explicitly.

| Command                     | Installs | Updates `mise.toml`                  | Updates `mise.lock`                     |
| --------------------------- | -------- | ------------------------------------ | --------------------------------------- |
| `mise use node@22`          | Yes      | Yes (sets `node = "22"`)             | Yes                                     |
| `mise install`              | Yes      | No                                   | Yes                                     |
| `mise install node`         | Yes      | No                                   | Yes (installs config version for node)  |
| `mise install node@22.15.0` | Yes      | No                                   | No (one-off install, not config-driven) |
| `mise upgrade`              | Yes      | No                                   | Yes                                     |
| `mise upgrade node`         | Yes      | No                                   | Yes (upgrades node within its range)    |
| `mise upgrade node@22.15.0` | Yes      | Only if version doesn't match prefix | Yes                                     |
| `mise upgrade --bump`       | Yes      | Yes (bumps prefix to match)          | Yes                                     |
| `mise lock`                 | No       | No                                   | Yes (regenerates for all tools)         |
| `mise lock --bump`          | No       | No                                   | Yes (re-resolves selectors to latest)   |
| `mise lock node@22.15.0`    | No       | Only if version doesn't match prefix | Yes                                     |

## Strict Lockfile Mode

Use `mise install --locked` in CI to catch missing lock entries instead of
silently resolving them. For backends with URL-based locking, the entry must
include a download URL for the current platform. Dependency graphs must also
be present and match their recorded digest.

Locked mode does not make installation offline, and its checks depend on the
backend. See [backend support](#backend-support).

```sh
# Enable strict mode
mise settings set locked=true

# Or via environment variable
MISE_LOCKED=1 mise install
```

### Locking selected scopes

By default, invocation-wide locked mode applies to project, user-global, and
system config. Use `locked_scopes` to exclude config scopes that intentionally
contain rolling or distribution-managed tools:

```toml
# In ~/.config/mise/config.toml or /etc/mise/config.toml
[settings]
locked = true
locked_scopes = ["project"]
```

Valid scopes are `project`, `global`, and `system`. Explicit tool arguments and
environment-supplied tool versions remain locked because they do not belong to
a config scope. Excluding a scope relaxes locked mode for that scope; mise still
uses an existing lockfile when one is present. If global tools should be locked
and are missing from the lockfile, run `mise lock -g` to generate the global
lockfiles. `locked_scopes` is global-only so project configuration cannot weaken
a user's locked-mode policy.

### Locking one configuration root

To enforce strict mode only for tools declared by one config root, use
`tool_config.locked` instead of the invocation-wide setting:

```toml
[tool_config]
locked = true

[tools]
node = "24"
```

This policy belongs to the containing config root: tools declared by `mise.toml`,
`mise.local.toml`, and other configs sharing that root must be present in their
respective lockfiles. Tools inherited from global or parent config roots keep
their own policy. A config-root policy remains enforced even when its scope is
excluded from `locked_scopes`.

### Preparing platform entries

For URL-lockable backends, populate entries for the platforms that will install
the tools:

```sh
mise lock                    # refresh existing platforms, or the default set for a new file
mise lock --platform linux-x64,macos-arm64  # or specific platforms
```

URL checks skip backends that cannot record a download URL: `asdf`, `cargo`,
`gem`, `go`, `npm`, `pypi`/`pipx`, `ubi`, `core:dotnet`, `core:rust`,
`core:swift`, and vfox backend plugins. This exemption is specific to artifact
URLs; npm and PyPI dependency graphs have their own locked-install checks.
vfox tool plugins can record URLs and participate in URL locking.
Tools resolved from a [tool stub](/dev-tools/tool-stubs) also skip URL checks.

## Dependency graphs

Version 2 lockfiles can lock a CLI's transitive dependencies as well as its
top-level version. The native package-manager files live in
[sidecar directories](#native-dependency-sidecars), referenced by path and digest
from `mise.lock`.

### npm tools

mise's embedded aube installer records and replays npm dependency graphs.
Other npm installers have different limitations; see the
[npm backend](./backends/npm.md).

For an npm tool, `mise lock --bump <tool>` refreshes the transitive graph even when
the top-level package version does not change. Frozen installs validate and replay
that graph. Its file-byte digest is part of the installation directory name, so two
projects can use different transitive graphs for the same top-level version. This
selection guarantee does not make lifecycle-script output reproducible.

### Python dependency graphs

With uv >= 0.12.10 installed, `mise lock` records dependencies and wheel hashes;
`mise install --locked` replays them without resolution or source builds. Use
`mise lock --bump pypi:black` to refresh Black's dependencies independently of its
top-level version. See [PyPI tools](./backends/pypi.md#dependency-locking) for
interpreter selection, supported indexes, and compatibility limitations.

## Native dependency sidecars

Commit the sidecar directory alongside `mise.lock`. Each graph has its own directory:

```text
mise.lock
.mise/locks/pypi-black/24.10.0/pyproject.toml
.mise/locks/pypi-black/24.10.0/uv.lock
.mise/locks/npm-prettier/3.3.3/package.json
.mise/locks/npm-prettier/3.3.3/aube-lock.yaml
```

The corresponding entry contains a relative path and a SHA-256 digest of the native
lockfile's exact bytes:

```toml
[[tools."pypi:black"]]
version = "24.10.0"
backend = "pypi:black"
uv = { path = ".mise/locks/pypi-black/24.10.0", digest = "sha256:…" }
```

### Sidecar locations

The directory follows your configuration layout: `.mise/mise.lock` uses
`.mise/locks/`, and both `.config/mise/mise.lock` and `.config/mise.lock` use
`.config/mise/locks/`. Option variants have a hash suffix. Once recorded, a path
stays unchanged when other variants are added. Directory names follow the tool
spelling: `pypi:black` uses `pypi-black`, while `pipx:black` uses `pipx-black`.
Explicit `mise lock` and generate-mode auto-lock saves remove unreferenced sidecar
directories. Merge-mode auto-lock saves write sidecars but never delete them.
In a monorepo, sidecars follow the root lockfile layout; a subproject's configuration
layout does not affect their location. Successful migration removes legacy sidecars.

Non-default lockfile names have separate subdirectories. For example,
`mise.local.lock` uses `.mise/locks/mise.local/` in a root configuration layout.
If you ignore the local lockfile in Git, also ignore that matching sidecar directory.
Cleanup never removes another lockfile's sidecars.

### Inspecting and editing sidecars

Tools that recognize `pyproject.toml` and `uv.lock`, such as Renovate, can inspect
these native Python projects. Configure their repository scope to include the
sidecar directories. `aube-lock.yaml` is aube's native format; it is not an npm
`package-lock.json` and scanners may not recognize its transitive dependencies.

After editing a sidecar, run `mise lock` to validate it and update the recorded
digest. Then run `mise install --locked`. Even formatting-only edits change the
digest and installation identity.

Ordinary `mise install` also accepts valid edits when an installation is needed
and updates the digest through auto-locking. If the recorded installation already
exists, it skips graph validation and only warns if the graph file is missing.
`mise install --locked` rejects an inconsistent digest.

Run `mise lock` to regenerate a missing or unreadable sidecar; the tool must be
configured and its installer available. Locked installation fails when a required
sidecar is missing.

Ordinary environment resolution uses the recorded digest without opening
sidecars. Python graphs retain all wheel targets for portability; separate files
keep this detail out of the top-level version-pin diff.

## Complete lockfile generation

The default `merge` mode updates entries as tools are installed. To try the
new generator, which rebuilds the complete lockfile from current requests and
reuses unchanged artifacts:

```toml [mise.toml]
[settings]
lockfile_mode = "generate"
```

Run `mise lock` first, or also set `lockfile = true` to create lockfiles
automatically. `lockfile_mode` alone does not enable automatic creation.

During installation, a `lock` progress bar tracks background metadata downloads,
hashing, and verification. The first run can download artifacts for other
platforms; subsequent runs reuse unchanged entries. This helps reduce
cross-platform lockfile churn.

Use `MISE_LOCKFILE_MODE=generate` to try generation for one command. Set
`lockfile_mode = "merge"` to switch back; no format migration is needed.
The default remains `merge` pending a maintainer review of trial feedback
before mise 2026.12.0.

## How It Works

mise matches each configured request against its lock entry, including the
backend and tool options. Supported backends also verify downloaded artifacts
against recorded checksums.

`mise lock` includes tools declared in tasks, inherited templates, and included
task files. It reads those definitions without running tasks, hooks, or tool
installers, so task tools can be locked before their first execution. Entries
belong to the lockfile for the config that owns the task.

### Configuration precedence {#runtime-resolution}

Runtime command-line requests that read lockfiles use the lockfile belonging to
the effective tool configuration.
If a project defines `hk`, it overrides the global `hk` definition, including its
lockfile pins. A missing matching project entry falls back to normal unlocked
resolution, not an overridden pin. If the project does not define `hk`, the
inherited definition and its lockfile remain available.

In locked mode, a missing matching entry is an error even for read-only lookups
such as `mise which hk --tool hk@latest` and even when the tool is already installed.
Commands that intentionally bypass lockfiles, such as `mise exec hk@latest`,
retain that behavior.

Reading a configuration's pin does not make a command-line override part of that
configuration. For example, `mise exec node@24` does not replace the pin for a
project configured with `node = "22"`. Use `mise use` or `mise upgrade` to make an
intentional update. Environment-variable version overrides retain their existing
lockfile lookup behavior.

`mise which --tool` warns when the effective source has no matching pin but an
overridden configuration's lockfile does. This includes global, parent-project,
and environment-specific definitions. An unrelated lockfile containing a match
is not enough: that lower-precedence configuration must actually define the tool.
A matching effective pin produces no override warning.

## Environment-Specific Lockfiles

When using [environment-specific configuration files](/configuration/environments) (e.g., `mise.test.toml`), each environment gets its own lockfile:

| Config file            | Lockfile               |
| ---------------------- | ---------------------- |
| `mise.toml`            | `mise.lock`            |
| `mise.test.toml`       | `mise.test.lock`       |
| `mise.staging.toml`    | `mise.staging.lock`    |
| `mise.local.toml`      | `mise.local.lock`      |
| `mise.test.local.toml` | `mise.test.local.lock` |

For example, with `MISE_ENV=test`:

```sh
MISE_ENV=test mise lock  # creates mise.lock AND mise.test.lock
```

Tools from `mise.toml` go to `mise.lock`, and tools from `mise.test.toml` go to `mise.test.lock`.

**Resolution**: When `MISE_ENV=test`, mise reads `mise.test.lock` for tools defined in `mise.test.toml` and `mise.lock` for tools in `mise.toml`. Environment-specific lockfiles are strictly scoped to their corresponding config — they only contain tools defined in that config.

This design means CI environments that don't set `MISE_ENV` only depend on `mise.lock`, so dev tool version bumps in `mise.dev.lock` won't invalidate CI caches.

Both `mise.lock` and `mise.<env>.lock` files should be committed to version control. `mise.local.lock` and `mise.<env>.local.lock` should be gitignored alongside their corresponding config files.

## Global Lockfiles

Tools declared in the global config (`~/.config/mise/config.toml`) are never locked by a
plain `mise lock`, which only targets the active project config root. Use `mise lock --global`:

```sh
mise lock --global              # update global (and system) config lockfiles
```

::: tip
This also applies when your global config is a symlink into a dotfiles repo, for example
`~/.config/mise/config.toml` -> `~/dotfiles/mise.toml`. mise reaches the same file through
both paths and treats it as the global config, so `mise lock` run from the repo reports that
nothing is configured in project scope. Run `mise lock --global` instead; the lockfile is
written next to the symlink target (`~/dotfiles/mise.lock`).
:::

## Local Lockfiles

Tools defined in `mise.local.toml` (which is typically gitignored) use a separate `mise.local.lock` file. This keeps local tool configurations separate from the committed lockfile.

```sh
# mise.local.toml tools go to mise.local.lock
mise use --path mise.local.toml node@22

# Regular mise.toml tools go to mise.lock
mise use --path mise.toml node@20
```

Use `mise lock --local` to update the local lockfile for all platforms:

```sh
mise lock --local              # update mise.local.lock
mise lock --local node python  # update specific tools in mise.local.lock
```

## Monorepos

When `monorepo_root = true`, mise can use a single lockfile at the monorepo root. Set `[monorepo] lockfile = true` to opt into root lockfile variants such as `mise.lock`, `mise.ci.lock`, and `mise.local.lock`.

Existing subproject lockfiles are migrated into the root lockfile on the next lock-aware command. Leaving the setting unset keeps per-subproject lockfiles during the rollout. Monorepos using `mise*.lock` files start warning in mise `2026.12.0`, and the unset default switches to root lockfiles in mise `2027.6.0`. Older mise versions do not understand this layout for subproject-owned tools, so projects that need mixed-version compatibility can pin the old behavior:

```toml
[monorepo]
lockfile = false
```

See [Monorepo Tasks](/tasks/monorepo.html#lockfiles) for details.

## File Format

The lockfile is TOML. This abbreviated example shows how a request is bound to
a version and how artifact metadata is stored for one platform. Generate the
entries your project needs with `mise lock` rather than copying this excerpt.

```toml [mise.lock]
lockfile_version = 2

[[tools.node]]
version = "26.8.1"
backend = "core:node"
specifiers = ["26.8.1"]

[tools.node."platforms.macos-arm64"]
checksum = "sha256:6e577fd0d9db776db82306629e441a9dace416702622aebdd171c9dfaa41f4d2"
url = "https://nodejs.org/dist/v26.8.1/node-v26.8.1-darwin-arm64.tar.gz"
```

New lockfiles use the current versioned format. Older lockfiles retain their format
during ordinary updates to avoid making them unreadable by collaborators using an
older mise. Run `mise lock --upgrade` to upgrade explicitly. Version 1 records each
original tool request in the concrete entry it resolved to. Version 2 references native
aube and uv dependency graphs in sidecar directories. Older mise versions reject version 2 lockfiles.

### Platform Information

A platform entry is written under a quoted key such as
`[tools.node."platforms.macos-arm64"]`. The platform identifier is usually
`os-arch`. Its metadata can include:

- **`checksum`** (optional): SHA256 or Blake3 hash for integrity verification
- **`size`** (legacy): File size in bytes; accepted when reading older lockfiles but omitted by the current writer
- **`url`** (optional): Artifact download URL
- **`url_api`** (optional): API download URL, for sources that require authenticated asset requests
- **`provenance`**: Verification method successfully used for the artifact
- **`signer`** and **`attested_by`**: Packslip identity commitments

### Tool Entry Fields

Each tool entry (`[[tools.name]]`) can contain:

- **`version`** (required): The exact version of the tool
- **`backend`** (optional): The backend used to install the tool (e.g., `core:node`, `aqua:BurntSushi/ripgrep`)
- **`specifiers`** (version 1 and newer): Original requests that resolve to this version and option variant
- **`options`** (optional): Backend-specific options that identify the artifact (e.g., `{exe = "rg", matching = "musl"}`)
- **`platforms`** (optional): Platform-specific metadata (checksums, URLs, sizes)
- **`aube`** (version 2, npm only): `{ path, digest }` reference to an embedded-aube sidecar directory
- **`uv`** (version 2, Python tools): `{ path, digest }` reference to a uv sidecar directory

A tool can have several entries for the same version when its artifact identity
depends on more than the platform key. Swift, for example, publishes a different
Linux tarball per distro, so its entries record which one they describe:

```toml
[[tools.swift]]
version = "6.3.1"
backend = "core:swift"
options = { swift_platform = "ubuntu24.04" }

[[tools.swift]]
version = "6.3.1"
backend = "core:swift"
options = { swift_platform = "fedora39" }
```

Entries are matched on options exactly, so a machine only verifies against the
entry written for its own distro. Pin `swift.platform` to make every Linux
machine resolve the same artifact, and commit the entry it produces. A platform
whose artifact the tool doesn't publish — `ubi9` has no arm64 build, for
instance — is reported as skipped rather than locked.

### Platform Keys

The platform key format is generally `os-arch` but can be customized by backends:

- **Standard format**: `linux-x64`, `macos-arm64`, `windows-x64`
- **Backend-specific**: Some backends like Java may use more specific platform identifiers
- **Tool-specific**: Backends like `ubi` may include additional tool-specific information in the platform key

## Backend Support

Inspect the generated entry for the actual tool and platform. Support varies
by backend, tool options, and whether a release uses a precompiled artifact or a
source build:

| Backend family                                                      | What to expect                                                                                                                |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Download backends such as aqua, GitHub/GitLab/Forgejo, HTTP, and S3 | Platform artifact metadata where the source supplies it or mise can compute it                                                |
| Packslip                                                            | Signed artifact information and signer commitments; policy is checked at installation                                         |
| Built-in languages                                                  | Tool-specific support; Node, Python, and Ruby have artifact-resolution paths, while external installers have different limits |
| npm (embedded aube) and PyPI (uv)                                   | Version 2 sidecars record transitive dependencies; see [dependency graphs](#dependency-graphs)                                |
| Other language package installers                                   | Top-level versions only; transitive dependencies and build inputs are not fully locked                                        |
| vfox tool plugins                                                   | Download URLs from plugin hooks can participate in strict URL locking                                                         |
| asdf and vfox backend plugins                                       | No strict URL-lock requirement; plugin execution still determines installation                                                |

A `provenance` field and a cryptographically verified provenance result are
separate states. See [provenance and security](#provenance-and-security) before
using lockfile metadata as evidence of verification.

## Best Practices

### Version Control

Commit the project configuration and lockfile together when changing requests:

```sh
git add mise.toml mise.lock
# Also stage the dependency sidecar directory if one was generated.
git commit -m "chore: update development tools"
```

Commit environment lockfiles alongside their shared configs. Keep `.local`
variants out of version control. Review changes to artifact URLs, backend options,
and verification metadata as well as version numbers.

### Team workflow

After updating tools, review the configuration, lockfile, and sidecar diffs.
Run `mise install --locked` and the project's checks before committing.
Teammates use the same install command after pulling.

### CI/CD

After checking out the repository and installing mise, use:

```yaml
- name: Install locked tools
  run: mise install
  env:
    MISE_LOCKED: "1"
```

Prepare entries for the runner's platform before committing the lockfile. If you
use [`jdx/mise-action`](https://github.com/jdx/mise-action), it also provides tool
installation and caching; keep the lockfile in the checkout used by the action.
A cache speeds up installation but does not replace the lockfile or its checks.

## Troubleshooting

### Regenerating Checksums

A checksum mismatch means the downloaded bytes differ from the recorded artifact.
First check the tool, platform, URL, and backend options in the error and lockfile.
A vendor may have replaced an asset, a mirror may be serving different content,
or the entry may describe another build.

After verifying an intentional upstream change, refresh only the affected tool's
metadata and review the diff:

```sh
mise lock node
git diff -- mise.lock
```

Replace `node` with the affected tool. Do not delete checksums or uninstall every
tool to bypass the failure. If the new artifact is unexpected, keep the existing
lockfile and investigate the release source before accepting new bytes.

### Ruby Precompiled Build Revision Releases

Precompiled Ruby binaries can have build revision releases for the same Ruby
version. The lockfile keeps `version = "3.3.11"` but pins the selected build
revision in the platform `url`:

```toml
url = "https://github.com/jdx/ruby/releases/download/3.3.11-1/ruby-3.3.11.x86_64_linux.tar.gz"
```

Here `3.3.11-1` is build revision `1`. See [Ruby precompiled build revisions](/lang/ruby.html#precompiled-build-revisions)
for details on why revisions exist, how unlocked installs behave, and how to
update older lockfiles.

### Lockfile Conflicts

When merging branches with different lockfiles:

1. Resolve the intended version requests in configuration first.
2. Resolve lockfile conflicts while preserving the corresponding request bindings
   and platform entries. Run `mise lock` to refresh metadata and inspect its diff.
3. Run `mise install` and the relevant project checks, then commit the result.

### Disabling for Specific Projects

```toml
# In project's mise.toml
[settings]
lockfile = false
```

## Provenance and Security

For supported backends, `mise lock` records verified provenance such as SLSA,
Cosign, Minisign, or GitHub attestations. New provenance is cryptographically
verified against the artifact for each target platform before it is recorded.
Existing `provenance_verified` values are preserved as inert compatibility
metadata for unchanged artifacts; mise no longer reads or adds this flag.

During installation, a checksum plus recorded provenance can
allow mise to skip repeating that provenance check. The lockfile is therefore a
trust input: review it and obtain it from a trusted project source. Artifact
checksum verification still applies. A provenance field alone is not proof that
the bytes were verified.

If GitHub Artifact Attestations are enabled but the GitHub API confirms none exist for a checksum-backed artifact, mise may record `github_attestations = "unavailable"`. This is a negative cache entry, not provenance: it only skips the redundant GitHub attestation probe on later installs from that lockfile. Other verification paths such as SLSA, Cosign, Minisign, and checksum verification still run as usual.

Attestations can be published after a release asset. Run `mise lock` again or
use `MISE_LOCKED_VERIFY_PROVENANCE=1 mise install` to discover attestations
added after they were recorded as unavailable.

For additional security, you can force provenance re-verification on every install:

```toml
[settings]
locked_verify_provenance = true
```

Or via environment variable:

```sh
MISE_LOCKED_VERIFY_PROVENANCE=1 mise install
```

This is also automatically enabled in [paranoid mode](/paranoid.html):

```toml
[settings]
paranoid = true
```

When enabled, supported verification paths run again for artifacts being
installed instead of trusting a previous lockfile verification result. This does
not create provenance for releases that never published it, and an already
installed tool may not be downloaded again. It is separate from Packslip signer
and signed-list policy.

## Minimum Release Age

In addition to lockfiles, mise uses the [`minimum_release_age`](/configuration/settings.html#minimum_release_age) setting to limit supply chain risk by installing only versions that have been available for a minimum amount of time. It defaults to `24h`:

```toml
[settings]
minimum_release_age = "7d"  # override the default 24h delay
```

This pairs well with lockfiles — use `minimum_release_age` to avoid picking up brand-new releases,
and lockfiles to pin the exact versions you've vetted. Once a version is selected from `mise.lock`,
installation does not reapply the age cutoff; the committed selection remains reproducible even
while the release is still inside the cooling window. This exemption covers the locked top-level
version, not unpinned transitive dependencies resolved during installation.

This setting filters top-level fuzzy version resolution for backends that provide release timestamps.
Versions without timestamps are included by default.

Only `npm:` and `pypi:` (also available as `pipx:`) currently forward the same cutoff into transitive
dependency resolution during install, including when the top-level version comes from a lockfile.
For frozen dependency graphs, the cutoff applies when resolving the graph; installation replays the
committed dependencies without resolving them again. Other
backends may select an older top-level tool version, but they do not constrain dependencies fetched by
the tool's installer/compiler.

## Migration from Other Tools

### From asdf

Preview importing an existing version file, then generate the configuration:

```sh
mise generate config --tool-versions .tool-versions --dry-run
mise generate config --tool-versions .tool-versions --yes
mise lock
mise install
```

Use this in a project without an existing `mise.toml`, or review and merge the
preview into the existing file. If teammates still use asdf, keep the shared
`.tool-versions` consistent; see [asdf migration](/dev-tools/comparison-to-asdf.html).

### From package.json engines

`engines.node` commonly describes a compatibility range such as `>=22`, not an
exact version request. Choose a supported Node.js release for the project, then
lock it explicitly:

```sh
mise use node@24
mise lock node
```

For automatic project discovery, mise reads the supported `devEngines` fields
after [idiomatic version files](/lang/node.html#nvmrc-node-version-and-package-json-support)
are enabled. Do not pass arbitrary npm range syntax directly to `mise use`.

## See Also

- [Configuration Settings](/configuration/settings) - All available settings
- [Tool Version Management](/dev-tools/) - How tool versions work
- [Backends](/dev-tools/backends/) - Backend-specific checksum support
