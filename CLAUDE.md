# Repository Agent Guide

This file provides guidance to AI coding agents when working with code in this repository. It keeps the `CLAUDE.md` filename for compatibility with existing tooling, and `AGENTS.md` symlinks to it for other agents.

## Registry Submissions: READ THIS FIRST

**Most registry PRs from agents get rejected.** Before adding anything to `registry/`, understand the rules:

- **mise does not host self-written, personal, niche, or low-popularity tools.** The registry is curated for tools that are *already* widely used. "It works" or "it has tests" is not the bar.
- **Tools that aren't VERY popular will be rejected without explanation.** Per [contributing.md](docs/contributing.md): "@jdx won't explain why a given tool wasn't accepted." There is no appeal, no checklist, no second chance — the PR is closed and that's it.
- **Wasted PRs are the default outcome** for tools the agent or user has not vetted against this bar. Do not submit one speculatively.

### Required pre-submission check

Before touching `registry/`, ALWAYS do the following:

1. **Ask the user:** "Is this tool already widely used outside your own projects?" — if it's the user's own tool, a fork, an internal/company tool, or something with a small audience, **stop and tell them the PR will be rejected.** Do not submit.
2. **Actively check popularity for every registry PR — no exceptions.** Look up real numbers; do not guess. Useful sources:
   - GitHub stars and fork count (`gh repo view owner/repo --json stargazerCount,forkCount`)
   - Recent release activity / last commit date (`gh repo view owner/repo --json pushedAt,latestRelease`)
   - Download counts on relevant package registries (npm `npmjs.com/package/x`, crates.io, PyPI, Homebrew analytics, etc.)
   - Whether the project shows up in third-party docs, awesome-lists, or other tools
3. **Apply the bar.** Rough signals — very low numbers are disqualifying:
   - GitHub stars in the thousands, not hundreds
   - Active maintenance (recent releases, not abandoned)
   - Real third-party usage (referenced in docs, blog posts, other tools, package registries)
   - Recognizable in its ecosystem
4. **Include the popularity data in the PR description.** Every registry PR body MUST contain a short section like:

   ```
   ## Popularity
   - GitHub: 12.3k stars, 480 forks, last release 2026-04-12
   - crates.io: 1.2M downloads
   - Used by: <project A>, <project B>
   ```

   This is non-negotiable — it lets the maintainer evaluate the submission without re-doing the research. PRs without it look speculative and are more likely to be rejected.
5. **If the tool is borderline or numbers are low, warn the user clearly** that the PR is likely to be rejected without reason, and ask if they still want to proceed. Do not soften this — users have repeatedly been surprised when their PR was closed, and the agent should have warned them up front.
6. **Suggest the alternative:** users can install any tool themselves via explicit backend syntax (`mise use aqua:owner/repo`, `mise use github:owner/repo`, `mise use cargo:name`, `mise use npm:name`, etc.) or by writing a [tool plugin](https://mise.en.dev/tool-plugin-development.html). The registry is *only* for shorthand convenience for popular tools — not for enabling installation.

### Backend choice: aqua (preferred) or github

For registry entries the backend tiers are:

- **Tier 1 — preferred:** `aqua:` and `github:`. These are the routinely accepted backends.
  - **Prefer `aqua:`** when the tool is in the [aqua registry](https://github.com/aquaproj/aqua-registry). Better UX, SLSA verification, and per-version logic.
  - **Use `github:`** when the tool isn't in aqua but ships GitHub releases.
- **Tier 2 — high bar, but lower than tier 3:** `conda:`. Potentially acceptable when the tool genuinely can't be supported via aqua/github (e.g. it's a conda-native scientific package only distributed through conda-forge). Still requires a popular, well-maintained tool — but the bar is meaningfully lower than `npm:`/`cargo:`/etc., because for some ecosystems conda is the legitimate distribution channel.
- **Tier 3 — extremely high bar, almost never accepted:** `npm:`, `pipx:`, `cargo:`, `gem:`, `go:`, `dotnet:`. Don't reach for these for a registry PR unless the user has explicitly confirmed @jdx wants it that way for this specific tool. Even very popular tools have been rejected when proposed with one of these backends.
- **Not accepted at all:**
  - **New `asdf:` plugins** — supply-chain security. Use aqua/github instead.
  - **New `vfox:` plugins** — same reason. Use aqua/github instead.
  - **`ubi:`** is deprecated and will not be accepted under any circumstances.

Users can still install via any backend themselves with explicit syntax (`mise use vfox:...`, `mise use cargo:...`, etc.) — they just don't get a registry shorthand for it.

## Development Commands

### Building and Testing
- `mise run build` or `mise run b` - Build the project with cargo
- `target/debug/mise` - Run the built binary directly
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
- `mise --cd crates/vfox run lint-fix` - Run linting and fix issues for the vfox crate
- `mise task ls` - List all available tasks

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
- `registry/` - Tool registry mappings
- `tasks.toml` - Project task definitions

### Test Structure
- Unit tests within source files
- E2E tests in `e2e/` directory organized by feature area (e.g., `e2e/cli/`, `e2e/backend/`)
- E2E tests are bash scripts using assertion helpers from `e2e/assert.sh` (e.g., `assert`, `assert_contains`, `assert_fail`)
- E2E tests do not need cleanup steps (rm, etc.) — the test harness handles that
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
- `fix:` - Bug fixes that affect the CLI behavior (not CI, docs, or infrastructure)
- `refactor:` - Code refactoring
- `docs:` - Documentation changes
- `style:` - Code style/formatting (no logic changes)
- `perf:` - Performance improvements
- `test:` - Testing changes
- `chore:` - Maintenance tasks, releases, dependency updates, CI/infrastructure changes
- `security:` - Security-related changes
- `registry:` - Any changes to `registry/` (no scope needed, use for both new tools and fixes)

**Scopes:**
- For command-specific changes, use the command name: `install`, `activate`, `use`, `exec`, etc.
- For subsystem changes: `config`, `backend`, `env`, `task`, `vfox`, `python`, `github`, `release`, `completions`, `http`, `schema`, `doctor`, `shim`, `core`, `deps`, `ci`
- Use `task` (not `run`) for task-related changes, even if the code lives in `src/cli/run.rs` or `src/cmd.rs`

**Description Style:**
- Use lowercase after the colon
- Use imperative mood ("add feature" not "added feature")
- Keep it concise but descriptive

**Examples:**
- `fix(install): resolve version mismatch for previously installed tools`
- `feat(activate): add fish shell support`
- `feat(vfox): add semver Lua module for version sorting`
- `feat(env): add environment caching with module cacheability support`
- `docs(contributing): update hk usages`
- `chore: release 2026.1.6`
- `chore(ci): add FORGEJO_TOKEN for API authentication`
- `registry: add miller`

### Pre-commit Process
1. Run `hk install --mise` once to set up pre-commit hooks (runs `hk fix` automatically on commit)
2. Run `mise run lint-fix` and `git add` any lint fixes before committing
3. Use `mise run test:e2e [test_filename]...` for running specific e2e tests
4. Never run e2e tests by executing them directly - always use the mise task

## Deprecation Policy

When deprecating a feature or backend:

1. **Immediately**: Mark as deprecated in docs (add warning banner)
2. **6 months later** (`warn_at`): Display deprecation warning in CLI using `deprecated_at!` macro from `src/output.rs`
3. **12 months after warn** (`remove_at`): `debug_assert!` in `deprecated_at!` fires, signaling the code should be removed

Use mise version format for dates (e.g., `deprecated_at!("2026.10.0", "2027.10.0", "id", "message")`).

If the replacement has been available for a long time, the CLI warning can start immediately (set `warn_at` to the current version).

## Important Implementation Notes

### Backend System
When implementing new tool backends, follow the pattern in `src/backend/mod.rs`. Each backend must implement the `Backend` trait with methods for listing versions, installing tools, and managing tool metadata.

### DO NOT ASSUME SEMVER
**Do not assume tool versions follow semver or any other orderable scheme.** mise manages hundreds of tools with wildly different versioning conventions:

- Date-based: `2024.01.15`, `20241015`
- Pre-release / ref / tag versions: `tip`, `HEAD`, `nightly`, `edge`, `canary`, `ref:main`, `tag:v1`, `sub-X.Y:...`
- Non-numeric tags: Python `3.12.0a1`, Ruby `3.2.0-preview1`, Go `1.22rc1`, Node `lts/hydrogen`, `lts-iron`
- Tool-specific meanings of `latest` (e.g. some exclude pre-releases, some don't)

**Rules:**
1. Do not call `versions::Versioning::new(...)` (or any other semver comparator) at a new call site to pick the "newest" version, "resolve latest", or sort a version list. That crate silently returns `None` / arbitrary ordering for non-semver strings, which means wrong versions get chosen for many tools.
2. To resolve a version request (`latest`, a prefix, a channel name), delegate to the backend via `Backend::latest_version`, `Backend::latest_installed_version`, `Backend::list_versions_matching`, or `ToolRequest::resolve` — the backend knows what "latest" means for its tool.
3. To list installed versions in a meaningful order, use `Backend::list_installed_versions_matching` or the toolset's resolved versions. Do not reorder them yourself.
4. Lockfile version strings must be treated as opaque — compare with `==`, never with a version ordering. Never write a non-concrete string (`latest`, `lts/*`, a prefix) into the lockfile; resolve first.

A few existing call sites (e.g. runtime symlinks) do use `Versioning` ordering today, but that's legacy behavior and arguably also wrong — do not point at them to justify new semver assumptions.

If you think you need to pick "the newest installed version" at a new call site, stop and ask — that call almost always belongs on the backend, not inline.

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
- we don't chmod mise e2e tests to be executable

## GitHub Interactions

When posting comments on GitHub PRs or discussions, always include a note that the comment was AI-generated (e.g., "*This comment was generated by an AI coding assistant.*").

## Documentation

### URL Structure
When referencing mise documentation URLs, use the correct path structure based on the `docs/` directory layout:

- **Dev tools & backends**: `mise.en.dev/dev-tools/backends/<backend>.html` (e.g., `mise.en.dev/dev-tools/backends/s3.html`)
- **Configuration**: `mise.en.dev/configuration/...`
- **Tasks**: `mise.en.dev/tasks/...`
- **Environments**: `mise.en.dev/environments/...`
- **CLI reference**: `mise.en.dev/cli/...`

Do NOT use shortened paths like `mise.en.dev/backends/...` - always include the full path matching the `docs/` directory structure.
