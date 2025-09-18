# Mise Product Features and User Guide

## What is Mise?

Mise (pronounced "mise-en-place") is a comprehensive development environment management tool that combines three essential functions:

1. **Tool Version Manager** - Like asdf, nvm, or pyenv but for any language
2. **Environment Variable Manager** - Like direnv for project-specific environments
3. **Task Runner** - Like make for building and testing projects

**Tagline**: "The front-end to your dev env"

## Core Features

### 1. Development Tool Management

**Supported Tools**: 2000+ tools including:

- **Languages**: Node.js, Python, Go, Java, Ruby, Rust, Swift, Zig, Elixir, Erlang, Deno, Bun
- **CLI Tools**: terraform, kubectl, docker, git, jq, ripgrep, shellcheck
- **Build Tools**: cmake, ninja, gradle, maven
- **Cloud Tools**: aws-cli, gcloud, azure-cli

**Key Capabilities**:

- Install multiple versions of any tool simultaneously
- Automatic version switching per project directory
- Real binaries (no shims) with fast performance
- Global and project-specific tool versions
- Idiomatic version files (`.node-version`, `.python-version`, etc.)

**Example Usage**:

```bash
# Install and use specific versions
mise use node@20 python@3.12 go@latest

# Execute with specific tool version
mise exec node@18 -- npm test

# Install tools from configuration file
mise install
```

### 2. Environment Variable Management

**Features**:

- Project-specific environment variables
- Template system with Tera for dynamic values
- `.env` file loading support
- Environment inheritance and overrides
- Shell integration for automatic activation

**Configuration Example**:

```toml
# mise.toml
[env]
DATABASE_URL = "postgres://localhost/{{project_name}}"
NODE_ENV = "development"
API_KEY = {"file" = ".api-key"}
PATH = ["./node_modules/.bin", "$PATH"]
```

### 3. Task Management System

**Features**:

- Task dependencies with directed acyclic graph
- Multiple execution environments
- File watching for automatic task execution
- Parallel task execution
- Task inheritance and overrides

**Configuration Example**:

```toml
# mise.toml
[tasks.build]
description = "Build the project"
run = "cargo build"
sources = ["src/**/*.rs", "Cargo.*"]
outputs = ["target/debug/myapp"]

[tasks.test]
description = "Run tests"
depends = ["build"]
run = "cargo test"

[tasks.deploy]
description = "Deploy to production"
depends = ["test", "lint"]
run = """
docker build -t myapp .
docker push myapp
kubectl apply -f k8s/
"""
```

## Backend Ecosystem

### Core Plugins (Built-in)

Fast, native implementations for major languages:

- Node.js with npm/yarn/pnpm support
- Python with pyenv compatibility
- Go with version management
- Java with multiple distributions (OpenJDK, Eclipse Temurin, etc.)
- Ruby with rbenv compatibility
- Rust via rustup integration

### External Plugin Support

- **ASDF Plugins**: 500+ community plugins
- **Vfox Plugins**: Modern Lua-based plugin system
- **Aqua Registry**: 1000+ tools with security verification
- **GitHub Releases**: Direct installation from GitHub
- **Package Managers**: npm, cargo, gem, pipx, go install

### Backend Prioritization

1. Core plugins (fastest, native Rust)
2. Aqua registry (secure, verified)
3. GitHub releases (direct, simple)
4. ASDF plugins (ecosystem compatibility)
5. Package managers (language-specific)

## Configuration System

### File Hierarchy (in order of precedence)

1. `mise.toml` - Project configuration
2. `.mise.toml` - Hidden project configuration
3. `~/.config/mise/config.toml` - Global configuration
4. `/etc/mise/config.toml` - System configuration

### Configuration Formats

- **mise.toml**: Modern TOML format (recommended)
- **.tool-versions**: ASDF compatibility
- **Idiomatic files**: `.node-version`, `.python-version`, etc.

### Environment-Specific Configuration

```toml
[env]
NODE_ENV = "development"

[env.production]
NODE_ENV = "production"
DATABASE_URL = "postgres://prod-db/myapp"

[tools]
node = "20"

[tools.production]
node = "18"  # Use LTS in production
```

## Shell Integration

### Activation Hook

Add to shell configuration:

```bash
# ~/.bashrc, ~/.zshrc
eval "$(mise activate)"
```

### Shims (Optional)

For compatibility with tools expecting shims:

```bash
mise reshim
```

## Advanced Features

### 1. Lockfile Support

- `mise.lock` - Lock exact tool versions
- Reproducible builds across environments
- CI/CD integration

### 2. Task Dependencies

```toml
[tasks.ci]
depends = ["format", "lint", "test", "build"]

[tasks.format]
run = "cargo fmt"

[tasks.lint]
depends = ["format"]
run = "cargo clippy"
```

### 3. Watch Mode

```bash
# Watch files and run tasks automatically
mise watch -t test
mise watch -t build src/**/*.rs
```

### 4. Environment Templates

```toml
[env]
PROJECT_ROOT = "{{config_root}}"
VERSION = "{{exec(command='git describe --tags')}}"
BUILD_DATE = "{{date(format='%Y-%m-%d')}}"
```

## Use Cases

### 1. Individual Developer

- Manage tool versions across multiple projects
- Consistent development environment
- Simplified tool installation and updates

### 2. Team Development

- Shared tool versions via version control
- Consistent CI/CD environments
- Standardized development workflows

### 3. Multi-Project Environments

- Different tool versions per project
- Automatic environment switching
- Reduced context switching overhead

### 4. CI/CD Integration

```yaml
# GitHub Actions
- name: Setup mise
  uses: jdx/mise-action@v2
- name: Install tools
  run: mise install
- name: Run tasks
  run: mise run ci
```

## Performance Characteristics

### Speed Optimizations

- Parallel tool installations
- Efficient PATH manipulation (no shims by default)
- Cached version lookups
- Lazy loading of plugins

### Resource Usage

- Minimal runtime overhead
- Efficient disk usage with shared tool installations
- Memory-efficient tool detection

## Security Features

### Tool Verification

- Checksum verification for downloads
- GPG signature checking where supported
- Aqua registry with security scanning
- minisign verification for mise releases

### Trust System

```bash
# Trust configuration files
mise trust

# Trust specific directories
mise trust ~/projects/myapp
```

## Migration and Compatibility

### From ASDF

- Direct `.tool-versions` file support
- Plugin compatibility layer
- Migration helper: `mise migrate`

### From Other Tools

- Import from `.node-version`, `.python-version`
- Environment variable compatibility
- Existing workflow integration

This comprehensive feature set makes mise a powerful all-in-one solution for development environment management, combining the best aspects of multiple specialized tools into a cohesive, fast, and reliable system.
