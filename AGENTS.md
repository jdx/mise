# Repository Agent Guide

This file is the canonical agent guide. `CLAUDE.md` is a symlink to `AGENTS.md` for compatibility with existing tooling.

Task-specific procedures live as skills in `.agents/skills/` (`.claude/skills` is a symlink to it). Load the named skill when this guide points to one; agents without skill support can read its `SKILL.md` directly.

## Registry Submissions: READ THIS FIRST

**Most new registry additions from agents get rejected.** mise's `registry/` is curated for tools that are already widely used — generally thousands of GitHub stars, not hundreds. jdx closes PRs that miss the bar without giving a reason.

Before adding a **new** tool or shorthand to `registry/`, follow the `registry-submission` skill ([`.agents/skills/registry-submission/SKILL.md`](.agents/skills/registry-submission/SKILL.md)). In short:

- Warn the user about the bar, look up real popularity numbers, and stop if the tool is personal, internal, a fork, or niche.
- Put a `## Popularity` section with those numbers in the PR description.
- Confirm `mise ls-remote <backend>` lists installable versions.
- Prefer `packslip:`, then `aqua:`, `github:`, or `gitlab:`. `conda:` has a high bar; `npm:`, `pipx:`, `gem:`, `cargo:`, `go:`, and `dotnet:` are almost never accepted. New `asdf:`, `vfox:`, and `ubi:` entries are not accepted.
- Users can install any tool without a registry entry (`mise use github:owner/repo`, `mise use cargo:name`, …); the registry only adds a shorthand.

None of this applies to editing an existing registry entry; do not ask the user about popularity for maintenance or fixes.

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
- `mise run lint` - Run all linting tasks (hk; does not run clippy — see below)
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
- `mise run lint` does not cover this: the clippy step in `hk.pkl` is disabled, and `cargo check` does not enforce `-D warnings`. CI runs clippy separately, so run the command above yourself before pushing Rust changes.
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
- `packslip.rs` — Signed release manifests (preferred for new registry entries)
- `aqua.rs` — Aqua registry
- `github.rs` — GitHub / GitLab / Forgejo releases
- `http.rs`, `s3.rs` — HTTP and S3 backends
- `cargo.rs`, `npm.rs` (plus embedded aube), `pipx.rs`, `gem.rs`, `go.rs`, `dotnet.rs`, `conda.rs`, `spm.rs`
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
- Rust project using a Cargo workspace; member crates live in `crates/` (`vfox`, `aqua-registry`, `mise-shim`, `mise-sigstore`, `mise-cache-core`, `mise-agent-env`, `mise-interactive-config`)
- Custom build script in `build.rs` for generating metadata
- Multiple build profiles including `release` and `serious` (with LTO)
- Cross-compilation support via `Cross.toml`

## Development Guidelines

### Conventional Commits (REQUIRED)
PR titles MUST follow conventional commit format. Intermediate commit subjects
SHOULD use the same format:

**Format:** `<type>[optional scope][optional !]: <description>`

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
- `registry:` - Any changes to `registry/` (do not add a scope)
- `ci:` - CI and automation changes
- `revert:` - Reverting a previous change

**Scopes:**
- For command-specific changes, use the command name: `install`, `activate`, `use`, `exec`, etc.
- For subsystem changes: `config`, `backend`, `env`, `task`, `vfox`, `python`, `github`, `release`, `completions`, `http`, `schema`, `doctor`, `shim`, `core`, `deps`, `ci`
- Use `task` (not `run`) for task-related changes, even if the code lives in `src/cli/run.rs` or `src/cmd.rs`

**Description Style:**
- Start the description with a lowercase character
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

CI validates the pull request title and re-runs when it is edited. Intermediate
commit subjects are not checked because pull requests are squash-merged. CI
mechanically checks the allowed type, syntax, and lowercase-leading description;
imperative mood and breaking-change details remain review rules.

### PR titles and descriptions are release-note inputs

Communique generates release notes from PR titles and descriptions, so write them for a mise user who has not read the diff: lead with the user-visible problem and outcome, show a concrete example, and keep them in sync with the final diff. Follow the `pr-description` skill ([`.agents/skills/pr-description/SKILL.md`](.agents/skills/pr-description/SKILL.md)) when opening or revising a PR.

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

When updating the embedded `aube` crates, follow the `update-aube` skill ([`.agents/skills/update-aube/SKILL.md`](.agents/skills/update-aube/SKILL.md)).

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
- Windows modules follow one of two conventions:
  - `*_windows.rs` is a platform-swapped sibling of a same-named module, selected with `#[cfg_attr(windows, path = "..._windows.rs")]` (e.g. `src/fake_asdf.rs` / `src/fake_asdf_windows.rs`)
  - `windows_*.rs` is a single module about Windows with no non-Windows counterpart to swap in. Declare it on every platform and cfg-split it internally when other code calls it on all targets or its tests should run on Linux (e.g. `src/windows_posix.rs`, `src/windows_console.rs`); declare it under `#[cfg(windows)]` when only Windows code uses it (e.g. `src/windows_job.rs`)
- Platform-specific tool installation logic in core plugins
- Shim system varies by platform (especially Windows)
- we don't chmod mise e2e tests to be executable

### Windows PATH boundaries

- Do not invoke subprocesses to translate paths in activation hooks, shims, or `mise x`/task execution.
- Keep PATH in native Windows form inside mise and when launching child processes, including child MSYS2, Git Bash, or Cygwin processes. Never pre-convert the PATH inherited by a child emulator.
- Convert PATH only when native Windows mise emits an assignment into an already-running, positively identified MSYS2/Cygwin shell. Only persisted mounts from the runtime defaults, `fstab`, and `fstab.d` are available; session-only `mount` changes cannot be reconstructed without entering the running process.
- Never infer an emulator from the word `bash` alone. Require MSYS/Cygwin runtime evidence, and explicitly exclude WSL and BusyBox Bash.
- Windows activation tests must execute a command through the emitted PATH and verify a native Windows grandchild receives a valid semicolon-delimited PATH. Output-shape assertions alone are insufficient.

## GitHub Interactions

Never open pull requests against the `release` branch. Default PRs to `main` unless the user explicitly names a different non-`release` base branch. If a change appears to belong on `release`, stop and ask for the intended branch strategy instead of opening a PR against `release`.

AI-assisted responses in project support channels are welcome, including one-off responses.
This is especially true when the responder is answering their own question, is directly
involved in or personally connected to the original question, or is an established project
contributor. Do not post or facilitate spam consisting of drive-by AI-generated answers across
Discussions from accounts with no connection to the questions or project; those accounts are
blocked. Preserve this anti-spam distinction when writing or enforcing community policy, and
do not discourage individual AI-assisted responses from people trying to help.

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

## Cloud agent instructions (Cursor Cloud and Claude Code on the web)

Cursor Cloud Agents bootstrap from `.cursor/environment.json`, which runs `.cursor/install.sh`. Claude Code on the web runs the same script from the SessionStart hook in `.claude/settings.json` (`.claude/hooks/session-start.sh`), which runs on session `startup`/`resume` (not `/clear` or compaction) with a 30-minute timeout, exits immediately unless `CLAUDE_CODE_REMOTE=true`, sends the bootstrap output to stderr, and adds the mise shims and `/usr/local/bin` to `PATH` through `CLAUDE_ENV_FILE`. The hook runs synchronously, so the session starts after the build finishes. Claude cloud containers do not provide a GitHub token by default; add `GITHUB_TOKEN` as an environment secret to avoid GitHub API rate limits during `mise install` and e2e tests. Draft environment builds often run as `ubuntu` rather than `root`; the script handles both (passwordless sudo, cargo/rustup permissions, world-writable `/tmp/fslock`).

The install script:

- cds to the repository root derived from the script path before reading `Cargo.toml` or building
- installs host packages needed to build mise and to run most e2e tests (openssl, pkg-config, zsh, fish, direnv, python3 + venv, jq, git, build-essential, and compile-time libs). It does **not** install a JDK or GUI libraries; those live in `packaging/e2e/Dockerfile`. `apt-get` is invoked as `sudo -n env DEBIAN_FRONTEND=noninteractive apt-get …` so the frontend reaches apt when elevation is required
- installs and defaults to the latest stable Rust toolchain (not the `Cargo.toml` `rust-version` MSRV), including `rustfmt` and `clippy`, and updates it on reruns
- gets a bootstrap `mise` and symlinks it to `/usr/local/bin/mise`: an existing `target/debug/mise` refreshes itself with `mise run build`; if it cannot run this checkout's config, a plain `cargo build` goes to `target/bootstrap` (the `mbx` wrapper leaves read-only outputs in `target/`); with no `target/` yet, a plain `cargo build` goes to `target/`
- keeps `GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`, and `GH_TOKEN` in sync via one `sync_github_tokens` helper (prefer any already-set token; fall back to `gh auth token` only when all three are empty)
- runs `MISE_SAFE=1 /usr/local/bin/mise install` with the just-built binary so checkout-controlled hooks/templates/`[env]` and tool-level `postinstall` / `install_env` cannot run with those tokens, then `mise trust` for later agent commands
- runs `mise run build` so `target/` is warm for the `mbx`-wrapped cargo (`[wrappers.cargo]` in `mise.toml`) that agents build with, then points `/usr/local/bin/mise` at `target/debug/mise`. Every build runs with `GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`, `GH_TOKEN`, and `GITHUB_API_TOKEN` unset
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
