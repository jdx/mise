---
description: "Propose, build, test, and submit changes to mise, from a first checkout to CLI help, settings, and docs."
outline: [2, 3]
---

# Contributing

mise welcomes bug fixes, documentation improvements, and registry entries for
widely used tools. Agree on the direction of anything larger before you write
code. Report bugs in [Issues](https://github.com/jdx/mise/issues), and bring
questions and ideas to [Discussions](https://github.com/jdx/mise/discussions).

## Community participation {#community-participation}

::: danger AI replies to Discussions and Issues are restricted
Breaking this rule is **an instant ban across all of jdx's projects**.

You may only use AI to reply to a [Discussion](https://github.com/jdx/mise/discussions) or
[Issue](https://github.com/jdx/mise/issues) if you created it, you opened a PR that fixes it, or
you have already had a contribution, attributed to your GitHub account, merged into the default
branch of mise. Everyone else is not allowed to use AI to reply. Drive-by AI replies are spam, and
this is a growing problem.

This includes raw, lightly edited, reviewed, and disclosed model output. Adding an "AI-assisted"
footer does not make an AI reply acceptable on its own. If you are running an agent, make sure it
does not post to threads you are not allowed to reply to, and never let it sweep through many
threads at once.
:::

Using AI to help write and file your own Discussion, Issue, or pull request is
fine. Review it before you post it, and end it with a disclosure line in this
form:

```text
*AI-assisted — Tool: <tool>; model: <provider>/<model>; version: <version-or-unavailable>.*
```

If you are allowed to use AI to reply to a thread, review and verify the reply
before you post it, and disclose it the same way.

## Before you open a pull request {#before-you-open-a-pull-request}

mise has a specific scope and design taste. Unless a change is obvious, propose
it in a [Discussion](https://github.com/jdx/mise/discussions) or on
[Discord](https://discord.gg/mABnUDvP57) before you write it. PRs that skip this
step are often rejected or need to change significantly.

If I am on the fence about a contribution, I will probably reject it for that
reason alone; otherwise mise would suffer from feature bloat. I may also reject
a PR if its quality does not give me confidence that the contributor can get it
across the finish line. I do not have time to coach contributors, and I get
hundreds of PRs a week across my projects, so a rejection may be brief.

Check these before you ask for a review:

1. The direction was agreed in a Discussion or on Discord, unless the change is
   obvious.
2. CI passes, including [Clippy](#clippy), which `mise run lint` does not run.
3. The title follows the [title format](#pull-request-titles-and-descriptions).
4. The description explains the user-visible change. It feeds the release
   notes.
5. Every automated AI review comment is addressed.
6. Generated files and snapshots are committed with the change that produced
   them. See [generated files](/architecture.html#generated-files).
7. User-facing changes include documentation.

If CI is failing, the title is wrong, or an AI review comment is still open,
assume I will wait before I look at the PR.

## Set up a checkout {#set-up-a-checkout}

### Build dependencies {#build-dependencies}

- Rust at or above the `rust-version` in `Cargo.toml`, installed with rustup.
  The project's `mise.toml` does not install Rust.
- mise at or above the `min_version` in `mise.toml`.
- Git and a C toolchain.
- clang and libclang. `mise run build` builds with `--all-features`, which
  generates `aws-lc-sys` bindings with bindgen.
- On Linux and macOS, OpenSSL and its headers, plus `pkg-config` on Linux.
  `--all-features` also turns on the optional `openssl` dependency, which links
  the system OpenSSL.
- For E2E tests: Bash, plus zsh, fish, python3, and jq for the tests
  that use them.

On Debian and Ubuntu:

```sh
sudo apt-get install -y build-essential pkg-config libssl-dev clang libclang-dev
```

On macOS, install the Xcode Command Line Tools with `xcode-select --install`,
and OpenSSL with `brew install openssl@3`. On Windows, `mise run build` builds
with the default features only.

### Clone and build {#clone-and-build}

```sh
git clone https://github.com/jdx/mise.git
cd mise
mise trust
mise install                     # the project's tools: mbx, hk, usage, prettier, shellcheck, ...
mise exec -- hk install --mise   # optional: run the linters as a Git pre-commit hook
mise run build
target/debug/mise --version
```

`mise trust` lets mise load the repository's `mise.toml`, which `mise install`
needs.

### Run your build {#run-your-build}

`mise.toml` puts `target/debug` and `node_modules/.bin` at the front of `PATH`
for everything mise sets up in the checkout: tasks, `mise exec`, and an
activated shell. A `mise` command that a task runs is therefore your
development build once it exists. The `mise` shell function from `mise activate`
still calls the binary that activated the shell. Run `target/debug/mise` when
you need to be sure which binary you are testing. An old build in
`target/debug` can fail the repository's `min_version` check; rebuild it or
delete it.

To run the checkout from any directory, rebuilding when the source changes, I
use this wrapper at `~/.local/bin/@mise`. Change the manifest path to your
clone:

```sh
#!/bin/sh
exec cargo run -q --all-features --manifest-path "$HOME/src/mise/Cargo.toml" -- "$@"
```

Make it executable and put its directory on `PATH`. Test activation changes in
a disposable shell, so a broken hook does not affect the one you work in:

```sh
@mise --help
eval "$(@mise activate zsh)"
@mise activate fish | source
```

Set `MISE_DEBUG=1` or `MISE_TRACE=1` for debug output. `RUST_LOG` has no
effect on mise's logging.

### Cargo build cache {#cargo-build-cache}

`mise.toml` wraps `cargo` with [mbx](https://mr-boxington.jdx.dev), which
shares compiled artifacts across checkouts. `mise run` tasks and
`mise exec -- cargo …` use the wrapper, and plain `cargo` does too once mise is
[activated in your shell](/shell-setup.html). Set `MBX_DISABLE=1` to build
without the cache, as most CI jobs do.

### Project structure {#project-structure}

| Path                       | Contents                                                              |
| -------------------------- | --------------------------------------------------------------------- |
| `src/`                     | The mise library, and the command line under `src/cli/`               |
| `crates/`                  | Workspace crates such as `vfox`, `mise-util`, and `mise-settings`     |
| `registry/`                | One TOML file per tool shorthand                                      |
| `settings.toml`            | Setting definitions, which generate code, schema, and docs            |
| `schema/`                  | JSON schemas for config files                                         |
| `e2e/`, `e2e-win/`         | End-to-end tests, in Bash and in Pester                               |
| `docs/`                    | This site                                                             |
| `tasks.toml`, `xtasks/`    | Project tasks                                                         |
| `hk.pkl`                   | Linter configuration                                                  |
| `packaging/`, `scripts/`   | Release packaging and build scripts                                   |

[Codebase architecture](/architecture.html) maps behaviors to the code under
`src/` and `crates/`.

## Project tasks {#project-tasks}

Run `mise tasks ls` to list tasks and `mise tasks info <name>` to see where one
is defined. Tasks come from `tasks.toml`, file tasks under `xtasks/`, and the
`[tasks]` entries in `mise.toml`, which hold the perf benchmarks.

| Task                          | Alias           | What it does                                                         |
| ----------------------------- | --------------- | -------------------------------------------------------------------- |
| `mise run build`              | `b`             | Debug build, with all features except on Windows                     |
| `mise run lint`               |                 | hk checks; does not run Clippy                                       |
| `mise run lint-fix`           | `fix`, `format` | Applies hk's fixes                                                   |
| `mise run test:unit`          |                 | `cargo test --all-features`                                          |
| `mise run test:e2e <pattern>` | `e`, `e2e`      | Builds, then runs the [E2E tests](#e2e-tests) you select             |
| `mise run snapshots`          |                 | Accepts changed insta snapshots and deletes unreferenced ones        |
| `mise run render`             |                 | Regenerates every [generated file](/architecture.html#generated-files) |
| `mise run docs`               |                 | Starts the docs dev server                                           |
| `mise run docs:build`         |                 | Builds the docs site and checks its links                            |
| `mise run perf`               |                 | Release build plus instruction-count benchmarks                      |
| `mise run clean`              |                 | `cargo clean`                                                        |

`mise run test` (alias `t`) runs the unit tests, then `test:e2e` with no
arguments, which selects no E2E tests; run `mise run test:e2e --all` for the
suite. `mise run ci` runs `lint-fix`, `build`, and `test`, which is not what CI
runs. `mise run release-plz` refuses to run outside GitHub Actions.

## Linting and formatting {#linting-and-formatting}

[`hk.pkl`](https://github.com/jdx/mise/blob/main/hk.pkl) configures Prettier,
markdownlint, taplo, ShellCheck, shfmt, stylua, lua-language-server,
`cargo fmt`, `cargo check`, and JSON schema validation.

```sh
mise run lint               # check every file
mise run lint-fix           # apply the fixes that can be automated
mise exec -- hk check --pr  # check only the files your branch changed
```

`hk install --mise` runs the same fixes as a Git pre-commit hook and stashes
unstaged changes while it runs. On Windows, `lint-fix` runs only Clippy,
Prettier, and `cargo fmt`.

### Clippy {#clippy}

CI runs Clippy with warnings denied, and `mise run lint` does not run it.
Before you push Rust changes, run:

```sh
cargo clippy --workspace --all-features --all-targets -- -D warnings
```

CI also runs Clippy with the default features and on Windows. Fix a warning by
changing the code. Do not add `#[allow(clippy::...)]`, `#[expect(clippy::...)]`,
a Cargo lint level set to `allow`, or `-A clippy::...`. If the fix needs a
preparatory refactor, put the refactor in its own PR first. `clippy.toml`
disallows `std::env::vars`, `std::env::args`, and `std::process::exit`, and
its messages name what to use instead.

## Testing {#testing}

Use unit tests for parsing and resolution, E2E tests for commands and shell
behavior, and snapshots for output that must stay stable. Prefer a local
fixture to a live download.

### Unit tests {#unit-tests}

```sh
mise run test:unit
cargo test --all-features test_name
cargo test --all-features module_name -- --nocapture
mise --cd crates/vfox run test   # the vfox crate
```

Unit tests run one at a time in a shared process: `.cargo/config.toml` sets
`RUST_TEST_THREADS=1`. `mise::testing::init` in `src/testing.rs` sets up the
shared fixture directories under `test/` and the process environment. The
library's tests call it from `src/test.rs`, and the binary's tests from the
`test` module in `src/main.rs`. When a test changes settings or environment
variables, use the existing guards, such as `SettingsGuard` and `EnvVarGuard`,
so the change cannot leak into a later test.

`mise run test:shuffle` runs the unit tests in random order to catch tests
that depend on order. It needs a nightly toolchain (`cargo +nightly`); you do
not have to change your default toolchain.

### E2E tests {#e2e-tests}

`mise run test:e2e` builds mise, then runs the tests you select. Each argument
is a regular expression matched against test file names:

```sh
mise run test:e2e '^test_version$'   # one test
mise run test:e2e '^test_task_'      # every test whose name starts with test_task_
mise run test:e2e --list             # list the test files
mise run test:e2e --all              # every test except *_slow ones
TEST_ALL=1 mise run test:e2e --all   # including *_slow tests
```

The wrapper reduces each argument to its file name and does not anchor the
pattern. `e2e/cli/test_version` therefore also runs `test_version_order`,
`test_version_range`, and `test_version_update_hint`, and `e2e/tasks` runs every
test whose name contains `tasks`, not the tests in that directory. With no
arguments, it runs nothing. Check the `Running test:` lines to see which tests
ran.

`--all` runs up to four tests at a time; set `E2E_JOBS` to change that, or to
run selected tests in parallel. `TEST_TRANCHE` and `TEST_TRANCHE_COUNT` split
the full suite, as CI does.

Each test runs in a fresh working directory with its own `HOME`, mise
directories, and trusted config path, with `MISE_EXPERIMENTAL=1` set and your
`target/debug` first on `PATH`. The host still has to provide the shells,
compilers, and services a test uses. `MISE_E2E_DOCKER=1` runs each test inside
`ghcr.io/jdx/mise:e2e`, the image CI runs the E2E tests in, which has those
packages; it needs a Linux build of mise. Set `MISE_GITHUB_TOKEN` or
`GITHUB_TOKEN` for tests that call the GitHub API; the harness passes them
through. When a test fails, the harness keeps its directory and prints the
path.

Do not run files under `e2e/` directly or make them executable.

### Writing an E2E test {#writing-an-e2e-test}

Create `e2e/<area>/test_<name>`, such as `e2e/env/test_env_greeting`. The
harness sources `e2e/assert.sh` and runs the file with Bash, with
`set -euo pipefail` in effect. A `#!/usr/bin/env zsh` first line runs it with
zsh, still with `assert.sh` and `-euo pipefail`. A `#!/usr/bin/env fish` test
runs with `fish --no-config` and gets neither. Do not clean up after the test:
the harness deletes its directory.

```sh
#!/usr/bin/env bash

cat >mise.toml <<'EOF'
[env]
GREETING = "hello"
EOF
assert "mise exec -- printenv GREETING" "hello"
```

Name a test `test_<name>_slow` if it compiles or downloads a large tool. A test
that takes more than 20 seconds without that suffix gets a warning.

<span id="test-assertions"></span>

Most helpers take the command as a string:

| Helper                                                      | Passes when                                                          |
| ----------------------------------------------------------- | -------------------------------------------------------------------- |
| `assert "cmd" "text"`                                       | `cmd` succeeds and prints exactly `text` (or succeeds, without `text`) |
| `assert_contains`, `assert_not_contains`                    | `cmd` succeeds and its output does or does not contain `text`        |
| `assert_matches "cmd" "regex"`                              | `cmd` succeeds and its output matches the Bash regular expression    |
| `assert_empty`, `assert_not_empty`                          | `cmd` succeeds with empty or non-empty output                        |
| `assert_json "cmd" "json"`                                  | `cmd` prints JSON equal to `json`                                    |
| `assert_fail "cmd" "text"`                                  | `cmd` fails, and its output contains `text` when you pass it         |
| `assert_fail_contains`, `assert_fail_matches`               | `cmd` fails and its output contains `text` or matches `regex`        |
| `assert_directory_exists`, `assert_directory_not_exists`    | The directory does or does not exist                                 |

### Windows E2E tests {#windows-e2e-tests}

The Windows tests in `e2e-win/` use Pester 5. Install it, build mise, and run
the suite. The runner puts `target\debug` first on `PATH`:

```powershell
Install-Module Pester -Force -SkipPublisherCheck
mise run build
pwsh -File e2e-win/run.ps1
pwsh -File e2e-win/run.ps1 -TestName '*task*'
```

`-TestName` filters by the test's full Pester name and accepts wildcards. A
test for activation or `PATH` must run a command through the `PATH` that mise
emits and check that a native Windows child process receives a valid
semicolon-separated `PATH`; comparing output strings is not enough.
`e2e-win/activate_posix_path.Tests.ps1` is an example.

### Registry tool tests {#registry-tool-tests}

`mise test-tool` installs registry tools and runs the test from each entry. See
[tool tests](/contributing/registry.html#tool-testing) for how it works and why
you run it with your build. Plugin authors should also read
[Publishing plugins](/plugin-publishing.html#testing-before-publication).

### Snapshot testing {#snapshot-testing}

`insta` snapshots record output that must stay stable. When output changes on
purpose, run `mise run snapshots`, which accepts every changed snapshot and
deletes unreferenced ones. Review the `.snap` diff, including deletions, before
you commit it.

### Performance testing {#performance-testing}

`mise run perf` builds a release binary and measures instruction counts with
tak. It needs valgrind; without it, tak reports wall-clock time only.
`mise run test:perf` runs wall-clock benchmarks against a generated workspace
and needs Bash 4 or newer, which macOS does not ship. Compare results only
between builds made the same way on the same machine.

## Changing a CLI command {#changing-a-cli-command}

Commands live in `src/cli/` and are declared with `usage_rs` derives:
`#[derive(usage_rs::Args)]` plus `#[usage(...)]` attributes. The same
definition produces `--help`, shell completions, the man page, and the
[CLI reference](/cli/), so edit the source and regenerate. Never edit
`docs/cli/`, `mise.usage.kdl`, or `man/` by hand.

- The doc comment on a command's struct is its help. The first line is the
  short description in `mise --help` and on the CLI index; the rest is the long
  help that `mise <command> --help` and the reference page show. Add
  `verbatim_doc_comment` to `#[usage(...)]` to keep your line breaks.
- Doc comments on fields become the help for arguments and flags.
- Add examples with `example(...)` inside `#[usage(...)]`, as in
  `src/cli/where.rs`. They appear in `--help` and in the reference page's
  examples.
- `hide = true` hides a command or flag. A hidden command gets no reference
  page.
- Classify every new command, hidden or not, in `src/cli/command_effects.rs`:
  add it to `EFFECTS` as `Read`, `Write`, or `Destructive`, or to
  `UNCLASSIFIED` with a reason if it runs user-supplied code. The
  `every_visible_command_is_classified` unit test fails otherwise.
- Completions for argument values, such as tool, task, and setting names, and
  the source link on each reference page come from
  `src/assets/mise-extra.usage.kdl`.
- The "Related documentation" link at the end of a reference page comes from
  the `guides` map in `docs/.vitepress/cli-reference.ts`, and a top-level
  command's group on the [CLI index](/cli/) from `categories` in the same file.
  Add a new command to both. The render fails when a guide points to a page
  that does not exist.

A command definition, trimmed from `src/cli/where.rs`:

```rust
/// Show the install directory of a tool version
///
/// Fails if no matching version is installed.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise where node@20
~/.local/share/mise/installs/node/20.0.0"###,
        help = r###"Show the newest installed node 20.x"###
    )
)]
pub(crate) struct Where {
    /// Tool to look up, such as ruby@3
    #[usage(value_name = "TOOL@VERSION")]
    tool: ToolArg,
}
```

Then regenerate and run the unit tests:

```sh
mise run render:usage        # mise.usage.kdl, docs/cli/, tasks.md
mise run render:completions  # completions/
mise run render:mangen       # man/man1/mise.1
mise run render:help         # docs/.vitepress/cli_commands.ts
mise run test:unit
```

`render:usage` deletes `docs/cli/` before it writes it again. If it fails
partway, run `git restore docs/cli` before you retry.

### Writing CLI help {#writing-cli-help}

- Start the first line with an imperative verb ("Remove stale cache files"),
  keep it under 70 characters, and end it without a period. Write "tools", not
  "tool(s)".
- In the long help, say what the command does, what it changes, and when to
  use something else. Wrap lines at 80 columns, and use full
  `https://mise.jdx.dev/…` URLs, not Markdown links; the reference page adds
  its related documentation link itself.
- Write flag and argument help as a capitalized fragment without a period. The
  web page joins its lines, so make each extra line a full sentence. Name a
  default by its source, such as "Defaults to the `jobs` setting".
- Give each example a different use, with capitalized help text and no
  trailing colon.

## Adding a setting {#adding-a-setting}

Settings are defined in
[`settings.toml`](https://github.com/jdx/mise/blob/main/settings.toml). Each
entry becomes three things: a field of the Rust `Settings` type, which
`crates/mise-settings/build.rs` generates at build time; a property in
`schema/mise.json`; and an entry on the
[settings reference](/configuration/settings.html), which renders
`settings.toml` when the site builds.

```toml
[http_timeout]
default = "30s"
description = "Timeout for connecting or waiting between reads during HTTP requests."
docs = """
This timeout applies to the connection phase and to each individual response
read. The read timer resets whenever data is received, so long-running artifact
downloads are bounded separately by `http_download_timeout`.
"""
env = "MISE_HTTP_TIMEOUT"
type = "Duration"
```

A dotted name, such as `[task.output]`, nests the setting, and the Rust field
follows it: `Settings::get().task.output`. The keys of an entry:

- `type`: `Bool`, `String`, `Integer`, `Duration`, `Path`, `Url`,
  `ListString`, `ListPath`, `SetString`, or `BoolOrString`.
- `description`: one plain sentence, shown by
  `mise settings ls --json-extended`, editor completions, and the schema. For a
  boolean, say what `true` does.
- `docs`: Markdown that replaces `description` on the settings page.
- `env`: the environment variable, usually `MISE_` plus the name in capitals,
  with `_` in place of `.`.
- `default`, or `optional = true` for a setting with no default.
  `default_docs` describes a default that depends on the platform.
- `rust_type`, `parse_env`, and `enum`: a Rust type other than the one `type`
  implies, a parser for the environment variable, and the allowed values.
- `deserialize_with`: a custom deserializer from `crates/mise-settings`, such
  as `bool_string`.
- `merge = "append_unique"`: for a `ListString`, combine the values from every
  config file and drop duplicates, instead of letting the nearest file win.
- `global_only = true`: project config cannot set it. `env_only = true`: only
  the environment variable sets it. `rc = true`: `.miserc.toml` can set it.
- `hide = true`: leaves it off the settings page.
- `deprecated`, `deprecated_warn_at`, and `deprecated_remove_at`: retire it;
  see [deprecations](#deprecations-and-breaking-changes).

Read the value in Rust with `Settings::get()`, for example
`Settings::get().jobs`. Loading, trust, and command-line flags are handled in
`src/config/settings.rs`. Then run `mise run render:schema`, which updates
`schema/mise.json`, `schema/miserc.json`, and `schema/mise-task.json`, and
preview the page with `mise run docs`. Keep an entry's keys in alphabetical
order; `mise run lint-fix` sorts them with taplo.

## Writing documentation {#writing-documentation}

The site is a VitePress project in `docs/`. Read
[Working on the mise docs](https://github.com/jdx/mise/blob/main/docs/README.md)
before you change a page: it covers where content belongs, moving pages,
examples, style, and generated content.

```sh
mise run docs        # dev server that reloads on save
mise run docs:build  # production build that checks every internal link and anchor
```

Add a new page to `docs/.vitepress/sidebar.ts`. Moving a page needs a
`pageRedirects` entry in `docs/.vitepress/redirects.mjs`, and moving a section
to another page needs an `anchorRedirects` entry in
`docs/.vitepress/anchor-redirects.mjs`. The guide's
[Move or rename a page](https://github.com/jdx/mise/blob/main/docs/README.md#move-or-rename-a-page)
section covers headings and labels.

Run `mise run render:llms` after you change a page title, a page's lead, or the
page list, and again after you rebase. It builds mise and renders the CLI pages
first; when only Markdown pages changed,
`mise exec node -- node docs/.vitepress/llms.ts` writes just the index.

## Pull request titles and descriptions {#pull-request-titles-and-descriptions}

PR titles follow [Conventional Commits](https://www.conventionalcommits.org/).
The `conventional-commits` workflow checks the title when a PR is opened,
edited, reopened, or updated. PRs are squash-merged, so CI does not check
intermediate commit messages, though they should use the same format.

```text
<type>[(scope)][!]: <description>
```

| Type       | Use for                                                       |
| ---------- | ------------------------------------------------------------- |
| `feat`     | A new user-facing capability                                  |
| `fix`      | A bug in mise's behavior, not in CI, docs, or infrastructure  |
| `perf`     | A performance improvement                                     |
| `refactor` | A code change with no behavior change                         |
| `docs`     | Documentation only                                            |
| `test`     | Tests only                                                    |
| `style`    | Formatting only                                               |
| `security` | A security fix or hardening                                   |
| `registry` | Changes under `registry/`; never takes a scope                |
| `chore`    | Maintenance, dependency updates, and releases                 |
| `ci`       | CI and automation                                             |
| `revert`   | Reverting an earlier change                                   |

Scope a change by the command it affects (`install`, `use`, `exec`,
`activate`) or by subsystem (`config`, `backend`, `env`, `task`, `vfox`,
`python`, `github`, `schema`, `doctor`, `shim`, `deps`, `ci`). Use `task`, not
`run`, for the task runner.

Start the description with a lowercase letter or an acronym such as `CLI` or
`PGO`, in the imperative mood: "add", not "added". Mark a breaking change with
`!` after the type or scope.

```text
fix(install): resolve version mismatch for previously installed tools
feat(vfox): add semver Lua module for version sorting
docs(contributing): update hk usages
chore(deps): update rust crate demand to v2.4.0
registry: add twg (http:twg)
```

The title becomes the `CHANGELOG.md` entry, which git-cliff groups by type.
Communique writes the GitHub release notes from the titles and descriptions of
merged PRs. Write both for a mise user who has not read the diff: lead with the
problem and what users can do now, include a short config or command example
for a new feature, and describe the before and after for a bug fix. Update them
when review changes what the PR does.

## Deprecations and breaking changes {#deprecations-and-breaking-changes}

Breaking changes are rarely accepted, and only when there is no better
alternative. To remove a feature, backend, or behavior, deprecate it first:

1. In the PR that deprecates it, add a warning banner to its docs and a CLI
   warning with `deprecated_at!` from `src/output.rs`, using the current
   version as `warn_at`:

   ```rust
   deprecated_at!("2026.10.0", "2027.10.0", "old-flag", "Use --new-flag instead.");
   ```

   From `warn_at` on, mise prints the warning once per id and adds the version
   that removes the feature. From `remove_at`, normally 12 months later, a
   `debug_assert!` fires as a reminder to remove the code. Some deprecations
   use a shorter window on purpose; do not lengthen an existing one.

2. Delay the warning by up to 6 months only when the replacement needs a
   setting or syntax that older supported mise versions reject. Removal is
   still counted from when the warning starts.
3. Document the migration path.

Temporary code with no user-facing deprecation, such as a migration or a
fallback that reads what an older mise wrote, uses
`remove_by!("2027.1.0", "id")`. The build fails once mise reaches that version,
until the code is removed.

## Dependencies {#dependencies}

Use the least specific version requirement that works in `Cargo.toml`: `"1"`
rather than `"1.2.3"`, and `"0.12"` for a pre-1.0 crate. A routine update
changes only `Cargo.lock`. Run `cargo update -p <crate> --precise <version>`,
and drop unrelated changes from the lockfile.

CI checks dependencies with these tools, which the project's `mise.toml` does
not install. See
[test-impl.yml](https://github.com/jdx/mise/blob/main/.github/workflows/test-impl.yml)
for the exact flags.

```sh
cargo deny check               # advisories, licenses, and allowed sources
cargo machete --with-metadata  # unused dependencies
cargo msrv verify              # the minimum supported Rust version
```

`deny.toml` allows duplicate versions of a crate.

## Adding tools to the registry {#adding-tools}

New shorthands are for tools that are already widely used, normally with
thousands of GitHub stars, and most proposals are declined. Read
[Adding tools to the registry](/contributing/registry.html) before you open a
PR. It covers the popularity bar, the
[backend acceptance tiers](/contributing/registry.html#backend-acceptance-tiers),
the entry format, and [tool tests](/contributing/registry.html#tool-testing).

## Adding a backend {#adding-backends}

Most contributors want to [add a tool](/contributing/registry.html), not a
backend. New backends are rarely accepted into core, because each one is
permanent maintenance; one might be accepted for a major package manager that
would add a lot to mise. Check first whether an existing backend, such as
`github`, `aqua`, `npm`, or `pypi`, can install what you need. To support a new
source without changing mise, write a
[backend plugin](/backend-plugin-development.html) or a
[tool plugin](/tool-plugin-development.html), starting from
[mise-tool-plugin-template](https://github.com/jdx/mise-tool-plugin-template).
If you think a backend belongs in core, raise it in a
[Discussion](https://github.com/jdx/mise/discussions) or on
[Discord](https://discord.gg/mABnUDvP57) before you write code.

To implement one:

1. Create `src/backend/my_backend.rs` and implement the
   [`Backend` trait](https://github.com/jdx/mise/blob/main/src/backend/mod.rs),
   following a backend with the same installation model. The trait's wrapper
   methods handle caching and policy; implement hooks such as
   `_list_remote_versions`, which returns `VersionInfo` entries, and
   `install_version_` instead of duplicating that logic. Treat versions as
   opaque and leave resolution to the backend.
2. Declare the module in `src/backend/mod.rs` (`pub(crate) mod my_backend;`).
3. Add a variant to `BackendType` in `src/backend/backend_type.rs`, and map its
   prefix in `BackendType::guess`, which falls back to `Unknown` without a
   compiler error. If the backend starts experimental, add it to
   `is_experimental`, which falls back to `false`.
4. Construct the backend in the factory match in `src/backend/mod.rs`. The
   compiler then lists the other exhaustive matches to update, such as
   install-time option keys.
5. Add argument parsing in `src/args/backend_arg.rs` if the identifier needs
   it.

Add E2E tests in `e2e/backend/test_my_backend` that install a tool with the
backend and run it, plus Windows tests if the backend supports Windows. Add a
backend page under `docs/dev-tools/backends/` with installation examples, and
link it from `docs/.vitepress/sidebar.ts` and from the table in
`docs/dev-tools/backends/index.md`. If registry entries will use the backend,
add a URL builder for its prefix in `docs/registry.data.ts` so the registry
page links them.

Backends to read first:

- `src/backend/dotnet.rs`: a small package-manager backend
- `src/backend/packslip.rs`: a release-manifest backend with verification
- `src/plugins/core/node.rs`: a full language runtime

See [Backends](/dev-tools/backends/) for how mise picks a backend.

## CI and releases {#ci-and-releases}

CI builds and tests on Linux, macOS, and Windows, runs the full E2E suite on
Linux in parallel tranches, and checks Clippy, dependencies, the minimum Rust
version, and the PR title. A change under `registry/` also runs
`mise test-tool` for the entries it adds or changes.

mise versions are CalVer (`YYYY.M.PATCH`) and releases are automated. The
release-plz workflow opens a release PR on every push to `main` and once a day,
and publishes the release when that PR merges. Do not run
`mise run release-plz` locally.

## Packaging and self-update instructions {#packaging-and-self-update-instructions}

Distribution packagers can turn off `mise self-update` and point users to their
package manager instead. See [Packaging mise](/packaging.html).
