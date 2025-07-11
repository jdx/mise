# mise (mise-en-place) - Development Environment Tool

## What is mise?

mise (pronounced "meez") is a development environment setup tool that manages:
1. **Dev Tools**: Version management for programming languages and tools (like asdf, nvm, pyenv, rbenv)
2. **Environments**: Environment variable management (can replace direnv)
3. **Tasks**: Task runner for project automation (can replace make, npm scripts)

mise supports hundreds of development tools and can automatically switch between different versions based on your project directory.

## Installation

### Quick Install (Linux/macOS)
```bash
curl https://mise.run | sh
```

### Other Methods
- **Homebrew**: `brew install mise`
- **Windows**: `winget install jdx.mise` or `scoop install mise`
- **Debian/Ubuntu**: Available via apt repository
- **Fedora**: Available via dnf repository

## Core Configuration

### mise.toml
The main configuration file for mise. Can be named:
- `mise.toml` (project-specific)
- `mise.local.toml` (local, not committed to git)
- `~/.config/mise/config.toml` (global)

Example `mise.toml`:
```toml
[tools]
node = "20"
python = "3.11"
terraform = "1.0.0"
go = "latest"

[env]
NODE_ENV = "development"
DATABASE_URL = "postgresql://localhost/myapp"

[tasks.test]
run = "npm test"
description = "Run tests"

[tasks.build]
run = "npm run build"
deps = ["test"]
```

### .tool-versions (asdf compatibility)
mise is compatible with asdf's `.tool-versions` format:
```
node 20.0.0
python 3.11.5
terraform 1.0.0
```

## Essential Commands

### Tool Management
- `mise use node@20` - Install and set node version (updates config)
- `mise use -g node@20` - Set global default
- `mise install` - Install all tools from config
- `mise install node@20` - Install specific version
- `mise ls` - List installed tools
- `mise ls-remote node` - List available versions

### Execution
- `mise exec node@20 -- node script.js` - Run command with specific tool version
- `mise x node@20 -- node script.js` - Shorthand for exec
- `mise run test` - Run defined task
- `mise test` - Shorthand (if no command conflict)

### Environment
- `mise activate bash` - Activate mise in shell (add to .bashrc)
- `mise activate zsh` - Activate mise in zsh (add to .zshrc)  
- `mise activate fish` - Activate mise in fish
- `mise env` - Show current environment
- `mise set KEY=value` - Set environment variable

### Information
- `mise doctor` - Check mise setup
- `mise config` - Show loaded config files
- `mise current` - Show active tool versions
- `mise which node` - Show path to tool

## Key Concepts

### Activation vs Shims vs Exec
1. **Activation** (`mise activate`): Recommended for interactive shells. Updates PATH automatically when changing directories.
2. **Shims**: Symlinks that intercept tool calls. Good for CI/CD and IDEs.
3. **Exec** (`mise exec`): Run commands with mise environment without activation.

### Tool Scopes
- `node@20.1.0` - Exact version
- `node@20` - Latest 20.x version
- `node@latest` - Latest available version
- `node@lts` - Latest LTS version
- `ref:master` - Build from git ref
- `prefix:1.19` - Latest matching prefix
- `path:/custom/path` - Use custom installation

### Configuration Hierarchy
Config files are loaded in order (later overrides earlier):
1. Global: `~/.config/mise/config.toml`
2. Parent directories: `mise.toml` files going up the tree
3. Current directory: `mise.toml`
4. Local: `mise.local.toml`

## Tasks

### TOML Tasks
```toml
[tasks.build]
run = "cargo build"
description = "Build the project"
depends = ["install"]

[tasks.test]
run = "cargo test"
depends = ["build"]
```

### File Tasks
Create executable scripts in `mise-tasks/` directory:
```bash
#!/usr/bin/env bash
#MISE description="Run tests"
cargo test
```

### Task Features
- Parallel execution by default
- Dependency management
- File watching (`mise watch`)
- Environment variable passing

## Environment Variables

### Settings
- `MISE_DATA_DIR` - Where tools are installed (`~/.local/share/mise`)
- `MISE_CONFIG_DIR` - Config directory (`~/.config/mise`)
- `MISE_CACHE_DIR` - Cache directory
- `MISE_GITHUB_TOKEN` - GitHub token for API access

### Tool-specific
- `MISE_NODE_VERSION=20` - Override node version
- `MISE_PYTHON_VERSION=3.11` - Override python version

## Common Patterns

### Project Setup
```bash
# Initialize project
cd my-project
mise use node@20 python@3.11
mise install

# Add environment variables
mise set NODE_ENV=development
mise set DATABASE_URL=postgresql://localhost/myapp
```

### Global Tools
```bash
# Set global defaults
mise use -g node@lts python@3.11 go@latest
```

### CI/CD
```bash
# Install mise and tools
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise install
mise run build
```

## Troubleshooting

### Common Issues
- **Rate limiting**: Set `MISE_GITHUB_TOKEN` for GitHub API access
- **Tool not found**: Run `mise doctor` to check setup
- **Slow performance**: Check `mise cache clear` if stale
- **Permission errors**: Ensure proper file permissions

### Debugging
- `mise doctor` - Comprehensive health check
- `mise config` - Show config file loading order
- `mise env` - Show current environment
- `mise which <tool>` - Show tool path resolution

## Plugin System

mise uses plugins to support different tools:
- Compatible with asdf plugins
- Automatic plugin installation
- Custom plugin repositories supported
- Plugin shortcuts in registry

## Migration from asdf

mise is largely compatible with asdf:
- Reads `.tool-versions` files
- Uses asdf plugins
- Similar commands (with improvements)
- Can run alongside asdf temporarily

## Best Practices

1. Use `mise.toml` for committed project configuration
2. Use `mise.local.toml` for local overrides
3. Activate mise in shell for interactive use
4. Use shims for CI/CD and IDE integration
5. Set `MISE_GITHUB_TOKEN` to avoid rate limits
6. Use `mise doctor` regularly to check setup
7. Pin versions in production environments
8. Use tasks for project automation

## Advanced Features

### Tool Options
```toml
[tools]
node = { version = "20", postinstall = "corepack enable" }
python = { version = "3.11", virtualenv = "myproject" }
```

### Hooks
```toml
[hooks]
enter = "mise install --quiet"
leave = "deactivate"
```

### Multiple Versions
```toml
[tools]
python = ["3.10", "3.11", "3.12"]  # Install multiple versions
```

## Development & Contributing

### Setting Up Development Environment

mise is written in Rust and uses itself for development tasks. To contribute to mise:

#### Prerequisites
- Rust (latest stable)
- Node.js
- `pipx` or `uv` 
- Bash (newer than macOS default)

#### Getting Started
```bash
# Clone the repository
git clone https://github.com/jdx/mise.git
cd mise

# Install dependencies and build
mise run build

# Run sanity check
mise run build
```

#### Development Setup
Create a development shim to easily run mise during development:

```bash
# Create ~/.local/bin/@mise
#!/bin/sh
exec cargo run -q --all-features --manifest-path ~/src/mise/Cargo.toml -- "$@"
```

Then use `@mise` to run the development version:
```bash
@mise --help
eval "$(@mise activate zsh)"
```

### Testing

mise has a comprehensive test suite with multiple types of tests to ensure reliability and functionality across different platforms and scenarios.

#### Unit Tests
Unit tests are fast, focused tests for individual components and functions:

```bash
# Run all unit tests
cargo test --all-features

# Run specific unit tests
cargo test <test_name>

# Run tests with verbose output
cargo test --all-features -- --nocapture

# Run tests in series (not parallel)
RUST_TEST_THREADS=1 cargo test --all-features
```

**Unit test structure:**
- Located in `src/` directory alongside source code
- Use Rust's built-in test framework
- Test individual functions and modules
- Fast execution (used for quick feedback during development)

#### E2E Tests
End-to-end tests validate the complete functionality of mise in realistic scenarios:

```bash
# Run all E2E tests
mise run test:e2e

# Run specific E2E test
./e2e/run_test test_name

# Run E2E tests matching pattern
./e2e/run_test task  # runs tests matching *task*

# Run all tests including slow ones
TEST_ALL=1 mise run test:e2e
```

**E2E test structure:**
- Located in `e2e/` directory
- Organized by functionality:
  - `e2e/cli/` - Command-line interface tests
  - `e2e/core/` - Core functionality tests
  - `e2e/env/` - Environment variable tests
  - `e2e/tasks/` - Task runner tests
  - `e2e/config/` - Configuration tests
  - `e2e/tools/` - Tool management tests
  - `e2e/shell/` - Shell integration tests
  - `e2e/backend/` - Backend tests
  - `e2e/plugins/` - Plugin tests

**E2E test categories:**
- **Fast tests** (`test_*`): Run in normal test suites
- **Slow tests** (`test_*_slow`): Only run when `TEST_ALL=1` is set
- **Isolated environment**: Each test runs in a clean, isolated environment

#### Coverage Tests
Coverage tests measure how much of the codebase is covered by tests:

```bash
# Run coverage tests
mise run test:coverage

# Coverage tests run in parallel tranches for CI
TEST_TRANCHE=0 TEST_TRANCHE_COUNT=8 mise run test:coverage
```

#### Windows E2E Tests
Windows has its own test suite written in PowerShell:

```powershell
# Run all Windows E2E tests
pwsh e2e-win\run.ps1

# Run specific Windows tests
pwsh e2e-win\run.ps1 task  # run tests matching *task*
```

#### Plugin Tests
Test plugin functionality across different backends:

```bash
# Test specific plugin
mise test-tool ripgrep

# Test all plugins in registry
mise test-tool --all

# Test all plugins in config files
mise test-tool --all-config

# Test with parallel jobs
mise test-tool --all --jobs 4
```

#### Test Environment Setup
Tests run in isolated environments to avoid conflicts:

```bash
# Disable mise during development testing
export MISE_DISABLE_TOOLS=1

# Run tests with specific environment
MISE_TRUSTED_CONFIG_PATHS=$PWD cargo test
```

#### Test Assertions
The E2E tests use a custom assertion framework (`e2e/assert.sh`):

```bash
# Basic assertions
assert "command" "expected_output"
assert_contains "command" "substring"
assert_fail "command" "expected_error"

# JSON assertions
assert_json "command" '{"key": "value"}'
assert_json_partial_array "command" "fields" '[{...}]'

# File/directory assertions
assert_directory_exists "/path/to/dir"
assert_directory_not_exists "/path/to/dir"
assert_empty "command"
```

#### Running Specific Test Categories

```bash
# Run all tests (unit + e2e)
mise run test

# Run only unit tests
mise run test:unit

# Run only e2e tests
mise run test:e2e

# Run tests with shuffle (for detecting order dependencies)
mise run test:shuffle

# Run nightly tests (with bleeding edge Rust)
rustup default nightly && mise run test
```

#### Running Single Tests

**Unit Tests:**
```bash
# Run a specific unit test by name
cargo test test_name

# Run tests matching a pattern
cargo test pattern

# Run tests in a specific module
cargo test module_name

# Run a single test with output
cargo test test_name -- --nocapture

# Run a single test file (if organized by file)
cargo test --test test_file_name
```

**E2E Tests:**
```bash
# Run a specific E2E test by name
./e2e/run_test test_name

# Run E2E tests matching a pattern
mise run test:e2e pattern

# Examples:
./e2e/run_test test_use                    # Run specific test
./e2e/run_test test_config_set            # Run config-related test
mise run test:e2e task                     # Run all tests matching "task"
```

**Plugin Tests:**
```bash
# Test a specific plugin
mise test-tool ripgrep

# Test a plugin with verbose output
mise test-tool ripgrep --raw

# Test multiple plugins
mise test-tool ripgrep jq terraform
```

**Windows Tests:**
```powershell
# Run a specific Windows test
pwsh e2e-win\run.ps1 -TestName "test_name"

# Run tests matching a pattern
pwsh e2e-win\run.ps1 task
```

#### Test Development Tips

1. **Test naming**: Use descriptive names that explain what's being tested
2. **Test isolation**: Each test should be independent and clean up after itself
3. **Use slow tests sparingly**: Mark long-running tests as `_slow` to avoid CI timeouts
4. **Mock external dependencies**: Use dummy plugins and mock data when possible
5. **Test edge cases**: Include tests for error conditions and boundary cases

#### Performance Testing
```bash
# Run performance benchmarks
mise run test:perf

# Build performance test workspace
mise run test:build-perf-workspace
```

#### Snapshot Testing
Used for testing output consistency:

```bash
# Update test snapshots when output changes
mise run snapshots

# Use cargo-insta for snapshot testing
cargo insta test --accept --unreferenced delete
```

### Available Development Tasks

Use `mise tasks` to see all available development tasks:

#### Common Tasks
- `mise run build` - Build the project
- `mise run test` - Run all tests (unit + E2E)
- `mise run test:unit` - Run unit tests only
- `mise run test:e2e` - Run E2E tests only
- `mise run lint` - Run linting
- `mise run lint:fix` - Run linting with fixes
- `mise run format` - Format code
- `mise run clean` - Clean build artifacts
- `mise run snapshots` - Update test snapshots
- `mise run render` - Generate documentation and completions

#### Documentation Tasks
- `mise run docs` - Start documentation development server
- `mise run docs:build` - Build documentation
- `mise run render:help` - Generate help documentation
- `mise run render:completions` - Generate shell completions

#### Release Tasks
- `mise run release` - Create a release
- `mise run ci` - Run CI tasks (format, build, test)

### Project Structure

```
mise/
├── src/           # Main Rust source code
├── e2e/           # End-to-end tests
├── docs/          # Documentation
├── tasks.toml     # Development tasks
├── mise.toml      # Project configuration
├── Cargo.toml     # Rust project configuration
└── xtasks/        # Additional build scripts
```

### Pre-commit Hooks & Code Quality

mise uses multiple approaches for ensuring code quality and running pre-commit hooks:

#### hk (Modern Hook Manager)
mise uses [hk](https://hk.jdx.dev) (pronounced "hook") as its primary hook manager for linting and code quality checks. hk is a modern alternative to lefthook written by the same author as mise.

**hk Configuration:**
The project uses `hk.pkl` (written in the Pkl configuration language) to define linting rules:

```bash
# Run all linting checks
hk check --all

# Run linting with fixes
hk fix --all

# Run specific linter
hk check --linter shellcheck
```

**Available Linters in hk:**
- **prettier**: Code formatting for multiple languages
- **clippy**: Rust linting with `cargo clippy`
- **shellcheck**: Shell script linting
- **shfmt**: Shell script formatting
- **pkl**: Pkl configuration file validation

**Using hk in Development:**
```bash
# Install hk via mise
mise use hk

# Run linting (used in CI and pre-commit)
mise run lint  # This runs hk check --all

# Run linting with fixes
hk fix --all

# Check specific file types
hk check --linter prettier
hk check --linter shellcheck
```

#### Traditional Pre-commit Hooks (Alternative)
mise also supports traditional pre-commit hooks through multiple methods:

**Option 1: Using lefthook (Legacy)**
```bash
# Install lefthook
mise use lefthook

# Install the git hooks
lefthook install

# Run pre-commit manually
lefthook run pre-commit
```

**Option 2: Using standard pre-commit framework**
```bash
# Install pre-commit
mise use pre-commit

# Install the git hooks
pre-commit install

# Run pre-commit manually
pre-commit run --all-files
```

**Option 3: Using mise's built-in git hook generation**
```bash
# Generate a git pre-commit hook that runs mise tasks
mise generate git-pre-commit --write --task=pre-commit

# This creates .git/hooks/pre-commit that runs:
# mise run pre-commit
```

#### Pre-commit Task Configuration
mise defines a `pre-commit` task that runs the main linting checks:

```toml
[pre-commit]
env = { PRE_COMMIT = 1 }
run = ["mise run lint"]
```

This task:
1. Sets `PRE_COMMIT=1` environment variable
2. Runs `mise run lint`, which executes `hk check --all`

#### Setting Up Pre-commit Hooks
**Recommended approach using hk:**
```bash
# Install hk
mise use hk

# Set up git hook to run mise's pre-commit task
mise generate git-pre-commit --write --task=pre-commit

# Or manually create .git/hooks/pre-commit:
cat > .git/hooks/pre-commit << 'EOF'
#!/bin/sh
exec mise run pre-commit
EOF
chmod +x .git/hooks/pre-commit
```

**Alternative approach using standard pre-commit:**
```bash
# Install pre-commit
mise use pre-commit

# Install hooks defined in .pre-commit-config.yaml
pre-commit install
```

#### Running Pre-commit Checks Manually
```bash
# Run all pre-commit checks
mise run pre-commit

# Run specific linting checks
mise run lint

# Run linting with fixes
hk fix --all

# Run checks on specific files
hk check --files="src/**/*.rs"
```

#### Environment Variables for Hooks
When hooks run, they have access to these environment variables:
- `PRE_COMMIT=1`: Indicates the code is running in pre-commit context
- `STAGED`: Git staged files (when using mise's git hook generation)
- `MISE_PRE_COMMIT=1`: Set by mise's generated git hooks

### Testing Configuration

- **Unit tests**: Fast tests for individual components
- **E2E tests**: Integration tests that test the full application
- **Slow tests**: Only run with `TEST_ALL=1` environment variable
- **Windows tests**: Separate PowerShell-based test suite

### Development Tips

1. **Disable mise during development**: If you use mise in your shell, disable it when running tests to avoid conflicts
2. **Use dev container**: Docker setup available (currently needs fixing)
3. **Test specific features**: Use `cargo test <test_name>` for targeted testing
4. **Update snapshots**: Use `mise run snapshots` when changing test outputs
5. **Rate limiting**: Set `MISE_GITHUB_TOKEN` to avoid GitHub API rate limits during development

### Dependency Management

mise uses several tools to validate dependencies and code quality:

- **cargo-deny**: Validates licenses, security advisories, and dependency duplicates
- **cargo-msrv**: Verifies minimum supported Rust version compatibility
- **cargo-machete**: Detects unused dependencies in Cargo.toml

These checks run automatically in CI and can be run locally:
```bash
# Install tools
mise use cargo-deny cargo-msrv cargo-machete

# Run checks
cargo deny check
cargo msrv verify
cargo machete --with-metadata
```

### Contributing Guidelines

1. **Before starting**: File an issue or discuss in Discord for non-obvious changes
2. **Look for issues**: Check "help wanted" and "good first issue" labels
3. **Test thoroughly**: Ensure both unit and E2E tests pass
4. **Follow conventions**: Use existing code style and patterns
5. **Update documentation**: Add/update docs for new features

#### Pull Request Workflow
1. **PR titles**: Must follow conventional commit format (validated automatically)
2. **Auto-formatting**: Code will be automatically formatted by autofix.ci
3. **CI checks**: All tests must pass across Linux, macOS, and Windows
4. **Coverage**: New code should maintain or improve test coverage
5. **Dependencies**: New dependencies are validated with cargo-deny

#### GitHub Actions Development
If modifying GitHub Actions workflows:
- Use `actionlint` to validate workflow files: `mise run lint`
- Self-hosted runners are configured for performance testing
- Configuration variables are allowed (see `.github/actionlint.yaml`)

### Conventional Commits

mise uses [Conventional Commits](https://www.conventionalcommits.org/) for consistent commit messages and automated changelog generation. All commits should follow this format:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

#### Commit Types

- **feat**: New features (🚀 Features)
- **fix**: Bug fixes (🐛 Bug Fixes)
- **refactor**: Code refactoring (🚜 Refactor)
- **doc**: Documentation changes (📚 Documentation)
- **style**: Code style changes (🎨 Styling)
- **perf**: Performance improvements (⚡ Performance)
- **test**: Testing changes (🧪 Testing)
- **chore**: Maintenance tasks, dependency updates
- **revert**: Reverting previous changes (◀️ Revert)

#### Examples

```bash
feat(cli): add new command for listing plugins
fix(parser): handle edge case in version parsing
refactor(config): simplify configuration loading logic
doc(readme): update installation instructions
test(e2e): add tests for new plugin functionality
chore(deps): update dependencies to latest versions
```

#### Scopes

Common scopes used in mise:
- `cli` - Command line interface changes
- `config` - Configuration system changes
- `parser` - Parsing logic changes
- `deps` - Dependency updates
- `security` - Security-related changes

#### Breaking Changes

For breaking changes, add `!` after the type or include `BREAKING CHANGE:` in the footer:

```bash
feat(api)!: remove deprecated configuration options
# OR
feat(api): remove deprecated configuration options

BREAKING CHANGE: The old configuration format is no longer supported
```

### Packaging Testing

For testing package installation:

#### Ubuntu/Debian
```bash
# Test apt installation
docker run -ti --rm ubuntu
apt update -y && apt install -y gpg sudo wget curl
# ... installation steps
```

#### Fedora
```bash
# Test dnf installation  
docker run -ti --rm fedora
dnf install -y dnf-plugins-core
# ... installation steps
```

### CI/CD & Pull Request Automation

mise uses several automated workflows to maintain code quality and streamline development:

#### Automated Code Formatting
- **autofix.ci**: Automatically formats code and fixes linting issues in PRs
- Runs `mise run render` and `mise run lint-fix` automatically
- Commits fixes directly to the PR branch

#### PR Title Validation
- **semantic-pr-lint**: Validates PR titles follow conventional commit format
- PR titles must match: `<type>[optional scope]: <description>`
- Example: `feat(cli): add new command for listing plugins`

#### Continuous Integration
- **Cross-platform testing**: Ubuntu, macOS, and Windows
- **Unit tests**: Fast component-level tests
- **E2E tests**: Full integration testing with multiple test tranches
- **Coverage testing**: Code coverage across 8 parallel test runs
- **Dependency validation**: `cargo deny`, `cargo msrv`, `cargo machete`

#### Release Automation
- **release-plz**: Automated release management based on conventional commits
- Automatically creates release PRs and publishes releases
- Runs daily via scheduled workflow
- Handles version bumping and changelog generation

## mise Codebase Architecture

### Overview

mise is a Rust-based tool with a modular architecture designed around three main concepts:
1. **Tool Version Management** - Managing different versions of development tools
2. **Environment Management** - Setting up environment variables and project contexts
3. **Task Running** - Executing project tasks with dependency management

### Core Architecture Components

#### 1. **Main Entry Point** (`src/main.rs`)
- Asynchronous Tokio runtime with multi-threading
- Global error handling and panic hooks
- CLI argument parsing and routing
- Multi-progress reporting system

#### 2. **CLI Layer** (`src/cli/`)
- **Command Structure**: Each command is a separate module (e.g., `install.rs`, `use.rs`, `run.rs`)
- **Argument Parsing**: Uses `clap` for command-line argument handling
- **Command Execution**: Async command execution with proper error handling
- **Key Commands**:
  - `install` - Install tool versions
  - `use` - Add tools to configuration
  - `run` - Execute tasks
  - `env` - Environment management
  - `shell` - Shell integration

#### 3. **Backend System** (`src/backend/`)
The backend system is the core abstraction for tool management:

**Backend Trait**: Common interface for all tool backends
```rust
pub trait Backend: Debug + Send + Sync {
    async fn list_remote_versions(&self) -> Result<Vec<String>>;
    async fn install_version(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()>;
    async fn uninstall_version(&self, tv: &ToolVersion) -> Result<()>;
    // ... other methods
}
```

**Backend Types**:
- **Core Backends**: Built-in support (Node.js, Python, etc.)
- **ASDF Backends**: External plugin system compatibility
- **Aqua Backends**: Package manager integration
- **Language-specific**: npm, cargo, pipx, gem, go, etc.
- **Universal**: ubi (GitHub releases)

#### 4. **Configuration System** (`src/config/`)
**Hierarchical Configuration**:
- `Config` struct manages all configuration sources
- `ConfigFile` trait for different config formats:
  - `MiseToml` - Main configuration format
  - `ToolVersions` - ASDF compatibility
  - `IdiomaticVersion` - Language-specific files (`.nvmrc`, `.python-version`)

**Configuration Hierarchy**:
1. System config (`/etc/mise/config.toml`)
2. Global config (`~/.config/mise/config.toml`)
3. Project configs (searched up directory tree)
4. Environment-specific configs (`mise.dev.toml`)

#### 5. **Toolset Management** (`src/toolset/`)
**Core Components**:
- `Toolset` - Collection of tools for a specific context
- `ToolVersion` - Represents a specific version of a tool
- `ToolRequest` - User request for a tool (e.g., `node@18`)
- `ToolsetBuilder` - Constructs toolsets from configuration

**Tool Resolution Flow**:
1. Parse configuration files
2. Resolve version specifications
3. Build toolset with dependencies
4. Install missing tools
5. Set up environment

#### 6. **Task System** (`src/task/`)
**Task Components**:
- `Task` struct with metadata, dependencies, and execution info
- `Deps` - Dependency graph management using petgraph
- `TaskFileProvider` - Discovers tasks from files and configuration
- Parallel execution with dependency resolution

**Task Types**:
- **TOML Tasks**: Defined in `mise.toml`
- **File Tasks**: Executable scripts in task directories
- **Command Tasks**: Simple command execution
- **Script Tasks**: Inline script execution

#### 7. **Plugin System** (`src/plugins/`)
**Plugin Architecture**:
- `Plugin` trait for extensibility
- `AsdfPlugin` - ASDF plugin compatibility
- `VfoxPlugin` - Vfox plugin system
- `ScriptManager` - Manages plugin scripts

**Plugin Types**:
- **Core Plugins**: Built into mise
- **External Plugins**: Downloaded from repositories
- **Local Plugins**: Custom local implementations

#### 8. **Shell Integration** (`src/shell/`)
**Shell Support**:
- Bash, Zsh, Fish, PowerShell support
- `Shell` trait for shell-specific behavior
- Activation scripts for environment setup
- Hook system for directory changes

#### 9. **Environment Management** (`src/env*.rs`)
**Environment Components**:
- `EnvDiff` - Tracks environment changes
- `EnvDirective` - Configuration for environment variables
- `PathEnv` - PATH management
- Context-aware environment resolution

#### 10. **Caching System** (`src/cache.rs`)
**Cache Management**:
- `CacheManager` - Generic caching with TTL
- File-based caching for versions, metadata
- Automatic cache invalidation
- Per-backend cache isolation

### Key Design Patterns

#### 1. **Trait-Based Architecture**
- `Backend` trait for tool management
- `ConfigFile` trait for configuration formats
- `Shell` trait for shell integration
- `Plugin` trait for extensibility

#### 2. **Async/Await Throughout**
- Tokio runtime for async operations
- Parallel tool installations
- Concurrent task execution
- Non-blocking I/O operations

#### 3. **Error Handling**
- `eyre` crate for error management
- Structured error types
- Context-aware error reporting
- Graceful fallback behavior

#### 4. **Configuration Hierarchy**
- Multiple configuration sources
- Inheritance and overrides
- Environment-specific configurations
- Validation and trust system

#### 5. **Dependency Management**
- Tool dependencies (e.g., Node.js for npm tools)
- Task dependencies with DAG resolution
- Parallel execution with proper ordering
- Circular dependency detection

### Data Flow

1. **Initialization**: Load configuration hierarchy
2. **Tool Discovery**: Find available backends and versions
3. **Resolution**: Resolve tool requests to specific versions
4. **Installation**: Install missing tools in dependency order
5. **Environment Setup**: Configure PATH and environment variables
6. **Execution**: Run tasks or provide shell integration

### Testing Architecture

**Test Structure**:
- **Unit Tests**: Component-level testing with mocks
- **Integration Tests**: End-to-end CLI testing
- **E2E Tests**: Real-world scenario testing
- **Snapshot Tests**: Output comparison testing

**Test Organization**:
- `src/` - Unit tests alongside source code
- `e2e/` - End-to-end test scripts
- `test/` - Test fixtures and utilities
- `src/snapshots/` - Snapshot test data

This architecture provides a flexible, extensible foundation for managing development environments while maintaining compatibility with existing tools and workflows.

This covers the essential information about mise for LLMs to help users effectively use the tool for development environment management and contribute to its development.