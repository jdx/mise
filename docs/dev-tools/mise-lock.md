# mise.lock Lockfile

`mise.lock` is a lockfile that pins exact versions and checksums of tools for reproducible environments. Lockfiles are not created automatically—you must run `mise lock` to generate them. Once a lockfile exists, mise will keep it updated as tools are installed or upgraded.

## Overview

The lockfile serves similar purposes to `package-lock.json` in npm or `Cargo.lock` in Rust:

- **Reproducible builds**: Ensures everyone on your team uses exactly the same tool versions
- **Security**: Verifies tool integrity with checksums when supported by the backend
- **Version pinning**: Locks tools to specific versions while allowing flexibility in `mise.toml`
- **Avoids API rate limits**: By storing download URLs, future installs use the lockfile and do not need to call GitHub (or other providers), avoiding rate limits and the need for `GITHUB_TOKEN` in most cases

## Enabling Lockfiles

Lockfiles are controlled by the `lockfile` setting:

```sh
# Enable lockfiles globally
mise settings lockfile=true

# Or set in mise.toml
[settings]
lockfile = true
```

## How It Works

1. **Lockfile Updates**: Once a `mise.lock` file exists, running `mise install` or `mise use` updates it with the exact versions installed
2. **Version Resolution**: If a `mise.lock` exists, mise will prefer locked versions over version ranges in `mise.toml`
3. **Checksum Verification**: For supported backends, mise stores and verifies checksums of downloaded tools

## File Format

`mise.lock` is a TOML file with a platform-based format that organizes asset information by platform:

```toml
# Example mise.lock
[[tools.node]]
version = "20.11.0"
backend = "core:node"

[tools.node.platforms.linux-x64]
checksum = "sha256:a6c213b7a2c3b8b9c0aaf8d7f5b3a5c8d4e2f4a5b6c7d8e9f0a1b2c3d4e5f6a7"
size = 23456789
url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz"

[[tools.python]]
version = "3.11.7"
backend = "core:python"

[tools.python.platforms.linux-x64]
checksum = "sha256:def456..."
size = 12345678

# Tool with backend-specific options
[[tools.ripgrep]]
version = "14.1.1"
backend = "aqua:BurntSushi/ripgrep"
options = { exe = "rg" }

[tools.ripgrep.platforms.linux-x64]
checksum = "sha256:4cf9f2741e6c465ffdb7c26f38056a59e2a2544b51f7cc128ef28337eeae4d8e"
size = 1234567

```

### Platform Information

Each platform in a tool's `[tools.name.platforms]` section uses a key format like `"os-arch"` (e.g., `"linux-x64"`, `"macos-arm64"`) and can contain:

- **`checksum`** (optional): SHA256 or Blake3 hash for integrity verification
- **`size`** (optional): File size in bytes for download validation
- **`url`** (optional): Original download URL for reference or re-downloading

### Tool Entry Fields

Each tool entry (`[[tools.name]]`) can contain:

- **`version`** (required): The exact version of the tool
- **`backend`** (optional): The backend used to install the tool (e.g., `core:node`, `aqua:BurntSushi/ripgrep`)
- **`options`** (optional): Backend-specific options that identify the artifact (e.g., `{exe = "rg", matching = "musl"}`)
- **`platforms`** (optional): Platform-specific metadata (checksums, URLs, sizes)

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

Tools from `mise.toml` go to `mise.lock`, tools from `mise.test.toml` go to `mise.test.lock`.

**Resolution**: When `MISE_ENV=test`, mise reads `mise.test.lock` for tools defined in `mise.test.toml` and `mise.lock` for tools in `mise.toml`. Environment-specific lockfiles are strictly scoped to their corresponding config — they only contain tools defined in that config.

This design means CI environments that don't set `MISE_ENV` only depend on `mise.lock`, so dev tool version bumps in `mise.dev.lock` won't invalidate CI caches.

Both `mise.lock` and `mise.<env>.lock` files should be committed to version control. `mise.local.lock` and `mise.<env>.local.lock` should be gitignored alongside their corresponding config files.

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

## Strict Lockfile Mode

The `locked` setting enforces that all tools have pre-resolved URLs in the lockfile before installation. This prevents API calls to GitHub, aqua registry, etc., ensuring fully reproducible installations.

```sh
# Enable strict mode
mise settings locked=true

# Or via environment variable
MISE_LOCKED=1 mise install
```

::: warning
All mise settings are global in scope. Setting `locked = true` in a project's `mise.toml` applies to **all** tool resolution, including tools from your global `~/.config/mise/config.toml`. If you see warnings about global tools missing from the lockfile, run `mise lock -g` to generate a global lockfile.
:::

When enabled, `mise install` will fail if a tool doesn't have a URL for the current platform in the lockfile. To fix this, first populate the lockfile with URLs:

```sh
mise lock                    # generate URLs for all platforms
mise lock --platform linux-x64,macos-arm64  # or specific platforms
```

This is useful for CI environments where you want to guarantee reproducible builds without any external API dependencies.

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
| `mise lock node@22.15.0`    | No       | Only if version doesn't match prefix | Yes                                     |

**Key points:**

- **`mise use`** is for changing which version you want in your config — it always writes to `mise.toml`
- **`mise install`** installs what's in your config without changing it — `mise install node` installs the config's version of node and updates the lockfile, while `mise install node@22.15.0` is a one-off that doesn't
- **`mise upgrade`** upgrades tools within their configured ranges and updates the lockfile — passing `tool@version` lets you target a specific version
- **`mise lock`** regenerates lockfile entries without installing — passing `tool@version` lets you pin a specific version

## Backend Support

Backend support for lockfile features varies:

- ✅ **Full support** (version + checksum + size + URL): `aqua`, `http`, `github`, `gitlab`
  - _Provenance support_: `aqua`, `github`, `core:python` (precompiled binaries), `core:ruby` (precompiled binaries), `core:zig` (install-time)
- ⚠️ **Partial support** (version + URL + provenance): `vfox` (tool plugins only)
- ⚠️ **Partial support** (version + checksum + size): `ubi`
- 📝 **Basic support** (version + checksum): `core` (some tools)
- 📝 **Version only**: `asdf`, `npm`, `cargo`, `pipx`
- 📝 **Planned**: More backends will add full asset tracking support over time

## Best Practices

### Version Control

```sh
# Always commit the lockfile
git add mise.lock
git commit -m "Update tool versions"
```

### Team Workflow

1. **Team Lead**: Updates `mise.toml` with new version ranges
2. **Team Lead**: Runs `mise install` to update `mise.lock`
3. **Team Lead**: Commits both files
4. **Team Members**: Pull changes and run `mise install` to get exact versions

### CI/CD

```yaml
# Example GitHub Actions
- name: Install tools
  run: |
    mise install  # Uses exact versions from mise.lock

- name: Cache lockfile
  uses: actions/cache@v5
  with:
    key: mise-lock-${{ hashFiles('mise.lock') }}
```

## Troubleshooting

### Regenerating Checksums

If checksums become invalid or you need to regenerate them:

```sh
# Remove all tools and reinstall
mise uninstall --all
mise install
```

### Lockfile Conflicts

When merging branches with different lockfiles:

1. Resolve conflicts in `mise.lock`
2. Run `mise install` to verify everything works
3. Commit the resolved lockfile

### Disabling for Specific Projects

```toml
# In project's mise.toml
[settings]
lockfile = false
```

## Migration from Other Tools

### From asdf

```sh
# Convert .tool-versions to mise.toml
mise config generate

# Enable lockfiles and generate the lockfile
mise settings lockfile=true
mise lock
mise install
```

### From package.json engines

```sh
# Set versions based on package.json
mise use node@$(jq -r '.engines.node' package.json)
```

## Provenance and Security

When `mise lock` generates a lockfile, it records a verified provenance type (e.g., `slsa`, `cosign`, `minisign`, `github-attestations`) for each tool when one is available. For the **current platform**, mise downloads the artifact and performs full cryptographic verification at lock time -- ensuring the provenance entry in the lockfile is backed by actual verification, not just registry metadata. This applies to both the aqua and github backends. For cross-platform entries, provenance is detected from registry metadata without verification (since the artifact may not be runnable on the current machine).

By default, when `mise install` sees a lockfile with both a checksum and a verified provenance entry, it trusts the lockfile and skips re-verification. This avoids redundant API calls (e.g., GitHub attestation queries) which can cause rate limit issues in CI. Since the current platform's provenance was already verified during `mise lock`, this is safe.

If GitHub Artifact Attestations are enabled but the GitHub API confirms none exist for a checksum-backed artifact, mise may record `github_attestations = "unavailable"`. This is a negative cache entry, not provenance: it only skips the redundant GitHub attestation probe on later installs from that lockfile. Other verification paths such as SLSA, Cosign, Minisign, and checksum verification still run as usual.

GitHub's docs show binary attestations generated from an existing artifact path with [`actions/attest`](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations#generating-build-provenance-for-binaries), and the REST API lists attestations by [subject digest](https://docs.github.com/en/rest/orgs/attestations#list-attestations). That means an attestation can appear after the release asset was uploaded. A later `mise lock` run or `MISE_LOCKED_VERIFY_PROVENANCE=1 mise install` can discover attestations added after the lockfile recorded them as unavailable.

For additional security, you can force provenance re-verification at install time on every install:

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

When enabled, every `mise install` will cryptographically verify provenance regardless of what the lockfile contains, ensuring the artifact was built by a trusted CI pipeline.

## Minimum Release Age

In addition to lockfiles, mise supports the [`minimum_release_age`](/configuration/settings.html#minimum_release_age) setting to limit supply chain risk by only installing versions that have been available for a minimum amount of time:

```toml
[settings]
minimum_release_age = "7d"  # only resolve to versions released more than 7 days ago
```

This pairs well with lockfiles — use `minimum_release_age` to avoid picking up brand-new releases, and lockfiles to pin the exact versions you've vetted.

Some package-manager backends also forward this cutoff into transitive dependency resolution during
install. This includes `npm:` and `pipx:` tools.

## See Also

- [Configuration Settings](/configuration/settings) - All available settings
- [Tool Version Management](/dev-tools/) - How tool versions work
- [Backends](/dev-tools/backends/) - Backend-specific checksum support
