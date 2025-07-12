# Plugin Publishing

This guide shows how to publish and distribute your plugins, whether they are backend plugins or tool plugins. Publishing makes your plugins available to other users and ensures they can be easily installed and maintained.

## Publishing Checklist

Before publishing your plugin, ensure you have:

### Essential Files

- **`metadata.lua`** - Plugin metadata with name, version, description, and author
- **Plugin implementation** - Either backend methods or hook functions
- **Test coverage** - Automated tests to verify functionality

### Optional but Recommended

- **`README.md`** - Basic usage instructions and examples
- **`test/`** directory - Test scripts for verification
- **Version control** - Git repository with proper versioning

## Repository Setup

### 1. Initialize Repository

Create a Git repository for your plugin:

```bash
# Create plugin directory
mkdir my-plugin
cd my-plugin

# Initialize git repository
git init
git remote add origin https://github.com/username/my-plugin.git

# Create initial structure
touch metadata.lua
mkdir -p test
echo "# My Plugin" > README.md
```

### 2. Basic Directory Structure

Organize your plugin with this structure:

```
my-plugin/
├── metadata.lua          # Plugin metadata
├── README.md            # Basic documentation
├── test/                # Test scripts
│   └── test.sh
├── .gitignore           # Git ignore rules
└── [implementation files]
```

For backend plugins:

```
backend-plugin/
├── metadata.lua          # Backend methods implementation
├── README.md
└── test/
    └── test.sh
```

For tool plugins:

```
tool-plugin/
├── metadata.lua          # Plugin metadata
├── hooks/               # Hook implementations
│   ├── available.lua
│   ├── pre_install.lua
│   └── env_keys.lua
├── lib/                 # Helper libraries
│   └── helper.lua
├── README.md
└── test/
    └── test.sh
```

### 3. Git Ignore Configuration

Create a `.gitignore` file:

```gitignore
# Temporary files
*.tmp
*.temp
.DS_Store
Thumbs.db

# Test artifacts
test/tmp/
test/output/

# IDE files
.vscode/
.idea/
*.swp
*.swo

# OS files
*.log
```

## Versioning Strategy

### Semantic Versioning

Use semantic versioning (SemVer) for your plugin releases:

- **Major version** (1.0.0 → 2.0.0): Breaking changes
- **Minor version** (1.0.0 → 1.1.0): New features, backward compatible
- **Patch version** (1.0.0 → 1.0.1): Bug fixes, backward compatible

### Version Management

Update version in `metadata.lua`:

```lua
PLUGIN = {
    name = "my-plugin",
    version = "1.2.3",  -- Update this for each release
    description = "My awesome plugin",
    author = "Your Name"
}
```

Create git tags for releases:

```bash
# Tag the current commit
git tag -a v1.2.3 -m "Release version 1.2.3"

# Push tags to repository
git push origin --tags
```

## Testing Before Publication

### Automated Testing

Create comprehensive test scripts:

```bash
#!/bin/bash
# test/test.sh
set -e

echo "Testing plugin functionality..."

# Install plugin locally
mise plugin install my-plugin .

# Test basic functionality
if [[ "$(mise ls-remote my-plugin)" == "" ]]; then
    echo "ERROR: No versions available"
    exit 1
fi

# Test installation
mise install my-plugin@latest

# Test execution
mise exec my-plugin:tool -- --version

# Clean up
mise plugin remove my-plugin

echo "All tests passed!"
```

### Manual Testing

Test your plugin manually:

```bash
# Link for development
mise plugin link my-plugin /path/to/plugin

# Test all functionality
mise ls-remote my-plugin
mise install my-plugin@latest
mise use my-plugin@latest

# Test in different environments
docker run --rm -it ubuntu:latest bash -c "
    curl -fsSL https://mise.jdx.dev/install.sh | sh
    mise plugin install my-plugin https://github.com/username/my-plugin
    mise install my-plugin@latest
"
```

## Publishing Process

### 1. Prepare for Release

Before publishing, ensure everything is ready:

```bash
# Run tests
./test/test.sh

# Check git status
git status

# Update version in metadata.lua
vim metadata.lua

# Commit changes
git add .
git commit -m "Prepare release v1.2.3"
```

### 2. Create Release

Create a tagged release:

```bash
# Create and push tag
git tag -a v1.2.3 -m "Release version 1.2.3"
git push origin v1.2.3
git push origin main
```

### 3. GitHub Releases (Recommended)

Create a GitHub release for better discoverability:

1. Go to your repository on GitHub
2. Click "Releases" → "Create a new release"
3. Choose your tag (v1.2.3)
4. Write release notes describing changes
5. Publish the release

### 4. Release Notes Template

```markdown
## Changes in v1.2.3

### Added
- New feature X
- Support for Y

### Changed
- Improved performance of Z
- Updated dependencies

### Fixed
- Fixed issue with A
- Resolved bug in B

### Installation
```bash
mise plugin install my-plugin https://github.com/username/my-plugin
```

```

## Distribution Methods

### 1. Direct Git Installation

Users can install directly from your repository:

```bash
# Install from GitHub
mise plugin install my-plugin https://github.com/username/my-plugin

# Install specific version
mise plugin install my-plugin https://github.com/username/my-plugin@v1.2.3

# Install from other Git providers
mise plugin install my-plugin https://gitlab.com/username/my-plugin
```

### 2. Private Repository Access

For private repositories, users need access:

```bash
# SSH access (recommended)
mise plugin install my-plugin git@github.com:username/private-plugin.git

# HTTPS with token
mise plugin install my-plugin https://username:token@github.com/username/private-plugin.git
```

### 3. Archive Distribution

You can also distribute as archives:

```bash
# Create release archive
git archive --format=zip --output=my-plugin-v1.2.3.zip v1.2.3

# Users can install from archive
mise plugin install my-plugin https://github.com/username/my-plugin/releases/download/v1.2.3/my-plugin-v1.2.3.zip
```

## Maintenance and Updates

### 1. Update Workflow

Establish a regular update process:

```bash
# Development workflow
git checkout -b feature/new-feature
# ... make changes ...
git commit -m "Add new feature"
git push origin feature/new-feature

# After review and merge
git checkout main
git pull origin main
git tag -a v1.3.0 -m "Release v1.3.0"
git push origin v1.3.0
```

### 2. Backward Compatibility

Maintain backward compatibility when possible:

- Keep existing plugin interface unchanged
- Add new features as optional
- Deprecate old features gradually
- Document breaking changes clearly

### 3. User Communication

Keep users informed about updates:

- Use clear release notes
- Announce major changes
- Provide migration guides for breaking changes
- Maintain documentation

## Security Considerations

### 1. Code Review

- Review all code changes before publishing
- Check for security vulnerabilities
- Validate external dependencies
- Test with untrusted inputs

### 2. Dependency Management

- Pin dependency versions where possible
- Regularly update dependencies
- Monitor for security advisories
- Use trusted sources only

### 3. Access Control

- Limit repository access appropriately
- Use strong authentication
- Regularly audit access permissions
- Consider signed releases for sensitive plugins

## Best Practices

### 1. Documentation

- Keep README.md concise but complete
- Include usage examples
- Document configuration options
- Provide troubleshooting guide

### 2. Testing

- Test on multiple platforms
- Include edge cases
- Test upgrade scenarios
- Automate testing where possible

### 3. Community

- Respond to issues promptly
- Accept contributions gracefully
- Maintain consistent code style
- Be helpful and respectful

### 4. Release Management

- Follow semantic versioning
- Create clear release notes
- Test releases thoroughly
- Maintain stable branches

## Troubleshooting

### Common Issues

**Plugin not installing:**

```bash
# Check repository URL
git clone https://github.com/username/my-plugin.git

# Verify metadata.lua exists
ls -la my-plugin/metadata.lua

# Test locally
mise plugin link my-plugin ./my-plugin
```

**Version conflicts:**

```bash
# Check version in metadata.lua
grep version my-plugin/metadata.lua

# Verify git tags
git tag -l
```

**Permission issues:**

```bash
# Check repository permissions
git ls-remote https://github.com/username/my-plugin.git

# For private repos, verify access
ssh -T git@github.com
```

## Next Steps

- [Backend Plugin Development](backend-plugin-development.md)
- [Tool Plugin Development](tool-plugin-development.md)
- [Plugin Lua Modules](plugin-lua-modules.md)

## Examples

### Simple Backend Plugin Release

```bash
# 1. Prepare plugin
cd my-backend-plugin
echo "Updated backend methods" > metadata.lua

# 2. Test locally
mise plugin link my-plugin .
mise ls-remote my-plugin:tool

# 3. Release
git add .
git commit -m "v1.0.0: Initial release"
git tag -a v1.0.0 -m "Initial release"
git push origin v1.0.0
```

### Tool Plugin with Hooks

```bash
# 1. Prepare plugin
cd my-tool-plugin
./test/test.sh  # Run tests

# 2. Update version
sed -i 's/version = "1.0.0"/version = "1.1.0"/' metadata.lua

# 3. Release
git add .
git commit -m "v1.1.0: Add new hook functionality"
git tag -a v1.1.0 -m "Add new hook functionality"
git push origin v1.1.0
```
