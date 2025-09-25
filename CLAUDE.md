# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Building and Testing
- `mise run build` or `mise run b` - Build the project with cargo
- `mise run test` or `mise run t` - Run all tests (unit + e2e)
- `mise run test:unit` - Run unit tests only
- `mise run test:e2e` - Run end-to-end tests only
- `mise run snapshots` - Update test snapshots with `cargo insta`

### Debugging
- Use `MISE_DEBUG=1` or `MISE_TRACE=1` environment variables to enable debug output (not `RUST_LOG`)

### Code Quality and Testing
- `mise run lint` - Run all linting tasks
- `mise run lint-fix` - Run linting and automatically fix issues
- `mise run format` - Format code (part of CI task)
- `mise run ci` - Run format, build, and test
- `mise run test:e2e [test_filename]...` - Run specific e2e tests (use this instead of executing test files directly)
- `mise --cd crates/vfox run test` - Run tests for the vfox crate
- `mise --cd crates/vfox run lint` - Run linting for the vfox crate

### Documentation and Generation
- `mise run render` - Generate all documentation and completions
- `mise run render:usage` - Generate CLI usage documentation
- `mise run render:completions` - Generate shell completions
- `mise run docs` - Start documentation dev server
- `mise run docs:build` - Build documentation

### Development
- `mise run install-dev` - Install development version locally
- `mise run clean` - Clean cargo build artifacts

## Code Architecture

### High-Level Structure
Mise is a Rust CLI tool that manages development environments, tools, tasks, and environment variables. The codebase follows a modular architecture:

**Core Components:**
- `src/main.rs` - Entry point and CLI initialization
- `src/cli/` - Command-line interface implementation with subcommands
- `src/config/` - Configuration file parsing and management
- `src/backend/` - Tool backend implementations (asdf, vfox, cargo, npm, etc.)
- `src/toolset/` - Tool version management and installation logic
- `src/task/` - Task execution system
- `src/plugins/` - Plugin system for extending tool support

**Key Backend Systems:**
- `src/backend/asdf.rs` - ASDF plugin compatibility
- `src/backend/vfox.rs` - VersionFox plugin system
- `src/backend/cargo.rs` - Rust Cargo tool backend
- `src/backend/npm.rs` - Node.js/npm tool backend
- `src/backend/github.rs` - GitHub releases backend
- `src/backend/aqua.rs` - Aqua tool registry integration

**Core Tools (Built-in):**
- `src/plugins/core/` - Built-in tool implementations (Node, Python, Go, Ruby, etc.)

**Configuration System:**
- `mise.toml` files for project configuration
- `.tool-versions` files for ASDF compatibility
- Environment variable management and templating
- Task definition and execution

### Key Design Patterns
1. **Backend Architecture**: Tools are implemented through a unified backend interface, allowing multiple sources (ASDF plugins, vfox plugins, cargo, npm, etc.)
2. **Toolset Management**: The `Toolset` manages collections of tool versions and their installation state
3. **Configuration Layering**: Config files are loaded hierarchically from system → global → local with environment-specific overrides
4. **Task System**: Tasks can be defined in TOML files with dependencies, environment variables, and multiple execution modes

### Configuration Files
- `mise.toml` - Main configuration file format
- `settings.toml` - Global settings definitions (generates code/docs)
- `registry.toml` - Tool registry mappings
- `tasks.toml` - Project task definitions

### Test Structure
- Unit tests within source files
- E2E tests in `e2e/` directory organized by feature area
- Snapshot tests using `insta` crate for CLI output verification
- Windows-specific tests in `e2e-win/`

### Build System
- Rust project using Cargo with workspace for `crates/vfox`
- Custom build script in `build.rs` for generating metadata
- Multiple build profiles including `release` and `serious` (with LTO)
- Cross-compilation support via `Cross.toml`

## Development Guidelines

### Conventional Commits (REQUIRED)
All commit messages and PR titles MUST follow conventional commit format:

**Format:** `<type>(<scope>): <description>`

**Types:**
- `feat:` - New features
- `fix:` - Bug fixes
- `refactor:` - Code refactoring
- `docs:` - Documentation
- `style:` - Code style/formatting
- `perf:` - Performance improvements
- `test:` - Testing changes
- `chore:` - Maintenance tasks
- `chore(deps):` - Dependency updates
- `registry:` - New tool additions to the registry (no scope needed)

**Common Scopes:** `aqua`, `cli`, `config`, `backend`, `tool`, `env`, `task`, `api`, `ui`, `core`, `deps`, `schema`, `doctor`, `shim`, `security`

**Examples:**
- `feat(cli): add new command for tool management`
- `fix(config): resolve parsing issue with nested tables`
- `test(e2e): add tests for tool installation`
- `registry: add trunk metalinter (#5875)` - Adding new tool to registry

### Pre-commit Process
1. Run `mise run lint-fix` and `git add` any lint fixes before committing
2. Use `mise run test:e2e [test_filename]...` for running specific e2e tests
3. Never run e2e tests by executing them directly - always use the mise task

## Important Implementation Notes

### Backend System
When implementing new tool backends, follow the pattern in `src/backend/mod.rs`. Each backend must implement the `Backend` trait with methods for listing versions, installing tools, and managing tool metadata.

### Plugin Development
- Core tools are implemented in `src/plugins/core/`
- External plugins use ASDF or vfox compatibility layers
- Plugin metadata is defined in `mise.plugin.toml` files

### Configuration Parsing
The configuration system supports multiple file formats and environment-specific configs. Changes to settings require updating `settings.toml` and running `mise run render:schema`.

### Testing Strategy
- E2E tests are organized by feature area (cli/, config/, backend/, etc.)
- Use snapshot testing for CLI output verification
- Backend-specific tests verify tool installation and version management
- Slow tests (marked with `_slow` suffix) test actual tool compilation/installation

### Cross-Platform Considerations
- Windows-specific implementations in files ending with `_windows.rs`
- Platform-specific tool installation logic in core plugins
- Shim system varies by platform (especially Windows)
