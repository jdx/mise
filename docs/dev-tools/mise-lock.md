# mise.lock Lockfile

`mise.toml` records the versions a project accepts; `mise.lock` records the
concrete versions those requests resolved to. Supported backends also record
artifact URLs, checksums, and verification metadata. Commit both files so other
machines can use the same resolutions.

## Overview

A lockfile separates routine installation from intentional updates:

```sh
mise lock            # resolve configured tools without installing them
mise install         # install the recorded versions
mise lock --bump node # update Node's resolution within its configured request
```

Review the lockfile diff before committing an update. Locking tool versions does
not lock your application's packages, system libraries, or every dependency an
external installer fetches. Keep ecosystem lockfiles such as `package-lock.json`
and `uv.lock` as well.

Stored URLs reduce release-discovery API calls. Private downloads, uncached
artifacts, and verification or policy checks can still require network access and
authentication. See [backend support](#backend-support) and
[GitHub tokens](/dev-tools/github-tokens.html).

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

## How It Works

1. **Lockfile Creation and Updates**: With `lockfile = true`, running `mise install` or `mise use` creates or updates `mise.lock` with the exact versions installed. When `lockfile` is unset, these commands update an existing lockfile without creating one
2. **Version Resolution**: mise reuses the lock entry matching the configured request, backend, and options
3. **Checksum Verification**: For supported backends, mise stores and verifies checksums of downloaded tools

`mise lock` resolves both config-level tools and tools declared in individual tasks. It reads task
definitions—including inherited templates and included task files—but does not run tasks, their
dependencies, hooks, or tool installers. This lets task-specific tools be locked before the first
task execution. Their entries use the same `[[tools.*]]` format and are written to the lockfile for
the config that owns the task.

## File Format

The lockfile is TOML. This abbreviated example shows how a request is bound to
a version and how artifact metadata is stored for one platform. Generate the
entries your project needs with `mise lock` rather than copying this excerpt.

```toml [mise.lock]
lockfile_version = 1

[[tools.node]]
version = "26.8.1"
backend = "core:node"
specifiers = ["26.8.1"]

[tools.node."platforms.macos-arm64"]
checksum = "sha256:6e577fd0d9db776db82306629e441a9dace416702622aebdd171c9dfaa41f4d2"
url = "https://nodejs.org/dist/v26.8.1/node-v26.8.1-darwin-arm64.tar.gz"
```

New lockfiles use the current versioned format. Unversioned lockfiles are treated as
version 0 and remain in that format during ordinary updates to avoid unexpected lockfile
drift. Run `mise lock --upgrade` to deliberately upgrade legacy lockfiles. Version 1
records each original tool request in the concrete entry it resolved to, so overlapping
requests such as `"1"` and `"1.0.0"` can select different locked versions reliably.

### Platform Information

A platform entry is written under a quoted key such as
`[tools.node."platforms.macos-arm64"]`. The platform identifier is usually
`os-arch`. Its metadata can include:

- **`checksum`** (optional): SHA256 or Blake3 hash for integrity verification
- **`size`** (legacy): File size in bytes; accepted when reading older lockfiles but omitted by the current writer
- **`url`** (optional): Artifact download URL
- **`url_api`** (optional): API download URL, for sources that require authenticated asset requests
- **`provenance`** and **`provenance_verified`**: Available verification method and whether verification succeeded
- **`signer`** and **`attested_by`**: Packslip identity commitments

### Tool Entry Fields

Each tool entry (`[[tools.name]]`) can contain:

- **`version`** (required): The exact version of the tool
- **`backend`** (optional): The backend used to install the tool (e.g., `core:node`, `aqua:BurntSushi/ripgrep`)
- **`specifiers`** (version 1): Original requests that resolve to this version and option variant
- **`options`** (optional): Backend-specific options that identify the artifact (e.g., `{exe = "rg", matching = "musl"}`)
- **`platforms`** (optional): Platform-specific metadata (checksums, URLs, sizes)

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

## Strict Lockfile Mode

The `locked` setting requires a lockfile URL for the current platform before
installing a tool through a backend that supports URL-based locking. It catches
missing artifact resolutions instead of silently resolving them during install.
It is not an offline mode, and some backends are exempt.

```sh
# Enable strict mode
mise settings set locked=true

# Or via environment variable
MISE_LOCKED=1 mise install
```

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

When enabled, `mise install` fails if a tool has no URL for the current platform in the lockfile. To fix this, populate the lockfile with URLs first:

```sh
mise lock                    # refresh existing platforms, or the default set for a new file
mise lock --platform linux-x64,macos-arm64  # or specific platforms
```

The check only covers backends that can record a URL. `asdf`, `cargo`, `gem`, `go`, `npm`, `pipx`, `pkgx`, `ubi`, `core:dotnet`, `core:rust`, and `core:swift` install through an external tool or resolve their download at install time, and vfox _backend_ plugins cannot yet report one, so strict mode skips them instead of failing — a config that mixes them with lockable tools still installs. vfox _tool_ plugins do record a URL and are checked like any other lockable backend. Tools resolved from a [tool stub](/dev-tools/tool-stubs) are skipped as well. See [Backend Support](#backend-support) for what each backend records.

Use strict mode in CI to catch incomplete lock entries for supported backends.
Verification and authenticated downloads may still make API requests.

## Workflow

### Initial Setup

```sh
# Generate the lockfile
mise lock

# Install tools using locked versions
mise install
```

### Daily Usage

```sh
# Install exact versions from lockfile
mise install

# Update tools and lockfile
mise upgrade
```

### Updating Versions

When you want to update tool versions:

```sh
# Update tool version in mise.toml
mise use node@26

# This will update both the installation and mise.lock
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

The table below shows how each command interacts with `mise.toml` and `mise.lock`:

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

**Key points:**

- **`mise use`** changes the requested version in the selected config file (normally `mise.toml`)
- **`mise install`** installs what's in your config without changing it — `mise install node` installs the config's version of node and updates the lockfile, while `mise install node@22.15.0` is a one-off that doesn't
- **`mise upgrade`** upgrades tools within their configured ranges and updates the lockfile — passing `tool@version` lets you target a specific version
- **`mise lock`** regenerates lockfile entries without installing — passing `tool@version` lets you pin a specific version, and `--bump` advances fuzzy selectors to the latest matching versions

## Backend Support

Inspect the generated entry for the actual tool and platform. Support varies
by backend, tool options, and whether a release uses a precompiled artifact or a
source build:

| Backend family                                                      | What to expect                                                                                                                |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Download backends such as aqua, GitHub/GitLab/Forgejo, HTTP, and S3 | Platform artifact metadata where the source supplies it or mise can compute it                                                |
| Packslip                                                            | Signed artifact information and signer commitments; policy is checked at installation                                         |
| Built-in languages                                                  | Tool-specific support; Node, Python, and Ruby have artifact-resolution paths, while external installers have different limits |
| Language package installers                                         | A top-level tool version does not lock all transitive packages or build inputs                                                |
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
git commit -m "chore: update development tools"
```

Commit environment lockfiles alongside their shared configs. Keep `.local`
variants out of version control. Review changes to artifact URLs, backend options,
and verification metadata as well as version numbers.

### Team Workflow

1. Change a request with `mise use`, or advance an existing request with
   `mise lock --bump <tool>`.
2. Run `mise install` and the project's relevant checks.
3. Commit the reviewed configuration and lockfile changes.
4. After pulling, teammates run `mise install` to install the recorded versions.

Anyone updating the project can follow this workflow; lockfile changes do not
require a separate team role.

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

## Provenance and Security

For supported backends, `mise lock` records available provenance metadata such
as SLSA, Cosign, Minisign, or GitHub attestations. Aqua and GitHub can download
and cryptographically verify the current platform's artifact at lock time.
A successful check is recorded separately with `provenance_verified`.
Cross-platform metadata may describe an available verification method without
having been verified locally.

During installation, a checksum plus a recorded verified provenance result can
allow mise to skip repeating that provenance check. The lockfile is therefore a
trust input: review it and obtain it from a trusted project source. Artifact
checksum verification still applies. A provenance field alone is not proof that
the bytes were verified.

If GitHub Artifact Attestations are enabled but the GitHub API confirms none exist for a checksum-backed artifact, mise may record `github_attestations = "unavailable"`. This is a negative cache entry, not provenance: it only skips the redundant GitHub attestation probe on later installs from that lockfile. Other verification paths such as SLSA, Cosign, Minisign, and checksum verification still run as usual.

GitHub's docs show binary attestations generated from an existing artifact path with [`actions/attest`](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations#generating-build-provenance-for-binaries), and the REST API lists attestations by [subject digest](https://docs.github.com/en/rest/orgs/attestations#list-attestations). That means an attestation can appear after the release asset was uploaded. A later `mise lock` run or `MISE_LOCKED_VERIFY_PROVENANCE=1 mise install` can discover attestations added after the lockfile recorded them as unavailable.

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

This pairs well with lockfiles — use `minimum_release_age` to avoid picking up brand-new releases, and lockfiles to pin the exact versions you've vetted.

This setting filters top-level fuzzy version resolution for backends that provide release timestamps.
Versions without timestamps are included by default.

Only `npm:` and `pipx:` currently forward the same cutoff into transitive dependency resolution during
install. Other backends may select an older top-level tool version, but they do not constrain
dependencies fetched by the tool's installer/compiler.

## See Also

- [Configuration Settings](/configuration/settings) - All available settings
- [Tool Version Management](/dev-tools/) - How tool versions work
- [Backends](/dev-tools/backends/) - Backend-specific checksum support
