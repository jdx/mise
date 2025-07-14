# mise.lock Lockfile <Badge type="warning" text="experimental" />

`mise.lock` is a lockfile that pins exact versions and checksums of tools for reproducible environments. When enabled, mise will automatically maintain this file to ensure consistent tool versions across different machines and deployments.

## Overview

The lockfile serves similar purposes to `package-lock.json` in npm or `Cargo.lock` in Rust:

- **Reproducible builds**: Ensures everyone on your team uses exactly the same tool versions
- **Security**: Verifies tool integrity with checksums when supported by the backend
- **Version pinning**: Locks tools to specific versions while allowing flexibility in `mise.toml`

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

`mise.lock` is a TOML file with a consolidated format that avoids duplication:

```toml
# Example mise.lock
[tools.node]
version = "20.11.0"
backend = "core:node"

[tools.python]
version = "3.11.7"
backend = "core:python"

[tools.go]
version = "1.21.5"
backend = "core:go"

[tools.ripgrep]
version = "14.1.1"
backend = "aqua:BurntSushi/ripgrep"

# Consolidated assets section - no duplication of filenames
[assets]
"node-v20.11.0-linux-x64.tar.xz" = { 
  checksum = "sha256:a6c213b7a2c3b8b9c0aaf8d7f5b3a5c8d4e2f4a5b6c7d8e9f0a1b2c3d4e5f6a7", 
  size = 23456789,
  url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz"
}
"python-3.11.7-linux-x64.tar.xz" = { 
  checksum = "sha256:def456...",
  size = 12345678
}
"ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz" = { 
  checksum = "sha256:4cf9f2741e6c465ffdb7c26f38056a59e2a2544b51f7cc128ef28337eeae4d8e",
  size = 1234567
}
```

### Asset Information

Each asset in the `[assets]` section can contain:

- **`checksum`** (optional): SHA256 or Blake3 hash for integrity verification
- **`size`** (optional): File size in bytes for download validation
- **`url`** (optional): Original download URL for reference or re-downloading

### Legacy Format Migration

Older lockfiles with individual `[tools.name.checksums]` sections are automatically migrated to the new consolidated format when read. The migration is seamless and maintains all existing functionality.

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
mise use node@22

# This will update both the installation and mise.lock
```

## Backend Support

Not all backends support checksums in lockfiles:

- ✅ **Full support** (version + checksum + size): `http`, `github`, `gitlab`, `ubi`
- ⚠️ **Partial support** (version + checksum): `aqua`, `core` (some tools)
- 📝 **Version only**: `asdf`, `npm`, `cargo`, `pipx`
- 📝 **Planned**: More backends will add checksum and size support over time

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
  uses: actions/cache@v3
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

## Experimental Features

Since lockfiles are still experimental, enable them with:

```sh
mise settings experimental=true
mise settings lockfile=true
```

## Benefits of the New Format

The consolidated assets format provides several advantages:

1. **Reduced Duplication**: Filenames are no longer repeated across multiple tools
2. **Centralized Management**: All asset information is in one place
3. **Extended Metadata**: Support for file sizes and download URLs
4. **Better Performance**: Smaller lockfiles load faster
5. **Easier Maintenance**: Simpler structure for tools and automation

## See Also

- [Configuration Settings](/configuration/settings) - All available settings
- [Tool Version Management](/dev-tools/) - How tool versions work
- [Backends](/dev-tools/backends/) - Backend-specific checksum support
