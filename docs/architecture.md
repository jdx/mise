---
outline: [1, 3]
---
# mise Architecture

This document provides a comprehensive overview of mise's architecture, designed primarily for contributors and those interested in understanding how mise works internally.

For practical development guidance, see the [Contributing Guide](contributing.md).

## System Overview

mise is a Rust-based tool with a modular architecture centered around three core concepts:

1. **Tool Version Management** - Installing and managing different versions of [development tools](dev-tools/)
2. **Environment Management** - Setting up [environment variables](environments/) and project contexts  
3. **Task Running** - Executing [project tasks](tasks/) with dependency management

These three pillars work together to provide a unified development environment management experience.

## Core Architecture Components

### Command Layer ([`src/cli/`](https://github.com/jdx/mise/tree/main/src/cli/))

The CLI layer provides the user interface and delegates to core functionality:

- **Modular Commands**: Each command is a separate module ([`install.rs`](https://github.com/jdx/mise/blob/main/src/cli/install.rs), [`use.rs`](https://github.com/jdx/mise/blob/main/src/cli/use.rs), [`run.rs`](https://github.com/jdx/mise/blob/main/src/cli/run.rs), etc.)
- **Argument Parsing**: Leverages [`clap`](https://clap.rs) for robust CLI parsing and validation
- **Async Command Execution**: All commands support concurrent operations
- **Unified Error Handling**: Consistent error reporting across all commands

**Key Commands Architecture:**

- [`install`](cli/install.md) - Tool installation coordination
- [`use`](cli/use.md) - Tool activation and configuration management
- [`run`](cli/run.md) - Task execution with dependency resolution
- [`env`](cli/env.md) - Environment variable management
- [`shell`](cli/shell.md) - Shell integration and activation

### Backend System ([`src/backend/`](https://github.com/jdx/mise/tree/main/src/backend/))

The backend system is mise's core abstraction for tool management, implementing a trait-based architecture:

```rust
pub trait Backend: Debug + Send + Sync {
    async fn list_remote_versions(&self) -> Result<Vec<String>>;
    async fn install_version(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()>;
    async fn uninstall_version(&self, tv: &ToolVersion) -> Result<()>;
    // ... additional methods for lifecycle management
}
```

**Backend Categories:**

- **Core Backends**: Native Rust implementations for maximum performance
- **Language Package Managers**: npm, pipx, cargo, gem, go modules
- **Universal Installers**: ubi (GitHub releases), aqua (comprehensive package management)
- **Plugin Systems**: [backend plugins](backend-plugin-development.md) (enhanced methods), [tool plugins](tool-plugin-development.md) (hook-based), [asdf plugins](asdf-legacy-plugins.md) (legacy)

For guidance on implementing new backends, see the [Contributing Guide](contributing.md#adding-backends). For detailed backend system design, see [Backend Architecture](dev-tools/backend_architecture.md).

### Configuration System ([`src/config/`](https://github.com/jdx/mise/tree/main/src/config/))

A hierarchical configuration system that merges settings from multiple config files:

**Config Trait Architecture:**

```rust
pub trait ConfigFile: Debug + Send + Sync {
    fn get_path(&self) -> &Path;
    fn to_tool_request_set(&self) -> Result<ToolRequestSet>;
    fn env_entries(&self) -> Result<Vec<EnvDirective>>;
    fn tasks(&self) -> Vec<&Task>;
    // ... additional configuration methods
}
```

**Concrete Implementations:**

- `MiseToml` - Primary configuration format with full feature support
- `ToolVersions` - asdf compatibility layer
- `IdiomaticVersion` - Language-specific version files (`.node-version`, etc.)

**Configuration Hierarchy:** See [Configuration Documentation](configuration.md) for the complete hierarchy and precedence rules.

### Toolset Management ([`src/toolset/`](https://github.com/jdx/mise/tree/main/src/toolset/))

Coordinates tool resolution, installation, and environment setup:

**Core Components:**

- `Toolset` - Immutable collection of resolved tools for a context
- `ToolVersion` - Represents a specific, resolved tool version (e.g., `node@latest` becomes `node@18.17.0`)
- `ToolRequest` - User's tool specification (e.g., `node@18`, `python@latest`)
- `ToolsetBuilder` - Constructs toolsets from configuration with dependency resolution

**Tool Resolution Pipeline:**

1. **Configuration Parsing**: Extract tool requirements from config files
2. **Version Resolution**: Resolve version specifications (`latest`, `~1.2.0`, etc.) to concrete versions
3. **Backend Selection**: Choose appropriate backend for each tool
4. **Dependency Analysis**: Resolve tool dependencies (e.g., npm requires Node.js)
5. **Installation Coordination**: Install missing tools in dependency order
6. **Environment Configuration**: Set up PATH and environment variables

### Task System ([`src/task/`](https://github.com/jdx/mise/tree/main/src/task/))

Sophisticated task execution with dependency graph management:

**Architecture Components:**

- `Task` - Task definition with metadata, dependencies, and execution configuration
- `Deps` - Dependency graph manager using `petgraph` for DAG operations
- `TaskFileProvider` - Discovers tasks from files and configuration
- Parallel execution engine with configurable concurrency

**Task Discovery:**

1. [File-based tasks](tasks/file-tasks.md) from configured directories
2. [TOML-defined tasks](tasks/toml-tasks.md) in configuration files
3. Inherited tasks from parent directories

**Dependency Resolution:**

- Uses directed acyclic graph (DAG) for dependency modeling
- Supports multiple dependency types: `depends`, `depends_post`, `wait_for`
- Parallel execution within dependency constraints
- Circular dependency detection and prevention

See the [Task Documentation](tasks/) for complete usage details and configuration options, and [Task Architecture](tasks/architecture.md) for detailed system design.

### Plugin System ([`src/plugins/`](https://github.com/jdx/mise/tree/main/src/plugins/))

Extensibility layer supporting multiple plugin architectures:

**Plugin Trait:**

```rust
pub trait Plugin: Debug + Send {
    fn name(&self) -> &str;
    fn path(&self) -> PathBuf;
    async fn install(&self, config: &Arc<Config>, pr: &Box<dyn SingleReport>) -> Result<()>;
    async fn update(&self, pr: &Box<dyn SingleReport>, gitref: Option<String>) -> Result<()>;
    // ... lifecycle management methods
}
```

**Plugin Types:**

- **Backend Plugins**: Enhanced plugins with backend methods for managing multiple tools
- **Tool Plugins**: Hook-based plugins using the traditional vfox format
- **asdf Plugins**: Legacy plugins compatible with the asdf plugin ecosystem (Linux/macOS only)

For complete plugin documentation, see [Plugin Guide](plugins.md).

### Shell Integration ([`src/shell/`](https://github.com/jdx/mise/tree/main/src/shell/))

Shell-specific code generation that abstracts commands like `mise env` and contains all shell differences in one place:

**Shell Trait:**

```rust
pub trait Shell {
    fn activate(&self, opts: ActivateOptions) -> String;
    fn set_env(&self, k: &str, v: &str) -> String;
    fn unset_env(&self, k: &str) -> String;
    // ... shell-specific methods
}
```

**Supported Shells:** See [`mise activate`](cli/activate.md) documentation for the complete list
**Shell Abstractions:** Environment variable setting, PATH modification, command execution

### Environment Management ([`src/env*.rs`](https://github.com/jdx/mise/tree/main/src/))

Helpers for working with environment variables:

- `EnvDiff` - Tracks and applies environment changes
- `EnvDirective` - Configuration-based environment variable management
- `PathEnv` - Intelligent PATH manipulation with precedence rules
- Context-aware resolution with inheritance

For environment setup and configuration, see [Environment Documentation](environments/).

### Caching System ([`src/cache.rs`](https://github.com/jdx/mise/blob/main/src/cache.rs))

Generic caching backed by files, using msgpack serialization with zstd compression:

- `CacheManager<T>` - Generic caching with TTL support
- Data serialized with msgpack and compressed with zstd for efficient storage
- Automatic cache invalidation based on file timestamps
- Per-backend cache isolation for data integrity

## Test Architecture

mise employs a multi-layered testing strategy that combines different testing approaches for thorough validation across its complex feature set.

**Testing Strategy Overview:**

1. **Unit Tests** - Rust `#[test]` functions embedded in source files
2. **End-to-End (E2E) Tests** - Bash-based integration tests with complete environment isolation
3. **Snapshot Tests** - Using `insta` crate for complex output validation

::: tip Testing Philosophy
**Most tests in mise are end-to-end tests, and this is generally the preferred approach** for new functionality. E2E tests provide thorough validation of real-world usage scenarios and catch integration issues that unit tests might miss. However, **E2E tests can be challenging to run locally** due to environment dependencies and setup complexity. For development and CI purposes, it's often easier to run tests on GitHub Actions where the environment is consistent and properly configured.

See the [Contributing Guide](contributing.md#testing) for detailed testing setup and guidelines.
:::

### Unit Tests ([`src/` modules](https://github.com/jdx/mise/tree/main/src/))

**Structure and Characteristics:**

- **Location**: Embedded within source files using `mod tests` blocks
- **Test Runner**: Standard Rust `cargo test`
- **Dependencies**: `pretty_assertions`, `insta`, `test-log`, `ctor`
- **Coverage**: ~50+ test modules covering all major functionality

```rust
mod tests {
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use crate::config::Config;
    use super::*;

    #[tokio::test]
    async fn test_hash_to_str() {
        let _config = Config::get().await.unwrap();
        assert_eq!(hash_to_str(&"foo"), "e1b19adfb2e348a2");
    }
}
```

**Test Environment Setup:**

- **Global Setup**: Uses `ctor::ctor` in [`src/test.rs`](https://github.com/jdx/mise/blob/main/src/test.rs) for test environment initialization
- **Isolated Environment**: Each test gets a clean environment with custom `HOME`, cache, and config directories
- **Async Support**: Extensive use of `#[tokio::test]` for async testing

### End-to-End Tests ([`e2e/`](https://github.com/jdx/mise/tree/main/e2e/))

**Architecture:**

```
e2e/
├── run_test          # Single test executor with environment isolation
├── run_all_tests     # Test orchestrator with parallel execution
├── assert.sh         # Rich assertion library
├── cli/              # CLI command tests
│   ├── test_use      # Testing tool activation and configuration
│   ├── test_install  # Testing tool installation
│   ├── test_upgrade  # Testing tool upgrades
│   ├── test_uninstall # Testing tool removal
│   └── test_version  # Testing version commands
├── backend/          # Backend-specific tests
│   ├── test_aqua     # Testing aqua package manager
│   ├── test_asdf     # Testing asdf plugin compatibility
│   └── test_npm      # Testing npm backend
├── tasks/            # Task system tests
│   ├── test_task_deps # Testing task dependencies
│   ├── test_task_run_depends # Testing task execution order
│   ├── test_task_ls  # Testing task listing
│   └── test_task_info # Testing task metadata
├── config/           # Configuration tests
│   ├── test_config_ls # Testing configuration listing
│   └── test_config_set # Testing configuration updates
└── [other domains]/  # Additional test categories
```

**Environment Isolation System:**

Each test runs in complete isolation with temporary directories:

```bash
setup_isolated_env() {
  TEST_ISOLATED_DIR="$(mktemp --tmpdir --directory "$(basename "$TEST").XXXXXX")"
  TEST_HOME="$TEST_ISOLATED_DIR/home"
  MISE_DATA_DIR="$TEST_HOME/.local/share/mise"
  MISE_CACHE_DIR="$TEST_HOME/.cache/mise"
  # ... complete environment isolation
}
```

**Rich Assertion Framework:**

The [`assert.sh`](https://github.com/jdx/mise/blob/main/e2e/assert.sh) provides rich test utilities:

```bash
# Basic assertions
assert "command" "expected_output"
assert_contains "command" "substring"
assert_fail "command" "error_message"

# JSON testing
assert_json "command" '{"key": "value"}'
assert_json_partial_object "command" "field1,field2" '{"field1": "value1"}'

# File system assertions
assert_directory_exists "path"
assert_directory_empty "path"
```

**Test Categories:**

- **CLI Tests**: Validate all command-line interfaces and argument parsing
- **Backend Tests**: Test tool installation, version resolution, and backend integration
- **Task Tests**: Validate task execution, dependency resolution, and parallel execution
- **Configuration Tests**: Test configuration parsing, hierarchy, and environment variable handling

### Windows Testing

**Windows-Specific Tests ([`e2e-win/`](https://github.com/jdx/mise/tree/main/e2e-win/)):**

- **Language**: PowerShell scripts (`.ps1`)
- **Focus**: Windows-specific functionality and cross-platform compatibility
- **Coverage**: Core tools like Go, Java, Node.js, Python, Rust

```powershell
Describe "go" {
    It "installs go" {
        mise install go@latest
        go version | Should -Match "go version"
    }
}
```

### Snapshot Testing ([`src/snapshots/`](https://github.com/jdx/mise/tree/main/src/snapshots/))

**Implementation:**

- **Crate**: Uses `insta` for snapshot testing with 11 snapshot files
- **Format**: Stores expected outputs as `.snap` files
- **Coverage**: Complex outputs like directory listings, configuration parsing, environment diffs

```rust
#[tokio::test]
async fn test_parse() {
    let diff = DirenvDiff::parse(input).unwrap();
    assert_snapshot!(diff);  // Creates/validates snapshot
}
```

### Test Infrastructure Features

**Performance and Utility Tests ([`xtasks/test/`](https://github.com/jdx/mise/tree/main/xtasks/test/)):**

- **Performance Testing**: `perf` script for benchmarking
- **Coverage Testing**: `coverage` script for test coverage analysis
- **E2E Runner**: `e2e` script with filtering capabilities

**Test Data Management ([`test/`](https://github.com/jdx/mise/tree/main/test/)):**

```
test/
├── fixtures/          # Sample configuration files
├── config/           # Test-specific configs
├── data/             # Test plugins and mock data
└── state/            # Test state directory
```

**Test Execution Modes:**

- **Fast Tests**: Regular tests that run in CI
- **Slow Tests**: Marked with `_slow` suffix, skipped unless `TEST_ALL=1`
- **Tranche Support**: Tests can be split across parallel runners using `TEST_TRANCHE_COUNT`

**Developer Experience Features:**

- **Environment Safety**: Complete isolation prevents tests from affecting user's actual mise installation
- **Parallel Execution**: E2E tests support parallel execution with proper isolation
- **Rich Reporting**: Detailed test timing, environment preservation on failure for debugging
- **Cross-Platform Validation**: Automated testing on multiple operating systems

**Running Tests:**

```bash
# Run all unit tests
cargo test

# Run all E2E tests
./e2e/run_all_tests

# Run specific E2E test
./e2e/run_test test_install

# Run with coverage
./xtasks/test/coverage

# Performance testing
./xtasks/test/perf
```

For complete development setup and testing procedures, see the [Contributing Guide](contributing.md).

This robust test architecture ensures mise's reliability across its complex feature set, including tool management, environment configuration, task execution, and multi-platform support.

## Related Architecture Documentation

For deeper understanding of specific subsystems:

- **[Task Architecture](tasks/architecture.md)** - Detailed design of the task dependency system, parallel execution engine, and task discovery mechanisms
- **[Backend Architecture](dev-tools/backend_architecture.md)** - In-depth guide to backend types, the trait system, and how different installation methods work
