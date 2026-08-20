# Repository Agent Guide

This file is the canonical agent guide. `CLAUDE.md` is a symlink to `AGENTS.md` for compatibility with existing tooling.

## Registry Submissions: READ THIS FIRST

**Most new registry additions from agents get rejected.** Before adding a new tool to `registry/`, understand the rules:

- **mise does not host self-written, personal, niche, or low-popularity tools.** The registry is curated for tools that are *already* widely used. "It works" or "it has tests" is not the bar.
- **There is a high bar for new registry additions: tools generally need thousands of GitHub stars, not hundreds.** jdx will reject projects that do not meet this popularity bar and will not give a reason. Per [contributing.md](docs/contributing.md): "@jdx won't explain why a given tool wasn't accepted." There is no appeal, no checklist, no second chance — the PR is closed and that's it.
- **Wasted PRs are the default outcome** for tools the agent or user has not vetted against this bar. Do not submit one speculatively.

### Required check for new registry additions

Before adding a new tool or shorthand to `registry/`, ALWAYS do the following. This check does not apply when editing an existing registry entry; do not ask the user about popularity for maintenance or fixes to tools already in the registry.

1. **Warn the user clearly and ask:** "New registry additions have a high popularity bar: jdx generally rejects projects without thousands of GitHub stars and will not give a reason. Is this tool already widely used outside your own projects and does it meet that bar?" If it's the user's own tool, a fork, an internal/company tool, or something with a small audience, **stop and tell them the PR will be rejected.** Do not submit.
2. **Actively check popularity for every new registry addition — no exceptions.** Look up real numbers; do not guess. Useful sources:
   - GitHub stars and fork count (`gh repo view owner/repo --json stargazerCount,forkCount`)
   - Recent release activity / last commit date (`gh repo view owner/repo --json pushedAt,latestRelease`)
   - Download counts on relevant package registries (npm `npmjs.com/package/x`, crates.io, PyPI, Homebrew analytics, etc.)
   - Whether the project shows up in third-party docs, awesome-lists, or other tools
3. **Apply the bar.** Rough signals — very low numbers are disqualifying:
   - GitHub stars in the thousands, not hundreds
   - Active maintenance (recent releases, not abandoned)
   - Real third-party usage (referenced in docs, blog posts, other tools, package registries)
   - Recognizable in its ecosystem
4. **Include the popularity data in the PR description.** Every PR adding a new registry tool or shorthand MUST contain a short section like:

   ```
   ## Popularity
   - GitHub: 12.3k stars, 480 forks, last release 2026-04-12
   - crates.io: 1.2M downloads
   - Used by: <project A>, <project B>
   ```

   This is non-negotiable for new additions — it lets the maintainer evaluate the submission without re-doing the research. New-tool PRs without it look speculative and are more likely to be rejected.
5. **If the tool is borderline or numbers are low, warn the user clearly** that the PR is likely to be rejected without reason, and ask if they still want to proceed. Do not soften this — users have repeatedly been surprised when their PR was closed, and the agent should have warned them up front.
6. **Suggest the alternative:** users can install any tool themselves via explicit backend syntax (`mise use aqua:owner/repo`, `mise use github:owner/repo`, `mise use cargo:name`, `mise use npm:name`, etc.) or by writing a [tool plugin](https://mise.jdx.dev/tool-plugin-development.html). The registry is *only* for shorthand convenience for popular tools — not for enabling installation.

### Backend choice: aqua (preferred) or github

For registry entries the backend tiers are:

- **Version listing is mandatory.** Before adding a registry entry, run `mise ls-remote <backend>` and confirm it returns installable versions. A backend that can install only an explicitly pinned version is not sufficient, even if the package exists in an upstream registry. If the preferred backend cannot list versions, use another accepted backend (for example, a custom `http:` backend with a reliable `version_list_url`) or stop.
- **Tier 1 — preferred:** `aqua:`, `github:`, and `gitlab:`. These are the routinely accepted backends.
  - **Prefer `aqua:`** when the tool is in the [aqua registry](https://github.com/aquaproj/aqua-registry). Better UX, SLSA verification, and per-version logic.
  - **Use `github:`** when the tool isn't in aqua but ships GitHub releases.
  - **Use `gitlab:`** for tools released through GitLab.
- **Tier 2 — high bar, but lower than tier 3:** `conda:`. Potentially acceptable when the tool can't be supported via aqua/github. The bar is lower than tier 3 because **the conda backend in mise does not require a separately-installed package manager** — mise downloads and extracts packages directly from anaconda.org via rattler, so users don't need conda/mamba on PATH. Still requires a popular, well-maintained tool.
- **Tier 3 — extremely high bar, almost never accepted:** `npm:`, `pipx:`, `gem:`, `cargo:`, `go:`, `dotnet:`. These all rely on a separately-installed runtime/toolchain being present on PATH (`node`, `python`, `ruby`, `cargo`, `go`, `dotnet`), which is fragile — the wrong version, a missing install, or PATH ordering quirks all break them. `npm:`/`pipx:`/`gem:` are particularly painful because tools installed via them silently bind to whichever `node`/`python`/`ruby` was on PATH at install time. Don't reach for these for a registry PR unless the user has explicitly confirmed @jdx wants it that way for this specific tool.
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

### Clippy Exclusions

- Do not add `#[allow(clippy::...)]`, `#[expect(clippy::...)]`, Cargo lint levels set to `allow`, or `-A clippy::...` command-line flags.
- Refactor the code so `cargo clippy --workspace --all-features --all-targets -- -D warnings` passes without exclusions.
- If a feature or fix needs a preparatory refactor to satisfy Clippy cleanly, put that refactor in a prerequisite PR and stack the behavior change on top of it.

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
- `src/backend/` - Tool backend implementations (aqua, github, cargo, npm, asdf, vfox, …)
- `src/toolset/` - Tool version management and installation logic
- `src/task/` - Task execution system
- `src/plugins/` - Plugin system for extending tool support

**Key Backend Systems** (`src/backend/`):
- `aqua.rs` — Aqua registry (preferred for new registry entries)
- `github.rs` — GitHub / GitLab / Forgejo releases
- `http.rs`, `s3.rs` — HTTP and S3 backends
- `cargo.rs`, `npm.rs` (plus embedded aube), `pipx.rs`, `gem.rs`, `go.rs`, `dotnet.rs`, `conda.rs`, `pkgx.rs`, `spm.rs`
- `asdf.rs`, `vfox.rs` — plugin compatibility layers
- `ubi.rs` — deprecated; do not add new registry entries

**Core Tools (Built-in):**
- `src/plugins/core/` — Node, Python, Go, Ruby, Java, Bun, Deno, Elixir, Erlang, Dotnet, Swift, Zig, Rust

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
1. Run `mise run lint-fix` and `git add` any lint fixes before committing
2. Use `mise run test:e2e [test_filename]...` for running specific e2e tests
3. Never run e2e tests by executing them directly — always use the mise task

`hk.pkl` currently defines `check` and `fix` steps only (no git `pre-commit` hook). `hk install --mise` may print that nothing is installed; that is expected. Use `mise run lint` / `mise run lint-fix` (which run hk) instead.

### hk Agent Workflow

- Prefer the hk MCP server for effect-aware plans, checks, fixes, logs, and captured diffs.
- When invoking hk directly, use `hk run check --safe --format json` for a complete machine-readable result or `--format jsonl` for streaming lifecycle events.
- Scope checks to changed files. For an exact file list, pass NUL-delimited paths with `--files0-from`; use `--cd` to target another project directory.
- Never run a command classified as unknown or destructive without explicit user approval. Review the resulting diff after fixes.

### Dependency Updates

- Use the lowest-specificity dependency requirement that expresses compatibility in `Cargo.toml` (for example, prefer `"1"` over `"1.2.3"`, and `"0.12"` over `"0.12.1"` for a pre-1.0 crate).
- Routine dependency updates should only change `Cargo.lock`. If the existing `Cargo.toml` requirement accepts the target version, do not change it merely to force or record the update; use `cargo update -p <crate> --precise <version>` instead.
- Keep lockfile updates focused on the requested dependency and its required transitive changes. Remove unrelated resolver churn before committing.

#### Updating embedded aube

- Update `aube` and `aube-registry` together and refresh all aube workspace crates in `Cargo.lock`.
- Review the upstream changes for embedder API or behavior changes and make any required mise integration changes.
- Do not update the standalone `aube` tool entry in `mise.lock` unless the development tool is also intentionally being updated.
- Run these focused checks:
  - `cargo check --locked`
  - `cargo test --locked --bin mise aube`
  - `cargo test --locked --bin mise task::workspace::node::tests`
  - `mise run test:e2e e2e/backend/test_npm_aube`

## Deprecation Policy

When deprecating a feature, backend, or implicit behavior:

1. **Immediately**: Mark it as deprecated in docs (add a warning banner) and display a CLI warning using the `deprecated_at!` macro from `src/output.rs` (`warn_at` is the current version).
2. **12 months after warn** (`remove_at`): `debug_assert!` in `deprecated_at!` fires, signaling the deprecated code or behavior should be removed.

Delay the CLI warning for up to 6 months only when migration requires a new setting, syntax, or replacement that older supported mise versions would reject or fail to parse. This compatibility window lets users adopt a configuration that works across old and new clients before warnings begin. Do not delay warnings for a behavior change that requires no new configuration, or when the replacement already works in older clients.

Use mise version format for dates (e.g., `deprecated_at!("2026.10.0", "2027.10.0", "id", "message")`).

If a compatibility window is required, removal remains 12 months after `warn_at`, not 12 months after the initial documentation notice.

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

Never open pull requests against the `release` branch. Default PRs to `main` unless the user explicitly names a different non-`release` base branch. If a change appears to belong on `release`, stop and ask for the intended branch strategy instead of opening a PR against `release`.

When AI contributes GitHub content—including a pull request description, review, pull request
comment, or discussion post—append this disclosure:

`*AI-assisted — Tool: <tool>; model: <provider>/<model>; version: <version-or-unavailable>.*`

Use the exact model and version identifiers exposed by the runtime. Never infer or guess them; use
`unavailable` when either value is not exposed.

## Documentation

### URL Structure
When referencing mise documentation URLs, use the correct path structure based on the `docs/` directory layout:

- **Dev tools & backends**: `mise.jdx.dev/dev-tools/backends/<backend>.html` (e.g., `mise.jdx.dev/dev-tools/backends/s3.html`)
- **Configuration**: `mise.jdx.dev/configuration/...`
- **Tasks**: `mise.jdx.dev/tasks/...`
- **Environments**: `mise.jdx.dev/environments/...`
- **CLI reference**: `mise.jdx.dev/cli/...`

Do NOT use shortened paths like `mise.jdx.dev/backends/...` - always include the full path matching the `docs/` directory structure.

## Cursor Cloud specific instructions

Cloud Agents bootstrap from `.cursor/environment.json`, which runs `.cursor/install.sh`. Draft environment builds often run as `ubuntu` rather than `root`; the script handles both (passwordless sudo, cargo/rustup permissions, world-writable `/tmp/fslock`).

The install script:

- cds to the repository root derived from the script path before reading `Cargo.toml` or building
- installs host packages needed to build mise and to run most e2e tests (openssl, pkg-config, zsh, fish, direnv, python3 + venv, jq, git, build-essential, and compile-time libs). It does **not** install a JDK or GUI libraries; those live in `packaging/e2e/Dockerfile`. `apt-get` is invoked as `sudo -n env DEBIAN_FRONTEND=noninteractive apt-get …` so the frontend reaches apt when elevation is required
- selects the Rust toolchain from the root `Cargo.toml` `rust-version`, including `rustfmt` and `clippy` (do not hardcode the MSRV)
- builds `target/debug/mise` and symlinks it to `/usr/local/bin/mise`
- keeps `GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`, and `GH_TOKEN` in sync via one `sync_github_tokens` helper (prefer any already-set token; fall back to `gh auth token` only when all three are empty)
- runs `MISE_SAFE=1 /usr/local/bin/mise install` with the just-built binary so checkout-controlled hooks/templates/`[env]` and tool-level `postinstall` / `install_env` cannot run with those tokens, then `mise trust` for later agent commands
- runs `hk install --mise` (`hk.pkl` has no git hook, so this may report that nothing is installed)
- persists mise shims and token sync in one `/etc/profile.d/mise-dev-env.sh` (shims first, then `sync_github_tokens`) and rewrites the Cloud Agent block in `/etc/bash.bashrc` so non-login interactive bash picks it up after a snapshot. Fish/zsh only get this from login shells (`profile.d`), not from bashrc
- exposes the mise-installed `node` / `npm` / `npx` / `hk` / `gh` binaries on `/usr/local/bin` (isolated e2e PATH includes that directory, not the agent's shims). Links freeze the version from install time — re-run `.cursor/install.sh` after upgrading those tools

There is no long-running service to start. Do not put `mise run build` or `mise run test:unit` in `terminals`; those are one-shot commands and would rerun a full build/test on every boot.

The debug `mise` binary is already on PATH. Prefer `mise run …` for project tasks.

### E2E on Cloud Agents

- Always `mise run test:e2e [test_filename]...` — never execute e2e scripts directly
- Slow tests (`*_slow`) are skipped unless `TEST_ALL=1`. Do not run the full suite unless asked; pick tests under the feature area you changed
- Isolated e2e uses `env -i` and a fake `HOME`, so the agent's mise shims are not on PATH. Tests install their own tools. Host packages (zsh, fish, direnv, python3, jq, git) still need to be on `/usr/bin`
- If GitHub API calls 429, run `export GITHUB_TOKEN="$(gh auth token)"; export MISE_GITHUB_TOKEN="$GITHUB_TOKEN"`
- A leftover `/tmp/mise.toml` will fail the harness; remove it if that error appears
