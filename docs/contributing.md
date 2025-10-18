---
outline: [1, 3]
---

# Contributing

Before submitting a PR, unless it's something obvious, consider creating a
[discussion](https://github.com/jdx/mise/discussions)
or simply mention what you plan to do in the
[Discord](https://discord.gg/UBa7pJUN7Z).
PRs are often either rejected or need to change significantly after submission
so make sure before you start working on something it won't be a wasted effort.

## Contributing Guidelines

1. **Before starting**: Create a discussion or discuss in Discord for non-obvious changes
2. **Test thoroughly**: Ensure both unit and E2E tests pass
3. **Follow conventions**: Use existing code style and patterns
4. **Update documentation**: Add/update docs for new features

### Pull Request Workflow

1. **PR titles**: Must follow conventional commit format (validated
   automatically)
   - For new tools in registry: Use `registry: add tool-name`
2. **Auto-formatting**: Code will be automatically formatted by autofix.ci
3. **CI checks**: All tests must pass across Linux, macOS, and Windows
4. **Coverage**: New code should maintain or improve test coverage
5. **Dependencies**: New dependencies are validated with cargo-deny

### Development Tips

1. **Disable mise during development**: If you use mise in your shell, disable
   it when running tests to avoid conflicts
2. **Use dev container**: Docker setup available (currently needs fixing)
3. **Test specific features**: Use `cargo test test_name` for targeted testing
4. **Update snapshots**: Use `mise run snapshots` when changing test outputs
5. **Rate limiting**: Set `MISE_GITHUB_TOKEN` to avoid GitHub API rate limits
   during development

## Packaging and Self-Update Instructions

When mise is installed via a package manager, in-app self-update is disabled and users should update via their package manager. Packaging should install a TOML file with platform-specific instructions at `lib/mise-self-update-instructions.toml` (or `lib/mise/mise-self-update-instructions.toml`). Example contents:

```toml
# Debian/Ubuntu (APT)
message = "To update mise from the APT repository, run:\n\n  sudo apt update && sudo apt install --only-upgrade mise\n"
```

```toml
# Fedora/CentOS Stream (DNF)
message = "To update mise from COPR, run:\n\n  sudo dnf upgrade mise\n"
```

## Testing

mise has a comprehensive test suite with multiple types of tests to ensure
reliability and functionality across different platforms and scenarios.

### Unit Tests

Unit tests are fast, focused tests for individual components and functions:

```bash
# Run all unit tests
cargo test --all-features

# Run specific unit tests
cargo test <test_name>
```

**Unit test structure:**

- Located in `src/` directory alongside source code
- Use Rust's built-in test framework
- Test individual functions and modules
- Fast execution (used for quick feedback during development)

### E2E Tests

End-to-end tests validate the complete functionality of mise in realistic
scenarios:

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

### Coverage Tests

Coverage tests measure how much of the codebase is covered by tests:

```bash
# Run coverage tests
mise run test:coverage

# Coverage tests run in parallel tranches for CI
TEST_TRANCHE=0 TEST_TRANCHE_COUNT=8 mise run test:coverage
```

### Windows E2E Tests

Windows has its own test suite written in PowerShell:

```powershell
# Run all Windows E2E tests
pwsh e2e-win\run.ps1

# Run specific Windows tests
pwsh e2e-win\run.ps1 task  # run tests matching *task*
```

### Plugin Tests

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

### Test Environment Setup

Tests run in isolated environments to avoid conflicts:

```bash
# Disable mise during development testing
export MISE_DISABLE_TOOLS=1

# Run tests with specific environment
MISE_TRUSTED_CONFIG_PATHS=$PWD cargo test
```

### Test Assertions

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

### Running Specific Test Categories

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

### Running Individual Tests

#### Running Single Unit Tests

```bash
# Run a specific unit test by name
cargo test test_name

# Run tests matching a pattern
cargo test pattern

# Run tests in a specific module
cargo test module_name

# Run a single test with output
cargo test test_name -- --nocapture
```

#### Running Single E2E Tests

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

#### Testing Individual Plugins

```bash
# Test a specific plugin
mise test-tool ripgrep

# Test a plugin with verbose output
mise test-tool ripgrep --raw

# Test multiple plugins
mise test-tool ripgrep jq terraform
```

### Performance Testing

```bash
# Run performance benchmarks
mise run test:perf

# Build performance test workspace
mise run test:build-perf-workspace
```

### Snapshot Testing

Used for testing output consistency:

```bash
# Update test snapshots when output changes
mise run snapshots

# Use cargo-insta for snapshot testing
cargo insta test --accept --unreferenced delete
```

## Development Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/) (latest stable, we don't use mise to
  manage rust)
- mise

### Getting Started

```bash
# Clone the repository
git clone https://github.com/jdx/mise.git
cd mise

# Install dependencies
mise install

# Build the project
mise run build
```

### Development Shim

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

## Project Structure

```text
mise/
├── src/           # Main Rust source code
├── e2e/           # End-to-end tests
├── docs/          # Documentation
├── tasks.toml     # Development tasks
├── mise.toml      # Project configuration
├── Cargo.toml     # Rust project configuration
└── xtasks/        # Additional build scripts
```

## Available Development Tasks

Use `mise tasks` to see all available development tasks:

### Common Tasks

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

### Documentation Tasks

- `mise run docs` - Start documentation development server
- `mise run docs:build` - Build documentation
- `mise run render:help` - Generate help documentation
- `mise run render:completions` - Generate shell completions

### Release Tasks

- `mise run release` - Create a release
- `mise run ci` - Run CI tasks (format, build, test)

## Setup

Shouldn't require anything special I'm aware of, but `mise run build` is a good
sanity check to run and make sure it's all working.

## Dev Container

::: danger
The docker setup quit working and since I don't use it I haven't bothered to
fix it. For now you'll need to run outside of docker or you can try to fix the
docker setup.
:::

There is a docker setup that makes development with mise easier. It is
especially helpful for running the E2E tests.
Here's some example ways to use it:

```sh
mise run docker:cargo build
mise run docker:cargo test
mise run docker:mise --help # run `mise --help` in the dev container
# run the e2e tests inside of the docker container
mise run docker:mise run test:e2e
# shortcut for `mise run docker:mise run test:e2e`
mise run docker:e2e
```

## Pre-commit Hooks & Code Quality

mise uses [hk](https://hk.jdx.dev) as its git hook manager for
linting and code quality checks. hk is a modern alternative to lefthook written
by the same author as mise.

### hk Configuration

The project uses `hk.pkl` (written in the Pkl configuration language) to define
linting rules:

```bash
# Run all linting checks
hk check --all

# Run linting with fixes
hk fix --all

# Run specific linter
hk check --linter shellcheck
```

### Available Linters in hk

- **prettier**: Code formatting for multiple languages
- **clippy**: Rust linting with `cargo clippy`
- **shellcheck**: Shell script linting
- **shfmt**: Shell script formatting
- **pkl**: Pkl configuration file validation

### Using hk in Development

```bash
# Run linting (used in CI and pre-commit)
mise run lint  # This runs hk check --all

# Run linting with fixes
hk fix --all

# Check specific file types
hk check --linter prettier
hk check --linter shellcheck
```

### Pre-commit Task Configuration

mise defines a `pre-commit` task that runs the main linting checks:

```toml
[pre-commit]
env = { PRE_COMMIT = 1 }
run = ["mise run lint"]
```

This task:

1. Sets `PRE_COMMIT=1` environment variable
2. Runs `mise run lint`, which executes `hk check --all`

### Setting Up Pre-commit Hooks

```bash
# Set up git hook to run mise's pre-commit task
mise generate git-pre-commit --write --task=pre-commit
```

### Running Pre-commit Checks Manually

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

## Running the CLI

Even if using the devcontainer, it's a good idea to create a shim to make it
easy to launch mise. I use the following shim in `~/.local/bin/@mise`:

```sh
#!/bin/sh
exec cargo run -q --all-features --manifest-path ~/src/mise/Cargo.toml -- "$@"
```

::: info
Don't forget to change the manifest path to the correct path for your setup.
:::

Then if that is in PATH just use `@mise` to run mise by compiling it on the fly.

```sh
@mise --help
@mise run docker:e2e
eval "$(@mise activate zsh)"
@mise activate fish | source
```

## Releasing

Run `mise run release -x [minor|patch]`. (minor if it is the first release in a
month)

## Linting

- Lint codebase: `mise run lint`
- Lint and fix codebase: `mise run lint:fix`

## Generating readme and shell completion files

```sh
mise run render
```

## Dependency Management

mise uses several tools to validate dependencies and code quality:

- **cargo-deny**: Validates licenses, security advisories, and dependency
  duplicates
- **cargo-msrv**: Verifies minimum supported Rust version compatibility
- **cargo-machete**: Detects unused dependencies in Cargo.toml

These checks run automatically in CI and can be run locally:

```bash
# Run checks (tools are automatically available via mise.toml)
cargo deny check
cargo msrv verify
cargo machete --with-metadata
```

## Conventional Commits

mise uses [Conventional Commits](https://www.conventionalcommits.org/) for
consistent commit messages and automated changelog generation. All commits
should follow this format:

```text
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

### Commit Types

- **feat**: New features (🚀 Features)
- **fix**: Bug fixes (🐛 Bug Fixes)
- **refactor**: Code refactoring (🚜 Refactor)
- **docs**: Documentation changes (📚 Documentation)
- **style**: Code style changes (🎨 Styling)
- **perf**: Performance improvements (⚡ Performance)
- **test**: Testing changes (🧪 Testing)
- **chore**: Maintenance tasks, dependency updates
- **revert**: Reverting previous changes (◀️ Revert)

### Examples

```bash
feat(cli): add new command for listing plugins
fix(parser): handle edge case in version parsing
refactor(config): simplify configuration loading logic
docs(readme): update installation instructions
test(e2e): add tests for new plugin functionality
chore(deps): update dependencies to latest versions
```

### Scopes

Common scopes used in mise:

- `cli` - Command line interface changes
- `config` - Configuration system changes
- `parser` - Parsing logic changes
- `deps` - Dependency updates
- `security` - Security-related changes

### Breaking Changes

#### Breaking Change Policy

Breaking changes are rarely accepted into mise and are only performed in
exceptional situations where there is no better alternative. When a breaking
change is necessary, the process includes:

1. **CLI warnings**: Users receive deprecation warnings in the CLI
2. **Migration period**: Several months are provided for users to migrate
3. **Documentation**: Clear migration guides are provided
4. **Community notice**: Announcements in Discord and GitHub discussions

For breaking changes, add `!` after the type or include `BREAKING CHANGE:` in
the footer:

```bash
feat(api)!: remove deprecated configuration options
# OR
feat(api): remove deprecated configuration options

BREAKING CHANGE: The old configuration format is no longer supported
```

## CI/CD & Pull Request Automation

mise uses several automated workflows to maintain code quality and streamline
development:

### Automated Code Formatting

- **autofix.ci**: Automatically formats code and fixes linting issues in PRs
- Runs `mise run render` and `mise run lint-fix` automatically
- Commits fixes directly to the PR branch

### PR Title Validation

- **semantic-pr-lint**: Validates PR titles follow conventional commit format
- PR titles must match: `<type>[optional scope]: <description>`
- Example: `feat(cli): add new command for listing plugins`

### Continuous Integration

- **Cross-platform testing**: Ubuntu, macOS, and Windows
- **Unit tests**: Fast component-level tests
- **E2E tests**: Full integration testing with multiple test tranches
- **Dependency validation**: `cargo deny`, `cargo msrv`, `cargo machete`

### Release Automation

- **release-plz**: Automated release management based on conventional commits
- Automatically creates release PRs and publishes releases
- Runs daily via scheduled workflow
- Handles version bumping and changelog generation

## Adding a new setting

To add a new setting, add it to
[`settings.toml`](https://github.com/jdx/mise/blob/main/settings.toml) in the
root of the project and run `mise run render` to update the codebase.

## Adding Tools

Adding tools to mise involves adding entries to the
[registry.toml](https://github.com/jdx/mise/blob/main/registry.toml) file. This
allows users to install tools using short names like `mise use ripgrep` instead
of the full backend specification.

### Quick Start

1. **Choose the right backend** for your tool:

   - **[aqua](dev-tools/backends/aqua.md)** - Preferred for GitHub releases with security
     features
   - **[ubi](dev-tools/backends/ubi.md)** - Simple GitHub/GitLab releases following
     standard conventions
   - **Language package managers** - `npm`, `pipx`, `cargo`, `gem`, etc. for
     ecosystem-specific tools
   - **[Core tools](core-tools.md)** - Built-in support for major languages
     (not user-contributed)

2. **Add to registry.toml**:

   ```toml
   your-tool.description = "Brief description of the tool"
   your-tool.backends = ["aqua:owner/repo", "ubi:owner/repo"]
   your-tool.test = ["your-tool --version", "{{version}}"]
   ```

3. **Test the tool** works properly with `mise test-tool your-tool`

### Guidelines and Requirements

When adding a new tool, the following requirements apply (automatically
enforced by [GitHub Actions workflow](https://github.com/jdx/mise/blob/main/.github/workflows/registry_comment.yml)):

- **New asdf plugins are not accepted** - Use aqua/ubi instead
- **Tools may be rejected if they are not notable** - The tool should be
  reasonably popular and well-maintained
- **A test is required in `registry.toml`** - Must include a `test` field to
  verify installation

### Registry Format

The `registry.toml` file uses this format:

```toml
# Tool name (becomes the short name for `mise use`)
your-tool.description = "Tool description"
your-tool.backends = [
    "aqua:owner/repo",           # Preferred backend first
    "ubi:owner/repo",            # Fallback backends
    "npm:package-name"           # Multiple backends supported
]
your-tool.test = [
    "your-tool --version",       # Command to run
    "{{version}}"                # Expected output pattern
]
your-tool.aliases = ["alt-name"] # Optional alternative names
your-tool.os = ["linux", "macos"] # Optional OS restrictions
```

### Backend Priority

List backends in order of preference. Users will get the first available
backend, but can override with explicit syntax like `mise use aqua:owner/repo`.

### Tool Testing

All tools must include a test to verify proper installation:

```toml
your-tool.test = [
    "command-to-run",
    "expected-output-pattern"
]
```

The test command should be reliable and the output pattern should use
`{{version}}` to match any version number.

### Registry Examples

Recent tool additions:

- **DuckDB**: Simple ubi backend ([#4248](https://github.com/jdx/mise/pull/4248))

  ```toml
  duckdb.backends = ["ubi:duckdb/duckdb"]
  duckdb.test = ["duckdb --version", "{{version}}"]
  ```

- **Biome**: Multiple backends ([#4283](https://github.com/jdx/mise/pull/4283))

  ```toml
  biome.backends = ["aqua:biomejs/biome", "ubi:biomejs/biome"]
  biome.test = ["biome --version", "Version: {{version}}"]
  ```

## Adding Backends

:::warning Backend vs Tool Confusion
**Most contributors want to add tools, not backends.** Before reading this
section, make sure you actually need a new backend. Tools are individual
software packages (like `node` or `ripgrep`), while backends are installation
mechanisms (like `aqua` or `ubi`). If you want to add a specific tool to mise,
see [Adding Tools](#adding-tools) instead.
:::

:::warning Core Backend Acceptance Policy
**New backends are unlikely to be accepted into mise core.** They require
a lot of maintenance so it's generally better to use the [backend plugin system](backend-plugin-development.md) to add backends without core changes. A new backend would only be accepted for a major package manager
or tool that would greatly enhance mise's capabilities.

If you need a custom backend:

1. **Discuss with jdx first** in [Discord](https://discord.gg/UBa7pJUN7Z) or by
   creating a [discussion](https://github.com/jdx/mise/discussions)
2. **Consider if existing backends** (ubi, aqua, npm, pipx, etc.) can meet your
   needs
3. **Create a plugin** - use the [plugin system](tool-plugin-development.md) to create plugins for private/custom tools without core changes. Start with the [mise-tool-plugin-template](https://github.com/jdx/mise-tool-plugin-template) for a quick setup

Most tool installation needs can be met by existing backends, especially
[ubi](dev-tools/backends/ubi.md) for GitHub releases and
[aqua](dev-tools/backends/aqua.md) for comprehensive package management.
:::

Backends are mise's abstraction for different tool installation methods. Each
backend implements the `Backend` trait to provide consistent functionality
across different installation systems.

### Backend Types

- **Core Backends** (`src/backend/core/`) - Built-in language runtimes like
  Node.js, Python, Ruby
- **Package Manager Backends** (`src/backend/`) - npm, pipx, cargo, gem, go
  modules
- **Universal Installers** (`src/backend/`) - ubi, aqua for GitHub releases and
  package management
- **Plugin Backends** (`src/backend/`) - plugins can provide custom backends or individual tools

### Implementation Steps

1. **Create the backend module** in `src/backend/` (e.g., `my_backend.rs`)

2. **Implement the Backend trait**:

   ```rust
   use crate::backend::{Backend, BackendType};
   use crate::install_context::InstallContext;

   #[derive(Debug)]
   pub struct MyBackend {
       // backend-specific fields
   }

   impl Backend for MyBackend {
       fn get_type(&self) -> BackendType { BackendType::MyBackend }

       async fn list_remote_versions(&self) -> Result<Vec<String>> {
           // Implementation for listing available versions
       }

       async fn install_version(&self, ctx: &InstallContext,
                                 tv: &ToolVersion) -> Result<()> {
           // Implementation for installing a specific version
       }

       async fn uninstall_version(&self, tv: &ToolVersion) -> Result<()> {
           // Implementation for uninstalling a version
       }

       // ... other required methods
   }
   ```

3. **Register the backend** in `src/backend/mod.rs`:

   - Add your backend to the imports
   - Add it to the backend registry/factory function
   - Add the `BackendType` enum variant

4. **Add CLI argument parsing** in `src/cli/args/backend_arg.rs` if needed

5. **Update the registry** in `registry.toml` if it should be available as a
   shorthand

### Testing Requirements

- **Integration tests** in `e2e/backend/test_my_backend`
- **Test both installation and usage** of tools from your backend
- **Windows testing** if the backend supports Windows

### Documentation

- **Update backend documentation** in `docs/dev-tools/backends/`
- **Add usage examples** showing how to install tools with your backend
- **Update the registry documentation** if adding new shorthand tools

### Implementation Examples

Look at existing backends for patterns:

- `src/backend/ubi.rs` - Simple GitHub release installer
- `src/backend/npm.rs` - Package manager integration
- `src/backend/core/node.rs` - Full language runtime implementation

For detailed architecture information, see
[Backend Architecture](dev-tools/backend_architecture.md).

## Testing packaging

This is only necessary to test if actually changing the packaging setup.

### Ubuntu (apt)

This is for arm64, but you can change the arch to amd64 if you want.

```sh
docker run -ti --rm ubuntu
apt update -y
apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | \
  sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=arm64] \
https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
apt update
apt install -y mise
mise -V
```

### Fedora (dnf)

```sh
docker run -ti --rm fedora
dnf copr enable -y jdxcode/mise && dnf install -y mise && mise -v
```

### RHEL (dnf)

```sh
docker run -ti --rm registry.access.redhat.com/ubi9/ubi:latest
dnf copr enable -y jdxcode/mise && dnf install -y mise && mise -v
```
