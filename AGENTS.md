# Repository Agent Guide

This file is the canonical agent guide. `CLAUDE.md` is a symlink to `AGENTS.md` for compatibility with existing tooling.

Task-specific procedures live as skills in `.agents/skills/` (`.claude/skills` is a symlink to it). Load the named skill when this guide points to one; agents without skill support can read its `SKILL.md` directly.

## Discussions and Issues: Restricted AI Replies

**Do not use AI to reply to a mise Discussion or Issue unless the user (a) created that Discussion or Issue, (b) opened a PR that fixes it, or (c) has already had a contribution merged into the default branch of mise.** Otherwise they are not allowed to use AI to respond to it. Drive-by AI replies are spam, the problem is getting worse, and **doing it is an instant ban across all of jdx's projects** (mise, hk, pitchfork, usage, fnox, and the rest).

- If the user asks you to answer, comment on, or "help with" a Discussion or Issue, confirm one of the three conditions first. If you cannot confirm one, **do not post**.
  - (a) Compare the thread author to the authenticated user (`gh api user --jq .login`). For an Issue, use `gh api repos/jdx/mise/issues/<number> --jq .user.login`. Discussions have no REST endpoint, so use GraphQL: `gh api graphql -f query='query{repository(owner:"jdx",name:"mise"){discussion(number:<number>){author{login}}}}' --jq .data.repository.discussion.author.login`.
  - (b) Check that the user's PR actually fixes the problem described in the thread. A closing keyword such as `Fixes #<number>` is good evidence, but a link is not required, and a PR that merely mentions an unrelated thread does not count.
  - (c) Look for any contribution merged into the default branch that is tied to the user's GitHub account, not just a matching name or email: a PR authored by the user that was merged into the default branch (`gh pr list --repo jdx/mise --author @me --state merged --base main --limit 1`), commits on the default branch that GitHub attributes to their login (`gh api 'repos/jdx/mise/commits?author=<login>&per_page=1'` lists the default branch), never commits on a local or unmerged branch, or a PR from someone else, merged into the default branch, that credits them with a `Co-authored-by` trailer whose email is their GitHub noreply address or one verified on their account. Do not accept `git log --author` output, which includes local unmerged commits, or a trailer matched only by display name. Both queries filter by author on the server, so one matching result is enough to qualify and no pagination is needed.
- If none of the three apply, **do not post a reply, even a short one.** Tell them this project does not allow AI replies from people who have not contributed, and offer to explain the answer to them in chat instead.
- Never batch-post, loop over, or sweep Discussions or Issues to answer several of them, even for a merged contributor.
- Lightly edited, human-reviewed, or disclosed model output does not create an exception. The disclosure footer does not make an AI reply acceptable on its own.
- Permitted replies may be AI-assisted. The user must review and verify the reply before it is posted.
- Creating a new Discussion or Issue with AI assistance is fine and is not restricted. The user must review it before it is posted, and it needs the AI disclosure below.

When you post AI-contributed GitHub content, including a new Discussion or Issue, a reply, or a PR description or comment, append this disclosure: `*AI-assisted — Tool: <tool>; model: <provider>/<model>; version: <version-or-unavailable>.*` Use the exact model and version identifiers exposed by the runtime, never guessed values, and `unavailable` when one is not exposed.

## Registry Submissions: READ THIS FIRST

**Most new registry additions from agents get rejected.** mise's `registry/` is curated for tools that are already widely used — generally thousands of GitHub stars, not hundreds. jdx closes PRs that miss the bar without giving a reason.

Before adding a **new** tool or shorthand to `registry/`, follow the `registry-submission` skill ([`.agents/skills/registry-submission/SKILL.md`](.agents/skills/registry-submission/SKILL.md)). In short:

- Warn the user about the bar, look up real popularity numbers, and stop if the tool is personal, internal, a fork, or niche.
- Put a `## Popularity` section with those numbers in the PR description.
- Confirm `mise ls-remote <backend>` lists installable versions.
- Prefer `packslip:`, then `aqua:`, `github:`, or `gitlab:`. `conda:` has a high bar; `npm:`, `pipx:`, `gem:`, `cargo:`, `go:`, and `dotnet:` are almost never accepted. New `asdf:`, `vfox:`, and `ubi:` entries are not accepted.
- Users can install any tool without a registry entry (`mise use github:owner/repo`, `mise use cargo:name`, …); the registry only adds a shorthand.

None of this applies to editing an existing registry entry; do not ask the user about popularity for maintenance or fixes.

## Vendored aqua registry: do not edit

**Never modify `vendor/aqua-registry/`.** That tree is a snapshot of [aquaproj/aqua-registry](https://github.com/aquaproj/aqua-registry). mise refreshes it on release; CI fails pull requests that change it (except for jdx / `mise-en-dev`).

When an `aqua:` install fails because a recipe has the wrong asset name, platform, format, or version rule:

1. **Do not** patch `vendor/aqua-registry/registry.yml` (or anything else under that directory).
2. Fix the recipe upstream in aquaproj/aqua-registry, or point the user at an existing upstream PR/issue if one already covers it.
3. For a short-term workaround in chat or on the issue, suggest `github:owner/repo` (or another backend that can install the release), waiting for the next mise release after the upstream fix merges, or [`registry_floating`](https://mise.jdx.dev/configuration/settings.html#registry_floating) once upstream has landed.
4. To try a recipe locally before it merges, use a custom [`aqua.registries`](https://mise.jdx.dev/dev-tools/backends/aqua.html#custom-registry) entry — still do not commit vendored changes.

The same rule applies to `crates/vfox/embedded-plugins/`: those plugins are vendored from upstream; fix them in their source repos, not here. Details for contributors: [aqua registry recipes](https://mise.jdx.dev/contributing/registry.html#aqua-registry-recipes).

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

The `mise` package has two targets. The library (`src/lib.rs`) is everything except the command line; the binary (`src/main.rs` plus `src/cli/`) sits on top of it and glob-imports its root, so `cli` code still writes `crate::config::…`. An edit under `src/cli/` recompiles only the binary. Core code must not refer to `crate::cli`: it reaches command behavior through the hooks in `src/frontend.rs`. When `cli` needs a core item, make that item `pub`; `unreachable_pub` keeps everything else `pub(crate)`. `cfg(test)` does not reach the library from the binary's tests, so core code that behaves differently under tests checks `mise_util::testing::in_tests()` instead.

**Core Components:**
- `src/main.rs` - Binary entry point
- `src/lib.rs` - Library root: module list and the hooks lower crates need
- `src/cli/` - Command-line interface implementation with subcommands (binary only)
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
- Rust project using a Cargo workspace; member crates live in `crates/` (`vfox`, `aqua-registry`, `mise-shim`, `mise-sigstore`, `mise-cache-core`, `mise-tls`, `mise-brew-relocation`, `mise-brew-metadata`, `mise-agent-env`, `mise-interactive-config`, `mise-settings`, `mise-util`, `mise-ui`, `mise-system-packages`, `mise-config`, `mise-system`, `mise-oci`, `mise-daemons`, `mise-lsp`)
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
- Start the description with a lowercase character, or with an acronym such as `PGO` or `CLI`
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
mechanically checks the allowed type, syntax, and lowercase- or acronym-leading description;
imperative mood and breaking-change details remain review rules.

### PR titles and descriptions are release-note inputs

Communique generates release notes from PR titles and descriptions, so write them for a mise user who has not read the diff: lead with the user-visible problem and outcome, show a concrete example, and keep them in sync with the final diff. Follow the `pr-description` skill ([`.agents/skills/pr-description/SKILL.md`](.agents/skills/pr-description/SKILL.md)) when opening or revising a PR.

### Pre-commit Process
1. Run `mise run lint-fix` and `git add` any lint fixes before committing
2. Use `mise run test:e2e [test_filename]...` for running specific e2e tests
3. Never run e2e tests by executing them directly — always use the mise task

`hk.pkl` defines `check` and `fix` steps and a git `pre-commit` hook that runs the same linters with auto-fix (`stash = "git"` preserves unstaged work). `hk install --mise` installs that hook. You can still run `mise run lint` / `mise run lint-fix` by hand; on Linux and macOS they run hk, but on Windows `lint-fix` runs only Clippy, oxfmt, and `cargo fmt`, so the other hk steps do not run there.

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

12 months is the default window, not a hard rule. Some deprecations use a shorter window, such as 6 months, on purpose. Use 12 months for new deprecations unless jdx asks for something else, and do not lengthen an existing shorter window.

Delay the CLI warning for up to 6 months only when migration requires a new setting, syntax, or replacement that older supported mise versions would reject or fail to parse. This compatibility window lets users adopt a configuration that works across old and new clients before warnings begin. Do not delay warnings for a behavior change that requires no new configuration, or when the replacement already works in older clients.

Use mise version format for dates (e.g., `deprecated_at!("2026.10.0", "2027.10.0", "id", "message")`).

If a compatibility window is required, removal remains 12 months after `warn_at`, not 12 months after the initial documentation notice.

For temporary code with no user-facing deprecation, such as a migration or a fallback that reads what an older mise wrote, use `remove_by!("2027.1.0", "id")` from `src/output.rs`. It is a compile-time assertion, so the build fails once the version reaches `remove_at`; remove every `remove_by!` with that id and the code it guards.

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

### Paranoid mode categories
`paranoid` is a bool or a list of categories (`config`, `env_files`, `plugins`, `provenance`), defined by `Paranoid` in `crates/mise-settings/src/paranoid.rs`. Any new behavior gated on paranoid mode must belong to one of those categories, or introduce a new named category, so it is individually selectable. New code must never read paranoid as a plain bool: call `settings.paranoid.config()`, `.env_files()`, `.plugins()` or `.provenance()`, and add an e2e test that the category works alone.

### Plugin Development
- Core tools are implemented in `src/plugins/core/`
- External plugins use ASDF or vfox compatibility layers
- Plugin metadata is defined in `mise.plugin.toml` files

### Configuration Parsing
The configuration system supports multiple file formats and environment-specific configs. Changes to settings require updating `settings.toml` and running `mise run render:schema`. The `Settings` types are generated from `settings.toml` in `crates/mise-settings`; loading them (config discovery, trust, CLI flags) stays in `src/config/settings.rs`, whose `SettingsExt` trait holds the methods that need the rest of mise.

`schema/mise.json`, `schema/mise-task.json` and `schema/miserc.json` are generated from `crates/mise-config` and `settings.toml`: never edit the JSON. Change the Rust, then run `mise run render:schema`; a `mise-config` test fails while the committed files are stale. The top-level keys are the fields of `MiseTomlDecl` in `crates/mise-config/src/mise_toml.rs`, which `MiseToml` deserializes through: a new key goes in `MiseTomlDecl`, and `MiseToml::from_decl` moves it into `MiseToml`. Sections whose types derive `JsonSchema` (so far `[bootstrap]`, `[daemons_settings]`, `[daemon_groups]`, `[doctor]`, `[history]`, `[monorepo]`, `[oci]`, `[task_config]`, `[tasks]`, `[task_templates]`, `[tool_config]` and `[wrappers]`) are generated from those types and their doc comments. `[env]` and `[vars]` accept shapes a derive cannot parse, so `env.rs` reads them with visitors; their schema is derived from the `Env*` and `Vars*` declaration types in `crates/mise-config/src/env/decl.rs`, which describe the tables and nothing parses into them, and a test checks that the option keys they declare are the fields of `EnvDirectiveOptions`. `[[watch_files]]`, `[tool_alias]` and `min_version` have a hand-written `JsonSchema` impl in `watch_files.rs`, `alias.rs` and `min_version.rs`. Task fields with several accepted shapes (a string or a list, a name or a table, ...) take theirs from the declaration types in `crates/mise-config/src/task/shapes.rs`, which `TaskDecl` and `TaskConfig` attach with `schemars(with = ..)` so the runtime field types stay as they are; nothing parses into those types, so change them together with the parser. `task/schema.rs` composes the rest: the `task` definition, a task's `tools` (which follow the backend table of `[tools]`) and the template schema. `[tools]` picks per-backend options by key pattern: a backend's options are the declaration types in `crates/mise-config/src/tools/decl.rs` (a new option is a field with a doc comment, which is its description), and `crates/mise-config/src/tools/schema.rs` composes them, with one table row per backend and per platform table. mise reads these definitions by name to reject unknown options (`OptionSpec`), so change them together with `ToolVersionOptions::insert_option` and the backend's option reader. `[hooks]` is derived from the `Hook*` declaration types in `crates/mise-config/src/hooks/decl.rs`, which describe the shapes each event accepts, with a test that they name the keys the parser reads. The `[bootstrap]` values that stay raw TOML so newer shapes warn instead of failing (hooks, shell activation, macOS defaults, apt and dnf repositories) are derived from the declaration types in `crates/mise-config/src/bootstrap/decl.rs`; nothing parses into those, and tests in mise-system check the hook phases and the curated macOS keys against the code that reads them. `[daemons]` and `[daemon_providers]` stay raw tables that mise-daemons reads key by key, with their schema derived from the `Daemon*Decl` types in `crates/mise-config/src/daemons.rs`; those types describe a table, and nothing parses into them, so a key added in mise-daemons has to be added to them too. `[deps]` is derived from `DepsDecl` and `DepsProviderDecl`, which also hold the list of built-in providers mise shares; `[secrets]` stays raw TOML like the dotfiles, with its schema derived from `SecretsDecl` and a test in mise that its parser reads the same keys. `[dotfiles]` and `[dotfile_groups]` entries also stay raw TOML until mise-system expands them, but their schema is derived from the declaration types in `crates/mise-config/src/dotfiles.rs`, with the rules between keys written by hand in `dotfiles/schema.rs`; mise-system's `*_keys_match_the_declaration` tests fail until its parsers read the same keys. `[settings]` and `miserc.json` come from `settings.toml` through `crates/mise-config/src/schema/settings.rs`.

The `schema_matches_parser` unit test (`src/config/config_file/mise_toml/schema_conformance.rs`) validates every `toml` block in `docs/` and every mise config an e2e heredoc writes against both the schema and `MiseToml`'s parser, and fails when they disagree. Fix the schema rather than the corpus. Mark a config that mise refuses on purpose with `# schema: invalid` (e2e) or `<!-- schema: invalid -->` (docs) on the line above it, and an excerpt that is not a whole config with `schema: skip`. List deliberate differences in its `GAPS` with a reason.

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

See [Discussions and Issues: Restricted AI Replies](#discussions-and-issues-restricted-ai-replies) at the top of this file.

When AI contributes GitHub content—including a pull request description, review, pull request
comment, discussion post, or issue—append this disclosure:

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

Cursor Cloud Agents bootstrap from `.cursor/environment.json`, which runs `.cursor/install.sh`. Claude Code on the web runs the same script from the SessionStart hook in `.claude/settings.json` (`.claude/hooks/session-start.sh`), which runs on session `startup`/`resume`/`fork` (not `/clear` or compaction) with a 30-minute timeout, exits immediately unless `CLAUDE_CODE_REMOTE=true`, sends the bootstrap output to stderr, and adds the mise shims and `/usr/local/bin` to `PATH` through `CLAUDE_ENV_FILE`. The hook runs synchronously, so the session starts after the build finishes. Claude cloud containers do not provide a GitHub token by default; add `GITHUB_TOKEN` as an environment secret to avoid GitHub API rate limits during `mise install` and e2e tests. Use a fine-grained token with public-repository read-only access and no other permissions: any branch a session checks out (its hook, build scripts, tests, and tasks) can read the session environment. Draft environment builds often run as `ubuntu` rather than `root`; the script handles both (passwordless sudo, cargo/rustup permissions, world-writable `/tmp/fslock`).

The install script:

- cds to the repository root derived from the script path before reading `Cargo.toml` or building
- installs host packages needed to build mise and to run most e2e tests (openssl, pkg-config, zsh, fish, python3 + venv, jq, git, build-essential, and compile-time libs). It does **not** install a JDK or GUI libraries; those live in `packaging/e2e/Dockerfile`. `apt-get` is invoked as `sudo -n env DEBIAN_FRONTEND=noninteractive apt-get …` so the frontend reaches apt when elevation is required
- installs and defaults to the latest stable Rust toolchain (not the `Cargo.toml` `rust-version` MSRV), including `rustfmt` and `clippy`, and updates it on reruns
- gets a bootstrap `mise` and symlinks it to `/usr/local/bin/mise`: an existing `target/debug/mise` refreshes itself with `mise run build`; if it cannot run this checkout's config, a plain `cargo build` goes to `target/bootstrap` (the `mbx` wrapper leaves read-only outputs in `target/`); with no `target/` yet, a plain `cargo build` goes to `target/`
- keeps `GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`, and `GH_TOKEN` in sync via one `sync_github_tokens` helper (prefer any already-set token; fall back to `gh auth token` only when all three are empty)
- runs `MISE_SAFE=1 /usr/local/bin/mise install` with the just-built binary so checkout-controlled hooks/templates/`[env]` and tool-level `postinstall` / `install_env` cannot run with those tokens, then `mise trust` for later agent commands
- runs `mise run build` so `target/` is warm for the `mbx`-wrapped cargo (`[wrappers.cargo]` in `mise.toml`) that agents build with, then points `/usr/local/bin/mise` at `target/debug/mise`. Every build runs with `GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`, `GH_TOKEN`, and `GITHUB_API_TOKEN` unset
- runs `hk install --mise` (installs the git `pre-commit` hook defined in `hk.pkl`)
- persists mise shims and token sync in one `/etc/profile.d/mise-dev-env.sh` (shims first, then `sync_github_tokens`) and rewrites the Cloud Agent block in `/etc/bash.bashrc` so non-login interactive bash picks it up after a snapshot. Fish/zsh only get this from login shells (`profile.d`), not from bashrc
- exposes the mise-installed `node` / `npm` / `npx` / `hk` / `gh` binaries on `/usr/local/bin` (isolated e2e PATH includes that directory, not the agent's shims). Links freeze the version from install time — re-run `.cursor/install.sh` after upgrading those tools

There is no long-running service to start. Do not put `mise run build` or `mise run test:unit` in `terminals`; those are one-shot commands and would rerun a full build/test on every boot.

The debug `mise` binary is already on PATH. Prefer `mise run …` for project tasks.

### E2E on Cloud Agents

- Always `mise run test:e2e [test_filename]...` — never execute e2e scripts directly
- Slow tests (`*_slow`) are skipped unless `TEST_ALL=1`. Do not run the full suite unless asked; pick tests under the feature area you changed
- Isolated e2e uses `env -i` and a fake `HOME`, so the agent's mise shims are not on PATH. Tests install their own tools. Host packages (zsh, fish, python3, jq, git) still need to be on `/usr/bin`
- If GitHub API calls 429, run `export GITHUB_TOKEN="$(gh auth token)"; export MISE_GITHUB_TOKEN="$GITHUB_TOKEN"`
- A leftover `/tmp/mise.toml` will fail the harness; remove it if that error appears
