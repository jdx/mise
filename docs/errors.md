---
description: "Look up a mise error or warning message to find what caused it and how to fix it."
outline: [2, 3]
---

# Error messages

Look up a message that mise printed to see what causes it and how to fix it.
Problems without a clear message, such as the wrong tool version running, are
in [Troubleshooting](/troubleshooting.html).

## How mise reports an error {#reading-errors}

mise prints the error on a `mise ERROR` line, then each underlying cause on its
own line, so the most specific cause is usually the last one before `Version:`:

```text
mise ERROR error parsing config file: ~/src/app/mise.toml
mise ERROR Config files in ~/src/app/mise.toml are not trusted.
Trust them with `mise trust`. See https://mise.jdx.dev/cli/trust.html for more information.
mise ERROR Version: 2026.10.4 linux-x64 (2026-10-07)
mise ERROR Run with --verbose or MISE_VERBOSE=1 for more information
```

The `Version:` line and the hint to rerun with `--verbose` are not part of the
cause. With `--verbose` or `MISE_DEBUG=1`, mise prints a longer report that
numbers the causes from `0` and adds the source location. Before you share that
output, see [Collect diagnostics](/troubleshooting.html#mise-is-failing-or-not-working-right).

## Loading config {#loading-config}

### `Invalid TOML in config file` {#invalid-toml}

A config file has a syntax error. mise shows the file, line, and column with a
marker under the problem:

```text
  × Invalid TOML in config file
   ╭─[~/src/app/mise.toml:1:7]
 1 │ [tools
   ·       ▲
   ·       ╰── unclosed table, expected `]`
```

Fix the syntax at that position. Until a project file parses, mise cannot tell
whether it needs trust, so an untrusted file with a syntax error fails with
[`Config files in <path> are not trusted`](#untrusted-config) instead. The
`TOML parse error at line <N>, column <M>` warning above that error shows the
real problem.

### `Config files in <path> are not trusted` {#untrusted-config}

mise found a project config file that needs trust before it can load: one with
`[env]`, hooks, templates, tool options, or settings. Usually `<path>` is the
config file, and the message follows an `error parsing config file: <path>`
line. Read the file, then trust it:

```sh
mise trust ~/src/app/mise.toml
```

mise asks before it fails when it can. This error appears where it cannot ask,
such as in a script, an editor extension, or a command without a terminal, and
after you answer No. Once you answer No, or run `mise trust --ignore`, later
commands skip that config without an error until you run `mise trust` on it.

When `mise run` finds no tasks because a config in the current directory is
untrusted or was declined, it prints
`Config file(s) in <dir> are not trusted: <files>` instead. `mise doctor` reports an untrusted file only as
`failed to load config: error parsing config file: <path>`; run
`mise trust --show` to see the trust status of each config directory from the
current one up.

Paths under
[`ignored_config_paths`](/configuration/settings.html#ignored_config_paths)
never load, and `mise trust` does not override that setting. To trust every
config under a directory you control, including projects you create there
later, set
[`trusted_config_paths`](/configuration/settings.html#trusted_config_paths) in
your global config. In [paranoid mode](/paranoid.html), every project config
needs trust, again each time it changes. See
[Configuration trust](/security.html#configuration-trust) for which files need
trust and which commands trust config for you.

### `mise version <X> is required, but you are using <Y>` {#min-version}

The project sets a
[`min_version`](/configuration.html#minimum-mise-version) newer than your mise.
Update with the package manager that installed mise, or run `mise self-update`.
`mise self-update` skips releases younger than 24 hours, so if the required
version is newer than that, name it:

```sh
mise self-update 2026.10.4
```

`mise version <X> is recommended, but you are using <Y>` is the same check as a
warning, from a `soft` minimum. mise keeps working.

### `<feature> is experimental` {#experimental}

The command or config uses an experimental feature, and the full message ends
`Enable it with mise settings experimental=true`. That command turns on
[`experimental`](/configuration/settings.html#experimental) in your global
config. To enable it for one project instead, add it to the project's
`mise.toml`:

```toml [mise.toml]
[settings]
experimental = true
```

Experimental features can change in any release.

### `<operation> is disabled in safe mode (MISE_SAFE=1)` {#safe-mode}

[Safe mode](/security.html#safe-mode) is on, through `MISE_SAFE=1` or the
`safe` setting, and the command reached something it refuses, such as running
a task, a `postinstall` option, or `exec()` in a template. Run the command
without safe mode if you trust the config, or use a command that only resolves
versions, such as `mise ls` or `mise lock`.

## Finding and installing tools {#installing-tools}

### `<tool> not found in mise tool registry` {#not-in-registry}

There is no [registry](/registry.html) entry with that name. `mise install` and
`mise use` report it as
`Failed to install <tool>@<version>: <tool> not found in mise tool registry`,
followed by a `Did you mean?` list when similar names exist. Check that list,
or search the registry:

```sh
mise search ripgrep
```

For a tool that is not in the registry, name its backend directly. See
[Which backend to use](/dev-tools/backends/#which-backend-to-use):

```sh
mise use github:owner/repo   # GitHub releases
mise use aqua:owner/repo     # a tool in the aqua registry
mise use npm:package-name    # an npm package
```

### `Failed to install <tool>@<version>: <cause>` {#failed-to-install}

The text after the colon is the actual error, often another entry on this page,
such as a 404, a 403, or a checksum mismatch. When several tools fail, mise
prints `Failed to install tools: <list>` and then each tool's cause. A line
starting `note: <tool>@<version> was not checked against its version list`
means mise could not fetch the version list, so the version may not exist.

If the cause is unclear, rerun with `--verbose`, or install the tool alone with
the installer connected to your terminal:

```sh
mise install node@24 --raw
```

### `HTTP status client error (404 Not Found)` {#http-404}

The URL in the message does not exist. Common causes:

- The version does not exist. When a request matches no listed version, mise
  tries to download it as written, so `node = "99"` fails with a 404 for
  `node-v99.tar.gz`. Check the available versions with `mise ls-remote <tool>`.
- Every release that matches a prefix is newer than the
  [minimum release age](/security.html#minimum-release-age). For a day after
  Node.js 26.11.0 comes out, `node = "26.11"` fails with a 404 for
  `node-v26.11.tar.gz`, and `mise ls-remote node` warns that a newer release
  is hidden. Name the exact version, such as `node@26.11.0`, to install it now,
  or wait until the release is old enough.
- The repository or package name in a backend identifier is wrong, such as
  `github:owner/repo`.
- The repository is private. GitHub answers 404, not 403, when the request has
  no token with access. See
  [GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html).

### `[<config file>] <tool>@<version>: <cause>` {#failed-to-resolve-version}

mise could not resolve the version that the named config file requests, for
example `[~/src/app/mise.toml] node@24: <cause>`. The text after
`<tool>@<version>:` says why, usually that mise could not fetch the version
list because of a network error or one of the HTTP errors on this page. It
also appears inside a `Failed to resolve tool version list for <tool>`
warning.

### `no versions found for <tool>` {#no-versions-found}

The backend listed no version that matches a `latest` or `prefix:` request.
A plain prefix such as `node = "26"` that matches nothing is tried as written
instead, which usually fails with a [404](#http-404). When the message goes on
`matching minimum_release_age`, every matching release is newer than the
[minimum release age](/security.html#minimum-release-age), 24 hours by
default. The message names the newest hidden release; install it explicitly as
the message suggests, such as `mise use node@24.11.1`, or lower
`minimum_release_age`.

`unable to fetch versions for <tool>: <cause>` means mise could not list
versions at all; the cause says why.

### `HTTP status client error (401 Unauthorized)` {#http-401}

The server rejected the credential that mise sent. For GitHub, the token is
invalid, expired, meant for another host, or missing a scope. The message
includes a `github auth:` line:

- `github auth: yes (token from <source>)` names where the token came from, such
  as `GITHUB_TOKEN`, `gh CLI (hosts.yml)`, or `github_tokens.toml`.
- `github auth: yes` means a configured token was sent, with no known source.
- `github auth: no` means mise sent no token.

Fix or replace the token in that source. `mise token github` shows which token
mise uses and where it came from. See
[GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html). For other
hosts, check that backend's authentication settings.

### `HTTP status client error (403 Forbidden)` / `GitHub rate limit exceeded` {#http-403}

A 403 (or 429) from GitHub usually means you hit the API rate limit, which is
common in CI. It can also mean that the token has no access to the repository,
or that an organization policy rejects it. The message has three lines that
tell these apart:

- `github auth:` says whether mise sent a token.
- `github rate limit:` shows the remaining quota and when it resets.
- `github response:` shows GitHub's own explanation.

For a rate limit, set a GitHub token or wait for the reset that mise prints in
`GitHub rate limit exceeded. Resets at <time>`. A token for public repositories
needs no scopes. If a token is already set, check its access to the repository
and any organization authorization. See
[GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html), and
[GitHub Actions and other CI](/dev-tools/github-tokens.html#ci-github-actions)
for CI.

mise gets public release metadata and artifact attestations from the
[mise-versions](https://mise-versions.jdx.dev) host to avoid most GitHub API
calls. A 403 from GitHub therefore usually means that the host did not have the
metadata yet, that `MISE_USE_VERSIONS_HOST=0` is set, or that the tool comes
from a private repository or GitHub Enterprise. A
[lockfile](/dev-tools/mise-lock.html) that records download URLs avoids most
API calls during installs.

A 403 from another host needs that backend's credentials; a GitHub token does
not help there.

### `Checksum mismatch for file <file>` {#checksum-mismatch}

```text
Checksum mismatch for file node-v24.0.0.tar.gz:
Expected: sha256:abc123...
Actual:   sha256:def456...
```

The downloaded file does not match the checksum mise expected, which comes from
`mise.lock`, the backend's metadata, or the release's checksum file. Usually the
download was cut short or a proxy changed it, so retry. If it fails again, the
publisher may have replaced the release file. Check the release page before you
refresh the lockfile entry; see
[Checksum mismatch](/dev-tools/mise-lock.html#regenerating-checksums). Never
delete the checksum to get past the error. Clearing the cache does not change a
checksum recorded in `mise.lock`.

### `<tool>@<version> is not in the lockfile` / `No lockfile URL found` {#not-in-lockfile}

[Strict lockfile mode](/dev-tools/mise-lock.html#strict-lockfile-mode) is on,
through `--locked`, `MISE_LOCKED=1`, or the `locked` setting, and `mise.lock`
does not cover this install:

- `is not in the lockfile` means the lockfile records no version for the tool.
- `No lockfile URL found for <tool> on platform <platform>` means the tool's
  entry has no download URL for this platform.

Run `mise lock`, as the `hint:` line under the message says, and commit the
updated `mise.lock`. To add entries for other platforms, such as your CI
runners, see
[Preparing platform entries](/dev-tools/mise-lock.html#preparing-platform-entries).

## Running tools {#running-tools}

### `missing: <tool>@<version>` {#missing}

This is a warning. The project asks for a version that is not installed, and
another version of the tool is. Run `mise install`. To change when the warning
appears, set
[`status.missing_tools`](/configuration/settings.html#status.missing_tools).

### `No version is set for shim: <command>` / `Tool not installed for shim: <command>` {#shim-errors}

You ran a [shim](/dev-tools/shims.html) in a directory where no config selects
a version of its tool. Add one with `mise use <tool>@<version>`, or set a
default for every directory with `mise use -g <tool>@<version>`; the message
lists the installed versions to choose from.

`Tool not installed for shim` means a config selects a version that is not
installed, and automatic installation did not run. Run `mise install`.

### `<tool>@<version> not installed` / `<command> is not a mise bin` {#not-installed}

`mise where` and `mise which` report these when the version you name, or the
version the project selects, is not installed. Run `mise install`, or
`mise install <tool>@<version>`. `mise ls <tool>` shows which versions are
installed and which ones config files ask for.

### `PATH is <N> characters, longer than the 8191 cmd.exe accepts` {#path-too-long}

On Windows, the `PATH` mise built is too long for `cmd.exe`, which then ignores
it. See [PATH too long](/troubleshooting.html#path-limits).

## Tasks and commands {#tasks}

### `no task <name> found` / `no tasks defined in <dir>` {#task-not-found}

`no task <name> found` lists similar task names and the tasks that are
available. Check the spelling, the selected
[config environment](/configuration/environments.html), and any monorepo
prefix. `mise tasks ls` lists the tasks for the current directory, and
`mise --cd <dir> tasks ls` lists them for another project.

`no tasks defined in <dir>` means mise found no tasks at all. Check that you
are in the project directory. If the message names non-executable files in a
task directory, follow its hint to make them executable. If a config in the
current directory is untrusted or was declined, mise reports
[`Config file(s) in <dir> are not trusted`](#untrusted-config) instead.

A dependency that names a missing task fails with `task not found: <name>`
instead; see [Dependencies and execution order](/tasks/architecture.html).

### `[<task>] ERROR task failed` / `<command> exited with non-zero status` {#command-failed}

A task, plugin script, hook, or installer that mise ran exited with an error.
For a task, mise prints `[<task>] ERROR task failed` and exits with the task's
status. With `--continue-on-error`, it ends with an `ERROR <N> task(s) failed:`
summary. For plugin scripts and installers, it reports
`<command> exited with non-zero status: exit code <N>` (or `killed by <signal>`).
`mise exec` and shims pass the command's exit code through without a message.

The command's own output above the message is the cause. If that output is
hidden, rerun with `--verbose`, or use `mise install --raw` for installs. If the
command behaves differently than in your shell, compare its directory, tools and
environment with `mise exec -- <command>`.
