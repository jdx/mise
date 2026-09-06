---
outline: [2, 3]
---

# Contributing

## Contribution Expectations

mise has a specific scope and design taste. Unless the change is obvious,
start a [discussion](https://github.com/jdx/mise/discussions) or mention what
you plan to do in [Discord](https://discord.gg/mABnUDvP57) before opening a PR.
The important part is to settle the direction before much implementation or
review happens. PRs are often rejected or need to change significantly after
submission, so make sure the idea fits before you invest too much time.

Before I review a PR, CI must be passing, the PR title must follow
[Conventional Commits](#conventional-commits), and all automated AI review
comments must be addressed. If any of those are still open, assume I will wait
to look at the PR.

If I am on the fence about a contribution, I will probably reject it for that
reason alone. If I did not do this, mise would suffer from feature bloat. I
may also reject a PR if the quality is poor enough that I do not have confidence
the contributor can get it across the finish line. I do not have time to coach
contributors.

I get hundreds of PRs per week across my projects, so I do not have time to
respond to every PR with detailed context. A rejection may be brief.

## Development Setup

### Prerequisites

- A Rust toolchain meeting the `rust-version` in the root `Cargo.toml`; the project does
  not declare Rust under `[tools]`, so your selected toolchain must already be suitable.
- A working mise installation meeting `min_version` in `mise.toml`.
- Git and the host dependencies required by the checks you plan to run.

### Getting Started

```bash
# Clone the repository
git clone https://github.com/jdx/mise.git
cd mise

# Install dependencies
mise install

# Build and verify the development binary
mise run build
target/debug/mise --version
```

### Development Shim

The repository adds `target/debug` and `node_modules/.bin` to its task environment. Use
`target/debug/mise` directly to check which binary you are testing. For a recompiling wrapper,
see [Running the CLI](#running-the-cli).

### Cargo build cache

`mise install` installs the mbx version selected by the project. `mise run` activates its
transparent Cargo wrapper, so build and lint tasks invoke ordinary Cargo commands through
the cache. Standalone Cargo commands need an activated mise shell.

If the wrapper fails, run the equivalent Cargo check with `MBX_DISABLE=1`; this bypasses
the cache without skipping validation:

```sh
MBX_DISABLE=1 cargo build --all-features
MBX_DISABLE=1 cargo test --all-features
MBX_DISABLE=1 cargo check --all-features
```

If bypassed Cargo succeeds, report the mismatch in a
[mr-boxington discussion](https://github.com/jdx/mr-boxington/discussions) with repository and
commit, OS, `mbx --version`, `mbx doctor`, and both commands/output. Redact secrets, absolute
cache paths, remote URLs, namespaces, and identifying details before posting.

## Pull Request Checklist

1. **Discuss first**: Use GitHub Discussions or Discord for non-obvious changes
2. **Use a conventional title**: PR titles are validated automatically
   - For new tools in registry: Use `registry: add tool-name (backend:full/name)`
3. **Run local checks**: Run `mise run render` and `mise run lint-fix` before
   opening a PR when relevant
4. **Test thoroughly**: Ensure the relevant unit and E2E tests pass
5. **Update documentation**: Add or update docs for user-facing changes
6. **Keep dependencies healthy**: New dependencies are validated with cargo-deny

### Development Tips

Use the project tasks for the supported environment and the development binary at
`target/debug/mise` when verifying a change. Read [Development Setup](#development-setup)
first, then run focused checks for the feature you changed. Set `MISE_DEBUG=1` or
`MISE_TRACE=1` when diagnosing CLI behavior.

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

Use `mise tasks ls` to list tasks and `mise tasks info <name>` to see their resolved source.
This repository loads both `tasks.toml` and file tasks under `xtasks`; a file task can replace
a same-named TOML task, so inspect the resolved task when behavior is surprising.

### Common Tasks

- `mise run build` - Build the project
- `mise run test:unit` followed by `mise run test:e2e --all` - Explicitly run unit and E2E suites
- `mise run test:unit` - Run unit tests only
- `mise run test:e2e <test-pattern>` - Run selected E2E tests
- `mise run lint` - Run linting
- `mise run lint-fix` - Run linting with fixes
- `mise run format` - Format code
- `mise run clean` - Clean build artifacts
- `mise run snapshots` - Update test snapshots
- `mise run render` - Generate documentation and completions

### Documentation Tasks

- `mise run docs` - Start documentation development server
- `mise run docs:build` - Build documentation
- `mise run render:usage` - Generate CLI reference documentation
- `mise run render:completions` - Generate shell completions

### Release Tasks

- `mise run release-plz` - CI-only release automation; do not run locally
- `mise run ci` - Run CI tasks (format, build, test)

## Setup

After installing prerequisites, `mise run build` and `target/debug/mise --version` establish
that the selected toolchain can build and run the checkout. Run feature-specific tests next.

## Running the CLI

I use the following shim in `~/.local/bin/@mise`:

```sh
#!/bin/sh
exec cargo run -q --all-features --manifest-path "$HOME/src/mise/Cargo.toml" -- "$@"
```

::: info
Don't forget to change the manifest path to the correct path for your setup.
:::

Make the wrapper executable and put its directory on PATH. `@mise` recompiles the checkout
when needed. Use a disposable shell when testing development activation so a broken hook
does not interfere with your normal shell.

```sh
@mise --help
eval "$(@mise activate zsh)"
@mise activate fish | source
```

## Pre-commit Hooks & Code Quality

### hk Configuration

[`hk.pkl`](https://github.com/jdx/mise/blob/main/hk.pkl) defines `check` and `fix` workflows.
It currently has no Git `pre-commit` hook, so `hk install --mise` may report that there is
nothing to install. Run the checks explicitly before committing.

### Available Linters in hk

The configured steps include Prettier, Markdown linting, Cargo formatting/checking,
ShellCheck, shfmt, Pkl, TOML/schema validation, Lua checks, and actionlint. The Clippy block
in `hk.pkl` is disabled; CI runs Clippy separately. Read the current configuration rather
than assuming a successful hk run includes every Rust lint.

### Using hk in Development

```sh
mise run lint
mise run lint-fix
```

Review and stage the fixes before committing. Do not add Clippy exclusions to make a check
pass; refactor the code so the applicable lint succeeds.

### Running Checks Manually

For an effect-aware check scoped to changed files:

```sh
mise exec hk -- hk run check --safe --format json
```

To check an exact file list, pass NUL-delimited paths with `--files0-from`. Check hk's reported
results: a skipped step is not a passed step. The project tasks remain the normal entry point
for the full configured workflows.

## Testing

Choose checks that demonstrate the changed behavior. Use unit tests for local parsing and
resolution, E2E tests for commands and shell boundaries, and snapshots for output that
needs a stable contract. Avoid live downloads when a local fixture can cover the behavior.

### Unit Tests

```sh
mise run test:unit
cargo test --all-features test_name
cargo test --all-features module_name -- --nocapture
```

Standalone Cargo commands need the activated development environment described in
[Cargo build cache](#cargo-build-cache).
The main binary's test initialization sets fixture paths and shared process state;
`.cargo/config.toml` and the task configure `RUST_TEST_THREADS=1`. Use existing environment
and current-directory guards when changing shared state in a test.

For the Lua runtime crate, use `mise --cd crates/vfox run test`. Tests and fixtures there
exercise hook return values and built-in modules independently of the CLI adapter.

### E2E Tests

Always use the mise task, which builds the project and invokes the test wrapper:

```sh
# A concrete test path or a regex matching test basenames
mise run test:e2e e2e/cli/test_version
mise run test:e2e '^test_use$'
mise run test:e2e '^test_task_'

# Inspect available files or run the complete suite
mise run test:e2e --list
mise run test:e2e --all
TEST_ALL=1 mise run test:e2e --all
```

The wrapper matches **basenames** after stripping a supplied path. A directory such as
`e2e/tasks` is not a directory filter. Use a concrete test file or a filename pattern and
check which tests ran. With no arguments, the current wrapper selects no files; use
`--all` explicitly for the full suite. `*_slow` files require `TEST_ALL=1`.

The harness creates isolated mise configuration, data, state, and working directories.
It still needs host prerequisites such as the shell under test, compilers, or a running
service. Let the harness handle cleanup. Do not execute files under `e2e/` directly or
change their executable bit to run them.

Supply `MISE_GITHUB_TOKEN` or `GITHUB_TOKEN` when a test needs GitHub API access. Avoid
printing credentials in debug output or failure reports.

### Coverage Tests

`mise run test:coverage` is the CI-oriented setup/E2E runner in `xtasks/test/coverage`.
Coverage instrumentation is supplied by its environment; running it locally by itself does
not create an instrumented build. The full runner supports `TEST_TRANCHE` and
`TEST_TRANCHE_COUNT` for partitioning tests.

### Windows E2E Tests

Install the Pester module in PowerShell and build the Windows binary first. The runner adds
`target/debug` to PATH:

```powershell
pwsh -File e2e-win/run.ps1
pwsh -File e2e-win/run.ps1 -TestName '*task*'
```

The filter matches Pester test names. Tests for activation and PATH should execute a child
command and verify its behavior, including a native Windows grandchild where relevant.

### Plugin Tests

`mise test-tool` tests **registry tools**, including tools using built-in backends; it is not
limited to plugins. It performs real installations and runs the entry's configured test:

```sh
mise test-tool ripgrep
mise test-tool ripgrep jq
mise test-tool ripgrep --raw
```

Use `--all` only when you intend to test the whole registry, and `--all-config` for configured
tools. These can involve many downloads, host dependencies, and long builds. Plugin authors
should also read [Plugin Publishing](/plugin-publishing.html#testing-before-publication).

### Test Environment Setup

Use the supported task/harness instead of setting `MISE_DISABLE_TOOLS=1` or broad trust
paths in your normal shell. Those settings can hide the very integration the test should
exercise. A test that invokes a host package manager must isolate its host-managed state
separately from mise's directories.

### Test Assertions

`e2e/assert.sh` provides exact-output, substring, failure, JSON, and filesystem helpers.
For example, inside an E2E test:

```sh
assert "mise exec -- printf '%s' hello" "hello"
assert_contains "mise --version" "mise"
assert_fail "mise definitely-not-a-command"
```

`assert_fail "command" "substring"` checks both failure status and an output substring.
`assert_fail_contains` requires a message to check; `assert_fail_matches` checks a regular
expression. Choose the helper that expresses the behavior you need to verify.

### Running Specific Test Categories

Run `mise run test:unit` and a focused E2E selection while developing. For an explicit full
local run, use `mise run test:unit` followed by `mise run test:e2e --all`. The aggregate
`test` task currently invokes the E2E wrapper without a selection, so do not infer E2E
coverage from that task's success alone.

`mise run test:shuffle` requires nightly Rust and tests order sensitivity. Use a command-local
toolchain selection; do not change your global Rust default just to run one check.

### Running Individual Tests

Use Cargo's name filter for a unit test and the E2E basename patterns shown above for a CLI
test. Confirm the output reports the intended test count; a successful command with zero
matching tests has not verified the change.

### Performance Testing

`mise run test:perf` prepares a workspace and runs the performance scripts. Read the script's
host-tool requirements before using it on macOS. The separate `mise run perf` task uses tak;
keep the build profile and runner class consistent when comparing results.

### Snapshot Testing

Run `mise run snapshots` when expected output intentionally changes. Review the resulting
`.snap` diff, including removed snapshots; do not accept snapshots as a substitute for
checking the behavior that produced them.

## Generating readme and shell completion files

Edit source inputs, then regenerate the outputs affected by your change:

| Change | Source and generation |
| --- | --- |
| CLI help or arguments | `src/cli` → `mise run render:usage`; regenerate completions when command structure changes |
| Settings | `settings.toml` → `mise run render:schema` |
| All generated docs and completions | `mise run render` |
| Docs website | Edit Markdown/Vue sources; run `mise run docs:build` |
| Documentation index for agents | `mise exec bun -- bun docs/.vitepress/llms.ts` after the final docs changes |

CLI pages under `docs/cli` are generated. Do not patch those files without changing their
source or generator. Docs examples use **TOML 1.1**; multiline inline tables, comments, and
trailing commas are valid. Use a compatible parser when validating examples.

Before opening a docs PR, rebase on the current `main`, resolve source conflicts, and rebuild
`docs/public/llms.txt` from the rebased tree. Build the website to catch Markdown/Vue and
link errors. Commit required generated changes with the source changes that produced them.

## Dependency Management

mise uses several tools to validate dependencies and code quality:

- **cargo-deny**: Validates licenses, security advisories, and dependency
  duplicates
- **cargo-msrv**: Verifies minimum supported Rust version compatibility
- **cargo-machete**: Detects unused dependencies in Cargo.toml

CI installs these tools separately; they are not all declared in the project's `mise.toml`.
Install the required tool before running its check locally. Consult the
[test workflow](https://github.com/jdx/mise/blob/main/.github/workflows/test-impl.yml) for
the exact CI environment and flags:

```bash
# Run the installed dependency-check tools
cargo deny check
cargo msrv verify
cargo machete --with-metadata
```

## Conventional Commits

mise uses [Conventional Commits](https://www.conventionalcommits.org/) for
PR titles and automated changelog generation. PR titles **must** use this format;
intermediate commit subjects should follow it too:

```text
<type>[optional scope][optional !]: <description>

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
- **ci**: CI and automation changes
- **security**: Security-related changes
- **registry**: Registry changes (without a scope)
- **revert**: Reverting previous changes (◀️ Revert)

### Examples

```text
feat(cli): add new command for listing plugins
fix(parser): handle edge case in version parsing
refactor(config): simplify configuration loading logic
docs(readme): update installation instructions
test(e2e): add tests for new plugin functionality
chore(deps): update dependencies to latest versions
```

Start the description with a lowercase character and use an imperative verb. Use `docs:`
for documentation changes and `fix:` for CLI behavior fixes, not CI or infrastructure.

### Scopes

Common scopes used in mise:

- `cli` - Command line interface changes
- `config` - Configuration system changes
- `task` - Task runner changes (use `task`, not `run`)
- `backend` - Tool backend changes
- `ci` - CI / Cloud Agent / infrastructure
- `deps` - Dependency updates
- `security` - Security-related changes
- `registry` - Registry entries (usually used as the **type**, not a scope)

### Breaking Changes

#### Breaking Change Policy

Breaking changes are rarely accepted into mise and are only performed in
exceptional situations where there is no better alternative. When a breaking
change is necessary, the process includes:

1. Mark the feature deprecated in documentation immediately and normally add a CLI warning
   with `deprecated_at!` in the same release.
2. Allow 12 months after the warning before removal.
3. Delay the warning by up to 6 months only when migration requires a new setting, syntax,
   or replacement that older supported clients reject. Removal remains 12 months after
   the warning, not after the initial documentation notice.
4. Provide a working migration path and explain the affected behavior.

For breaking changes, add `!` after the type or include `BREAKING CHANGE:` in
the footer:

```text
feat(api)!: remove deprecated configuration options
# OR
feat(api): remove deprecated configuration options

BREAKING CHANGE: The old configuration format is no longer supported
```

## CI/CD & Pull Request Automation

mise uses several automated workflows to maintain code quality and streamline
development:

### Formatting and Linting

- Run `mise run render` and `mise run lint-fix` before opening a PR
- Generated docs, completions, and snapshots should be committed with the
  change that requires them
- The contributor is responsible for fixing formatting or lint failures

### PR Title Validation

- **semantic-pr-lint**: Validates that PR titles follow the conventional commit format
- PR titles must match: `<type>[optional scope][optional !]: <description>`
- Example: `feat(cli): add new command for listing plugins`

### Continuous Integration

- **Cross-platform testing**: Ubuntu, macOS, and Windows
- **Unit tests**: Fast component-level tests
- **E2E tests**: Full integration testing with multiple test tranches
- **Dependency validation**: `cargo deny`, `cargo msrv`, `cargo machete`

### Release Automation

- **release-plz**: Automated release management based on conventional commits
- Automatically creates release PRs and publishes releases
- Runs on every push to `main` and daily via scheduled workflow
- Handles version bumping and changelog generation
- The release PR's dry run (`release.yml`) only builds the release tarballs
  once auto-merge is enabled on that PR. Until then its required `release`
  check fails with a message saying so, which keeps the PR from merging
  without a dry run while avoiding a full tarball build on every push to
  `main`.

## Adding a new setting

To add a new setting, add it to
[`settings.toml`](https://github.com/jdx/mise/blob/main/settings.toml) in the
root of the project and run `mise run render` to update the codebase.

## Adding Tools

Adding tools to mise involves adding a TOML file to the
[registry/](https://github.com/jdx/mise/blob/main/registry/) directory. This
allows users to install tools using short names like `mise use ripgrep` instead
of the full backend specification.

### Quick Start

First check the popularity requirements below. A new shorthand is for an already widely
used tool, not a way to make a personal or niche project installable. Explicit backend
syntax works without a registry entry.

1. **Choose the right backend** for your tool:

   - **[packslip](dev-tools/backends/packslip.md)** - Preferred when the project
     publishes signed release manifests
   - **[aqua](dev-tools/backends/aqua.md)** - Curated metadata and security
     features for tools without packslips
   - **[github](dev-tools/backends/github.md)** - Simple GitHub releases following
     standard conventions
   - **[gitlab](dev-tools/backends/gitlab.md)** - Tools released through GitLab
   - **Language package managers** - `npm`, `pipx`, `cargo`, `gem`, etc. for
     ecosystem-specific tools
   - **[Core tools](core-tools.md)** - Built-in support for major languages
     (not user-contributed)

2. **Add to registry/**:

   ```toml
   version_order = "semver"
   description = "Brief description of the tool"
   backends = ["packslip:github.com/owner/repo", "aqua:owner/repo", "github:owner/repo"]
   bins = ["your-tool"]
   test = { cmd = "your-tool --version", expected = "{{version}}" }
   ```

3. **Verify version listing** with `mise ls-remote <backend:identifier>`. A backend that can
   install only an explicitly pinned version is insufficient.
4. **Test the tool** with `mise test-tool your-tool` to confirm installation and execution.

### Guidelines and Requirements

When adding a new tool, the following requirements apply:

- **A test is required in `registry/`** - Must include a `test` field to
  verify installation. This is automatically enforced by the
  [`validate-new-tools` job](https://github.com/jdx/mise/blob/main/.github/workflows/registry.yml)
  in the registry workflow.
- **New tools must already be widely used** - The bar is normally thousands of GitHub stars,
  active maintenance, and real use outside the author's projects. Personal, internal, niche,
  and low-popularity tools do not meet it. A working installer or passing test is not enough.
  @jdx won't explain why a given tool wasn't accepted.
- **Include popularity evidence** - Put current stars/forks, release activity, relevant
  package downloads, and examples of third-party use in the PR description. Check the actual
  numbers before proposing an entry; do not submit speculatively.

#### Backend acceptance tiers

Which backend you choose for a registry entry matters as much as which tool you
add. Backends fall into the following tiers:

**Tier 1 — preferred, routinely accepted:** [`packslip`](/dev-tools/backends/packslip.html).

Use `packslip` when the project publishes signed release manifests. mise verifies
the signer and artifact digests without a plugin or separate package manager.

**Tier 2 — routinely accepted:** [`aqua`](/dev-tools/backends/aqua.html),
[`github`](/dev-tools/backends/github.html), and [`gitlab`](/dev-tools/backends/gitlab.html).

- When the project does not publish packslips, prefer `aqua` if the tool is in the [aqua registry](https://github.com/aquaproj/aqua-registry) —
  it has better UX, SLSA verification, and per-version logic.
- Use `github` when the tool isn't in aqua but ships GitHub releases.
- Use `gitlab` for tools released through GitLab.

**Tier 3 — high bar, but lower than tier 4:** [`conda`](/dev-tools/backends/conda.html).

Potentially accepted for tools that can't reasonably be supported via packslip/aqua/github/gitlab.
The bar is lower than tier 4 because **mise's conda backend does not require a
separately-installed package manager** — packages are downloaded and extracted
directly from anaconda.org, with no `conda`/`mamba`/`micromamba` needed on the
user's PATH. The tool still needs to be popular and well-maintained.

**Tier 4 — very high bar, rarely accepted:** `npm`, `pipx`, `gem`, `cargo`, `go`, `dotnet`.

Runtime and toolchain dependencies add setup and reproducibility constraints. For example,
npm requires Node, and gems depend on their Ruby installation. Requirements differ by
backend: pipx's default uv mode can provision Python, so consult the backend's guide rather
than assuming every dependency must already be on PATH. These backends are accepted only
when no packslip/aqua/github/gitlab option exists and the tool is widely used. Get explicit
agreement from @jdx before submitting an entry using one of these backends.

**Not accepted:** `asdf`, `vfox`, `ubi`.

- **New `asdf` plugins** — rejected for supply-chain security reasons. Use [packslip](/dev-tools/backends/packslip.html), [aqua](/dev-tools/backends/aqua.html), [github](/dev-tools/backends/github.html), or [gitlab](/dev-tools/backends/gitlab.html) instead.
- **New `vfox` plugins** — same reason. Use packslip/aqua/github/gitlab instead.
- **`ubi`** is deprecated and is not accepted for new registry entries.

Users can still install via any backend themselves with explicit syntax
(`mise use vfox:owner/repo`, `mise use cargo:name`, etc.) — they just don't get
a registry shorthand for it.

### Registry Format

Each `registry/<tool>.toml` file uses this format:

```toml
# Tool name "your-tool" (becomes the short name for `mise use`)
version_order = "semver"
description = "Tool description"
backends = [
    "packslip:github.com/owner/repo", # Preferred when the project publishes packslips
    "aqua:owner/repo",               # Fallback backend
    "github:owner/repo",             # Fallback backend
]
bins = ["your-tool"]
test = { cmd = "your-tool --version", expected = "{{version}}" }
aliases = ["alt-name"] # Optional alternative names
os = ["linux", "macos"] # Optional OS restrictions
```

Only list backends that support the tool: `packslip` requires signed release
manifests, and `aqua` requires an entry in the aqua registry.

Every registry entry must explicitly set `version_order` to `semver` or
`source`. Use `semver` only when the tool's stable releases consistently use
strict `MAJOR.MINOR.PATCH` semantic versions. Use `source` for date versions,
two-component versions, channels, refs, tool-specific formats, mixed histories,
or whenever the convention is uncertain. Semantic ordering currently affects
the Aqua, GitHub, GitLab, Forgejo, and HTTP backends; the field still documents
the policy for tools whose current backend owns version ordering itself.

Set `bins` to the tool's executable names so mise can create shims for
[lazy installation](/dev-tools/shims.html#lazy-tools) before downloading the tool. When
`packslip` or another non-Aqua backend is first, mise cannot infer these names
from the registry entry; list them explicitly as in the examples above.

When `aqua` is the first backend, mise derives the command names from the Aqua
registry's file metadata. Omit `bins` when that inferred list is correct. Set it
explicitly when the shorthand needs a different backend-independent command set,
such as commands bundled by a fallback backend that Aqua does not describe.

#### Minimum backend versions

When a backend supports only newer releases, set `min_version` on that backend.
For example, hk publishes Packslip manifests starting at 1.58.1:

```toml
version_order = "semver"
backends = [
  { full = "packslip:github.com/jdx/hk", min_version = "1.58.1" },
  "aqua:jdx/hk",
]
bins = ["hk"]
```

The minimum is inclusive and must be a complete semantic version. It is only
supported for registry tools with `version_order = "semver"`; do not add it to
tools with opaque or source-ordered versions. `mise use hk@1.57` and
`mise use hk@1.58.0` select Aqua, while `mise use hk@1.58.1` selects Packslip.
A prefix overlapping the boundary, such as `1.58`, keeps the preferred backend.
`latest`, channels, and unresolved aliases retain normal backend priority;
aliases are checked again after resolution.

Selection still respects platform support and disabled backends. Explicit
backend identifiers, backend overrides, and a matching lockfile's recorded
backend remain authoritative. A failed download or signature verification does
not trigger fallback. A backend without `min_version` has no lower bound.

#### Idiomatic version files

Registry tools can opt into [idiomatic version files](/configuration.html#idiomatic-version-files)
with `idiomatic_files`. A filename string uses mise's default plain-text parser:

```toml
backends = ["aqua:owner/repo"]
idiomatic_files = [".your-tool-version"]
```

For structured or tool-specific files, use a table with the same parsing options supported by the
[HTTP backend's version listing](/dev-tools/backends/http.html#version-list-url):

```toml
idiomatic_files = [
  { path = "your-tool.json", version_json_path = ".toolchain.version" },
  { path = "your-tool.conf", version_regex = 'version\s*=\s*"([^"]+)"' },
]
```

The supported parser fields are:

- `version_regex`: extract every regex match, using the first capture group when present.
- `version_json_path`: extract values using mise's jq-like JSON path syntax.
- `version_expr`: extract or post-process versions using an
  [expr-lang](https://expr-lang.org/) expression. The original contents are available as `body`,
  and versions produced by `version_regex` or `version_json_path` are available as `versions`.

These parsers are evaluated in-process and cannot run shell commands. Plain string entries remain
compatible with existing registry entries and backend-native parsers.

Only extract a value that states the version the project is built with. Good candidates are an
exact version or a configuration-format major that is intentionally coupled to the CLI major. Do
not extract a **minimum compatible version** — a floor such as `cmake_minimum_required` or
`package.json`'s `engines` describes what a consumer needs, not what the project is developed
against, and resolving it pins users to the oldest supported release (see
[which fields mise reads](/configuration.html#which-fields-mise-reads)).
Also do not extract unrelated project versions, dependency versions, lockfile schema revisions, or
generic `version` fields that do not constrain the tool itself.

An existing entry that reads a floor can be retired with `deprecated = "<reason>"` on the file,
which keeps it resolving while warning users to move the version into `mise.toml`.

Include all filenames that the tool officially searches, including documented nested paths such as
`.config/tool.yml`. When suffixes overlap, mise uses the most specific matching path.

Idiomatic files are disabled by default. Users enable them for a registry shorthand with:

```sh
mise settings add idiomatic_version_file_enable_tools your-tool
```

### Backend Priority

List backends in order of preference. Users get the first available backend
but can override it with explicit syntax such as `mise use aqua:owner/repo`.
Only include `npm` as a fallback for a tool that already has a non-npm primary
backend when the npm package works with lifecycle scripts disabled.

### Tool Testing

All tools must include a test to verify proper installation:

```toml
test = { cmd = "command-to-run", expected = "expected-output-pattern" }
```

The test command should be reliable and verify the installed executable. The template
<code v-pre>{{version}}</code> expands to the selected tool version; it is not a wildcard
matching any version. Use it when the command prints that version, or choose another stable
output check appropriate for the tool.

If `test.cmd` needs extra mise-managed tools on PATH, declare them with
`test.tools`. This is used only by `mise test-tool`; it does not affect normal
tool installation.

```toml
test = { cmd = "gradle -V", expected = "Gradle", tools = ["java"] }
```

### Registry Examples

Examples of registry shapes (consult the current files for all fields):

- **DuckDB**: Aqua backend ([#4248](https://github.com/jdx/mise/pull/4248))

  ```toml
  # registry/duckdb.toml
  version_order = "semver"
  backends = ["aqua:duckdb/duckdb"]
  test = { cmd = "duckdb --version", expected = "{{version}}" }
  ```

- **Biome**: Multiple backends ([#4283](https://github.com/jdx/mise/pull/4283))

  ```toml
  # registry/biome.toml
  version_order = "semver"
  backends = ["aqua:biomejs/biome", "npm:@biomejs/biome"]
  test = { cmd = "biome --version", expected = "Version: {{version}}" }
  ```

## Adding Backends

:::warning Backend vs Tool Confusion
**Most contributors want to add tools, not backends.** Before reading this
section, make sure you actually need a new backend. Tools are individual
software packages (like `node` or `ripgrep`), while backends are installation
mechanisms (like `aqua` or `github`). If you want to add a specific tool to mise,
see [Adding Tools](#adding-tools) instead.
:::

:::warning Core Backend Acceptance Policy
**New backends are unlikely to be accepted into mise core.** They require
a lot of maintenance, so it's generally better to use the
[backend plugin system](backend-plugin-development.md) to add backends without
core changes. A new backend would be accepted only for a major package manager
or tool that would greatly enhance mise's capabilities.

If you need a custom backend:

1. **Discuss with jdx first** in [Discord](https://discord.gg/mABnUDvP57) or by
   creating a [discussion](https://github.com/jdx/mise/discussions)
2. **Consider whether existing backends** (github, aqua, npm, pipx, etc.) can meet
   your needs
3. **Create a plugin** - use the [plugin system](tool-plugin-development.md) to create plugins for private/custom tools without core changes. Start with the [mise-tool-plugin-template](https://github.com/jdx/mise-tool-plugin-template) for a quick setup

Most tool installation needs can be met by existing backends, especially
[github](dev-tools/backends/github.md) for GitHub releases and
[aqua](dev-tools/backends/aqua.md) for comprehensive package management.
:::

Backends are mise's abstraction for different tool installation methods. Each
backend implements the `Backend` trait to provide consistent functionality
across different installation systems.

### Backend Types

- **Core Tools** (`src/plugins/core/`) - Built-in language runtimes like
  Node.js, Python, Ruby
- **Package Manager Backends** (`src/backend/`) - npm, pipx, cargo, gem, go
  modules
- **Universal Installers** (`src/backend/`) - github, aqua for GitHub releases and
  package management
- **Plugin Backends** (`src/backend/`) - plugins can provide custom backends or individual tools

### Implementation Steps

1. **Create the backend module** in `src/backend/` (e.g., `my_backend.rs`)

2. **Implement the current Backend trait** in
   [`src/backend/mod.rs`](https://github.com/jdx/mise/blob/main/src/backend/mod.rs).
   Follow a nearby backend with the same installation model. Shared wrapper methods handle
   caching and policy; implement the appropriate hooks, such as `_list_remote_versions`
   (which returns `VersionInfo` entries) and `install_version_`, rather than duplicating the
   wrapper logic. Preserve opaque versions and delegate resolution to the backend.

3. **Register the backend** in `src/backend/mod.rs`:

   - Add your backend to the imports
   - Add it to the backend registry/factory function
   - Add the `BackendType` enum variant

4. **Add CLI argument parsing** in `src/cli/args/backend_arg.rs` if needed

5. **Update the registry** in `registry/` if it should be available as a
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

- `src/backend/github.rs` - Simple GitHub release installer
- `src/backend/npm.rs` - Package manager integration
- `src/plugins/core/node.rs` - Full language runtime implementation

For detailed architecture information, see
[Backend Architecture](dev-tools/backend_architecture.md).

## Packaging and Self-Update Instructions

When mise is installed via a package manager, `mise self-update` should not replace the binary the package manager owns; users should update through the package manager instead. This is opt-in: a package that does none of the following keeps self-update fully enabled. Packagers have three ways to turn it off, and any of them makes `mise doctor` report `self_update_available: no`.

The paths below are relative to the install prefix, which mise derives from its own binary: the path is canonicalized (symlinks resolved) and then taken two levels up, so `/usr/bin/mise` gives `/usr`.

### Disable at build time

Build without the `self_update` Cargo feature. This example retains native TLS and bundled
Lua; a package using system Lua should choose its features and build dependencies accordingly:

```bash
cargo build --release --no-default-features --features native-tls,vfox/vendored-lua
```

The subcommand still exists, so scripts that call it get a clear error rather than "unknown command", but it always fails with `mise's self-update feature has been disabled at build time, cannot update`.

### Disable with a marker file

Install an empty `.disable-self-update` file at any one of:

- `lib/.disable-self-update` (used by Homebrew)
- `lib/mise/.disable-self-update` (used by the AUR `mise-bin` package)
- `lib64/mise/.disable-self-update`

### Ship update instructions

Installing a TOML file with platform-specific instructions also disables self-update; mise prints the file's message when `mise self-update` runs and when it detects a newer release. Install it at any one of:

- `lib/mise-self-update-instructions.toml`
- `lib/mise/mise-self-update-instructions.toml`
- `lib64/mise/mise-self-update-instructions.toml`

Example contents:

```toml
# Debian/Ubuntu (APT)
message = "To update mise from the APT repository, run:\n\n  sudo apt update && sudo apt install --only-upgrade mise\n"
```

```toml
# Fedora/CentOS Stream (DNF)
message = "To update mise from COPR, run:\n\n  sudo dnf upgrade mise\n"
```

Setting `MISE_SELF_UPDATE_INSTRUCTIONS` to a file path overrides the search.

### Overriding the outcome

`MISE_SELF_UPDATE_AVAILABLE=false` disables self-update without installing anything, and `MISE_SELF_UPDATE_AVAILABLE=true` re-enables it even when a marker or instructions file is present. Both are useful for testing a package build. Neither has any effect on a binary built without the `self_update` feature, where self-update is always unavailable.

`mise self-update --force` also bypasses the availability check, so a user who passes it updates the binary in place even when a marker file, an instructions file, or `MISE_SELF_UPDATE_AVAILABLE=false` is in effect. Treat the runtime mechanisms as "do not update by default" rather than a hard block. A build without the `self_update` feature is the only variant `--force` cannot get past.

## Testing packaging

Test packaging changes in a disposable container or machine for the target distribution.
A running Docker engine is required for the examples below. Start the container from your
host shell, then run the installation commands **inside** it as root. These checks exercise
the published repository; testing an unpublished package also requires copying that artifact
into the container and installing it there.

### Ubuntu (apt)

```sh
docker run -ti --rm ubuntu bash
```

Inside the container:

```sh
apt update -y
apt install -y curl ca-certificates
install -dm 755 /etc/apt/keyrings
curl -fSso /etc/apt/keyrings/mise-archive-keyring.asc https://mise.jdx.dev/gpg-key.pub
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.asc arch=$(dpkg --print-architecture)] \
https://mise.jdx.dev/deb stable main" >/etc/apt/sources.list.d/mise.list
apt update -y
apt install -y mise
mise --version
```

### Fedora (dnf)

```sh
docker run -ti --rm fedora bash
```

Inside the container, follow the [Fedora installation instructions](/installing-mise.html#dnf),
then run `mise --version`. Minimal images may require the distribution's DNF COPR plugin first.

### RHEL (dnf)

```sh
docker run -ti --rm registry.access.redhat.com/ubi9/ubi:latest bash
```

Inside the container, follow the [RHEL installation instructions](/installing-mise.html#dnf),
then run `mise --version`. RHEL 9 uses the `centos-stream+epel-next-9` COPR target; do not
assume that a generic COPR enable command selects an available build for every release.

## Linting

- Lint codebase: `mise run lint`
- Lint and fix codebase: `mise run lint-fix`

## Releasing

Releases are cut automatically by the `release-plz` GitHub Actions workflow
(`mise run release-plz` in CI). Do not run that task locally.
