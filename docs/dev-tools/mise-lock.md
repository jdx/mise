# mise.lock Lockfile

`mise.lock` is a lockfile that pins exact versions and checksums of tools for reproducible environments. When enabled, mise will automatically maintain this file to ensure consistent tool versions across different machines and deployments.

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

1. **Automatic Creation**: When you run `mise install` or `mise use`, mise updates `mise.lock` with the exact versions installed
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

# Environment-specific version (only used when MISE_ENV=test)
[[tools.tiny]]
version = "2.1.0"
env = ["test"]
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
- **`env`** (optional): List of environment names this version applies to (e.g., `["test", "staging"]`)
- **`platforms`** (optional): Platform-specific metadata (checksums, URLs, sizes)

### Platform Keys

The platform key format is generally `os-arch` but can be customized by backends:

- **Standard format**: `linux-x64`, `macos-arm64`, `windows-x64`
- **Backend-specific**: Some backends like Java may use more specific platform identifiers
- **Tool-specific**: Backends like `ubi` may include additional tool-specific information in the platform key

## Environment-Specific Versions

When using [environment-specific configuration files](/configuration/environments) (e.g., `mise.test.toml`), tools from those files are tagged with an `env` field in the lockfile:

```toml
# mise.test.toml
[tools]
tiny = "2"
```

When you run `MISE_ENV=test mise use tiny@2`, the lockfile will include:

```toml
[[tools.tiny]]
version = "2.1.0"
env = ["test"]
```

**Resolution priority**: When resolving versions, mise checks in order:

1. Entry with matching `env` for the current `MISE_ENV`
2. Base entry (no `env` field)
3. First available entry

This allows different environments to use different tool versions while sharing the same lockfile.

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

When enabled, `mise install` will fail if a tool doesn't have a URL for the current platform in the lockfile. To fix this, first populate the lockfile with URLs:

```sh
mise lock                    # generate URLs for all platforms
mise lock --platform linux-x64,macos-arm64  # or specific platforms
```

This is useful for CI environments where you want to guarantee reproducible builds without any external API dependencies.

## Workflow

### Initial Setup

```sh
# Create the lockfile
touch mise.lock

# Install tools (this will populate the lockfile)
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
mise use node@24

# This will update both the installation and mise.lock
```

## Backend Support

Backend support for lockfile features varies:

- ✅ **Full support** (version + checksum + size + URL): `aqua`, `http`, `github`, `gitlab`
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

# Enable lockfiles and populate
mise settings lockfile=true
mise install
```

### From package.json engines

```sh
# Set versions based on package.json
mise use node@$(jq -r '.engines.node' package.json)
```

## See Also

- [Configuration Settings](/configuration/settings) - All available settings
- [Tool Version Management](/dev-tools/) - How tool versions work
- [Backends](/dev-tools/backends/) - Backend-specific checksum support
