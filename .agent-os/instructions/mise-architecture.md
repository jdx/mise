# Mise Architecture and Development Guide

## Project Overview

**Mise** is a powerful Rust CLI tool for development environment management that serves as "the front-end to your dev env." It manages:

- **Development tools** (node, python, go, java, etc.) like asdf/nvm/pyenv but for any language
- **Environment variables** for different project directories like direnv
- **Tasks** for building and testing projects like make

## Core Architecture

### High-Level Structure

```
mise/
├── src/
│   ├── main.rs              # Entry point with async tokio runtime
│   ├── cli/                 # Command-line interface with 60+ subcommands
│   ├── backend/             # Tool backend implementations
│   ├── plugins/core/        # Built-in tool implementations (node, python, etc.)
│   ├── config/              # Configuration file parsing and management
│   ├── toolset/             # Tool version management and installation logic
│   ├── task/                # Task execution system with dependency graph
│   └── ui/                  # User interface components and progress reporting
├── crates/vfox/             # VersionFox plugin system implementation
├── e2e/                     # End-to-end tests organized by feature
└── docs/                    # Documentation site (VitePress)
```

### Key Systems

#### 1. Backend Architecture

- **Unified Backend Interface**: All tools implement the `Backend` trait
- **Multiple Sources**: Tools can come from ASDF plugins, vfox plugins, cargo, npm, GitHub releases, aqua registry, etc.
- **Core Plugins**: Built-in implementations for major languages (Node, Python, Go, Ruby, Java, etc.)
- **External Plugins**: Support for ASDF and vfox plugin ecosystems

#### 2. Configuration System

- **Hierarchical Loading**: system → global → local with environment-specific overrides
- **Multiple Formats**: `mise.toml` (primary), `.tool-versions` (ASDF compat), idiomatic files
- **Environment Variables**: Template system with Tera for dynamic values
- **Tool Management**: Version specifications with semver, latest, system, etc.

#### 3. Toolset Management

- **ToolVersion**: Represents an installed/installable tool version
- **Toolset**: Collection of tool versions with installation state tracking
- **Installation Context**: Manages parallel installations with progress reporting
- **Runtime Symlinks**: Creates symlinks for tool binaries in PATH

#### 4. Task System

- **Dependency Graph**: Tasks can depend on other tasks using petgraph
- **Multiple Sources**: Tasks from `mise.toml`, `tasks.toml`, or external files
- **Environment Handling**: Task-specific environment variables and PATH modifications
- **Execution Modes**: Shell scripts, file references, or command arrays

## Development Standards

### Code Organization

- **Modular Architecture**: Clear separation between CLI, backend, config, and core systems
- **Async/Await**: Heavy use of tokio for concurrent operations
- **Error Handling**: Comprehensive error handling with eyre and custom error types
- **Testing Strategy**: Unit tests in source + comprehensive e2e tests

### Backend Implementation Pattern

When implementing new backends, follow this pattern:

1. Create struct implementing `Backend` trait in `src/backend/`
2. Implement required methods: `id()`, `get_type()`, `list_remote_versions()`, `install_version()`, etc.
3. Add to backend registry in `src/backend/mod.rs`
4. Add comprehensive tests

### Plugin Development

- **Core plugins** go in `src/plugins/core/` for built-in tools
- **External plugins** use ASDF or vfox compatibility layers
- **Plugin metadata** defined in `mise.plugin.toml` files

### Configuration Parsing

- Uses `confique` crate for configuration management
- Settings defined in `settings.toml` (generates code/docs)
- Tool registry mappings in `registry.toml`
- All config changes require running `mise run render:schema`

## Key Files and Directories

### Essential Source Files

- `src/main.rs` - Entry point with panic handlers and async runtime
- `src/cli/mod.rs` - Main CLI router and command definitions
- `src/backend/mod.rs` - Backend system and tool loading logic
- `src/config/mod.rs` - Configuration loading and parsing
- `src/toolset/mod.rs` - Tool version management
- `src/task/mod.rs` - Task execution system

### Configuration Files

- `mise.toml` - Main project configuration file
- `settings.toml` - Global settings definitions (source of truth)
- `registry.toml` - Tool registry mappings (2000+ tools)
- `tasks.toml` - Project task definitions

### Build System

- `Cargo.toml` - Main package with workspace for vfox crate
- `build.rs` - Custom build script for generating metadata
- Cross-compilation support via `Cross.toml`
- Multiple profiles: `release` and `serious` (with LTO)

## Development Workflow

### Build Commands

```bash
mise run build          # Build with cargo
mise run test           # Run all tests (unit + e2e)
mise run test:e2e       # Run e2e tests only
mise run lint           # Run all linting tasks
mise run ci             # Run format, build, and test
```

### Documentation Generation

```bash
mise run render         # Generate all docs and completions
mise run docs           # Start dev server
```

### Testing Strategy

- **Unit Tests**: Within source files using standard Rust testing
- **E2E Tests**: In `e2e/` directory with real tool installations
- **Snapshot Tests**: Using `insta` crate for CLI output verification
- **Cross-Platform**: Separate Windows tests in `e2e-win/`

## Commit and PR Guidelines

### Conventional Commits (REQUIRED)

Format: `<type>(<scope>): <description>`

**Types:** feat, fix, refactor, docs, style, perf, test, chore, registry
**Scopes:** aqua, cli, config, backend, tool, env, task, api, ui, core, deps, schema, doctor, shim, security

**Examples:**

- `feat(cli): add new command for tool management`
- `fix(config): resolve parsing issue with nested tables`
- `registry: add trunk metalinter (#5875)`

### Pre-commit Requirements

1. Run `mise run lint-fix` and commit any fixes
2. Use `mise run test:e2e [test_filename]...` for specific tests
3. Never run e2e tests directly - always use mise task

## Technology Stack

- **Language**: Rust 2024 edition (min version 1.85)
- **Runtime**: Tokio async runtime with multi-threading
- **CLI Framework**: clap with derive features
- **Config**: confique, toml, serde
- **HTTP**: reqwest with various TLS backends
- **Crypto**: Multiple hash algorithms, minisign verification
- **Templating**: Tera for environment variable templating
- **Testing**: insta for snapshots, ctor for test setup

## Architecture Principles

1. **Performance**: Parallel operations where possible, efficient caching
2. **Reliability**: Comprehensive error handling and recovery
3. **Extensibility**: Plugin system supporting multiple backends
4. **Compatibility**: ASDF compatibility while adding modern features
5. **User Experience**: Rich progress reporting and helpful error messages
