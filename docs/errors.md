# Errors

This page lists common error messages mise emits, what causes them, and how to fix them.
It complements [Troubleshooting](/troubleshooting.html), which is organized by symptom
(wrong tool version, slow prompts, activation issues) rather than by error message.

Every error is followed by a generic footer like this:

```text
mise ERROR Version: 2026.7.0
mise ERROR Run with --verbose or MISE_VERBOSE=1 for more information
```

The actual error is the line(s) above the footer. To get more detail on any error:

```sh
mise --verbose <command>    # or MISE_VERBOSE=1 — show stacktraces and command output
MISE_DEBUG=1 mise <command> # debug logging
MISE_TRACE=1 mise <command> # trace logging (very verbose)
mise doctor                 # diagnostics and warnings about your setup
```

## `Config files in <dir> are not trusted. Trust them with mise trust.`

mise found a config file (`mise.toml`, `.tool-versions`, etc.) in a directory you
haven't marked as trusted. Config files can define environment variables, templates, and
tasks, so mise won't load them from unfamiliar directories until you approve them.

Run [`mise trust`](/cli/trust.html) in the directory to trust it. To trust a whole tree
of projects (e.g. everything under `~/src`), use the
[`trusted_config_paths`](/configuration/settings.html#trusted_config_paths) setting.
See also [paranoid mode](/paranoid.html) for stricter behavior.

## `<tool> not found in mise tool registry`

The tool name you used has no shorthand in the [registry](/registry.html). If the error
includes a "Did you mean?" list, check for a typo first.

If the tool genuinely isn't in the registry, you can still install it by specifying the
backend explicitly:

```sh
mise use aqua:owner/repo     # if it's in the aqua registry
mise use github:owner/repo   # GitHub releases
mise use cargo:some-tool     # crates.io
mise use npm:some-tool       # npm
```

See [backends](/dev-tools/backends/) for all options. The registry only provides short
names for popular tools — any tool can be installed with explicit backend syntax.

## `Failed to install <tool>@<version>: <underlying error>`

A wrapper around whatever actually went wrong during installation — the text after the
colon is the real error, so start there (it's often one of the other errors on this
page, like a 403 or checksum mismatch). If it's unclear, re-run with `--verbose` to see
the full output, or use `mise install <tool>@<version> --raw` to run the install serially
with stdin/stdout connected to your terminal.

## `<tool>@<version> not installed`

The requested version is known to mise but not installed on disk. Run
`mise install` (or `mise install <tool>@<version>`) to install it. `mise ls <tool>`
shows which versions are installed vs. merely requested by config files.

## `[<config file>] <tool>@<version>: <error>` (failed to resolve version)

mise could not resolve the version requested by the named config file — for example
`[~/src/proj/mise.toml] node@99` when no such version exists. Common causes:

- **The version doesn't exist**: check `mise ls-remote <tool>` for available versions.
- **Stale version cache**: a recently released version may not be cached yet. Run
  `mise cache clear` and retry. See
  [new version not available](/troubleshooting.html#new-version-of-a-tool-is-not-available).
- **Network/API errors**: the backend couldn't list versions (rate limits, offline).
  The underlying error after the colon will say so.

## `HTTP status client error (403 Forbidden)` / `GitHub rate limit exceeded`

You've hit GitHub's API rate limit, which is very low for unauthenticated requests.
This is especially common in CI. If no token is configured, mise prints a warning
telling you so.

Set a GitHub token (no scopes required) as `GITHUB_TOKEN` or `MISE_GITHUB_TOKEN` in
your environment — see [GitHub Tokens](/dev-tools/github-tokens.html) for all supported
token sources. If a token _is_ set, verify it's valid and has access to the repository
(private repos need appropriate scopes).

The error output includes `github auth:` and `github rate limit:` lines to help
diagnose which case you're in.

## `Checksum mismatch for file <file>`

```text
Checksum mismatch for file node-v24.0.0.tar.gz:
Expected: sha256:abc123...
Actual:   sha256:def456...
```

The downloaded file doesn't match the expected checksum from the lockfile, the aqua
registry, or the tool's published checksums. Causes, in rough order of likelihood:

- **Corrupted or truncated download**: run `mise cache clear` and retry.
- **Stale lockfile**: the checksum in [`mise.lock`](/dev-tools/mise-lock.html) was
  recorded for a different artifact (e.g. the upstream release asset was re-uploaded).
  Remove the affected entry from `mise.lock` and reinstall to re-lock it.
- **Tampering**: if the mismatch persists and you can't explain it, don't override
  it — verify the upstream release before installing.

## `mise version <X> is required, but you are using <Y>`

The project's config file declares a [`min_version`](/configuration.html) newer than
your installed mise. Update mise with `mise self-update` (if installed via the
standalone installer) or through the package manager you installed it with.

## `no tasks <name> found`

No [task](/tasks/) with that name is defined in the current config hierarchy. Run
`mise tasks ls` to see available tasks — note that tasks are loaded from config files in
the current directory and its parents, so a task defined in another project directory
won't be visible.

## `<command> exited with non-zero status: exit code <N>` / `command failed: exit code <N>`

These mean a command mise executed failed — a task, a plugin script, or the program run
via `mise exec`/shims. The problem is in the command, not mise itself; mise propagates
the command's exit code. Re-run with `--verbose` (or `MISE_DEBUG=1`) to see the
command's full output if it isn't already shown.
