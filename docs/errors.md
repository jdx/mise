# Errors

This page lists common error messages mise emits, what causes them, and how to fix them.
It complements [Troubleshooting](/troubleshooting.html), which is organized by symptom
(wrong tool version, slow prompts, activation issues) rather than by error message.

Start with the specific failure and any nested cause above the final exit-status or version
line. The footer often just suggests verbose logging; it is not the cause of the error.
For example, to diagnose a Node installation:

```sh
mise --verbose install node@24
MISE_DEBUG=1 mise install node@24
MISE_TRACE=1 mise install node@24
mise doctor
```

Replace `install node@24` with the command that failed. Trace logging is especially detailed;
review logs for credentials, environment values, and private paths before sharing them.

## `Config files in <dir> are not trusted. Trust them with mise trust.`

mise found configuration that needs trust before it can be loaded. Inspect the file, then
run [`mise trust`](/cli/trust.html) in its directory if you accept its contents.
`mise trust --show` displays the current trust status without changing it.

In normal mode, simple tool-version/task configuration can be loaded without trust, and
commands such as `mise run`, `mise install`, and `mise exec` implicitly trust their active
configuration. Environment directives, templates, tool options, ignored configurations, and
[paranoid mode](/paranoid.html) can change what requires explicit approval. Do not assume that
all files are blocked until you have run `mise trust`.

Use [`trusted_config_paths`](/configuration/settings.html#trusted_config_paths) only for
paths whose configuration you intend to trust, including future projects below those paths.

## `<tool> not found in mise tool registry`

The tool name you used has no shorthand in the [registry](/registry.html). If the error
includes a "Did you mean?" list, check for a typo first.

If there is no registry entry, select a backend that supports the tool. These are syntax
examples; replace the repository or package names with real ones:

```sh
mise use aqua:owner/repo     # if it's in the aqua registry
mise use github:owner/repo   # GitHub releases
mise use cargo:some-tool     # crates.io
mise use npm:some-tool       # npm
```

See [backends](/dev-tools/backends/) for all options. The registry only provides short
names for popular tools. Explicit backend syntax avoids needing a registry entry, but the
backend still needs a compatible package or release asset and any required runtime.

## `Failed to install <tool>@<version>: <underlying error>`

A wrapper around whatever actually went wrong during installation — the text after the
colon is the real error, so start there (it's often one of the other errors on this
page, like a 403 or checksum mismatch). If it's unclear, re-run with `--verbose` to see
the full output, or use `mise install <tool>@<version> --raw` to run the install serially
with stdin/stdout connected to your terminal.

## `<tool>@<version> not installed`

The requested version is known to mise but not installed on disk. Run
`mise install` (or `mise install <tool>@<version>`) to install it. `mise ls <tool>`
shows which versions are installed and which are merely requested by config files.

## `[<config file>] <tool>@<version>: <error>` (failed to resolve version)

mise could not resolve the version requested by the named config file — for example
`[~/src/proj/mise.toml] node@99` when no such version exists. Common causes:

- **The version doesn't exist**: check `mise ls-remote <tool>` for available versions.
- **Stale version cache**: a recently released version may not be cached yet. Run
  `mise cache clear node` for Node, or substitute the affected tool, and retry. See
  [new version not available](/troubleshooting.html#new-version-of-a-tool-is-not-available).
- **Network/API errors**: the backend couldn't list versions (rate limits, offline).
  The underlying error after the colon will say so.

## `HTTP status client error (401 Unauthorized)`

For a GitHub URL, this means GitHub rejected the credential mise sent, usually because the token is invalid,
expired, for a different GitHub host, or missing a required scope. The error
includes a `github auth:` line that names the token source when mise resolved it,
for example `GITHUB_TOKEN`, `gh CLI (hosts.yml)`, or `github_tokens.toml`.

Check or replace the token in the named source. If the source is not known, mise
prints `github auth: yes` and refers to a configured GitHub token. If no
Authorization header was sent, it prints `github auth: no`. See
[GitHub Tokens](/dev-tools/github-tokens.html) for supported token sources and
configuration. For a different host, check that backend's authentication settings.

## `HTTP status client error (403 Forbidden)` / `GitHub rate limit exceeded`

A 403 can mean an API rate limit, missing repository access, or an organization policy that
rejects the request. Check the URL and response body. For GitHub, the `github auth:` and
`github rate limit:` diagnostic lines help distinguish these cases.

If the error reports a rate limit, configure authentication or wait for the stated reset.
For public repositories, a token does not need private-repository access. If a token is
already present, verify its source and access to the repository, including any required
organization authorization. See [GitHub Tokens](/dev-tools/github-tokens.html).

For non-GitHub hosts, use the authentication mechanism documented by the relevant backend.
Adding a GitHub token will not fix a 403 from another service.

## `Checksum mismatch for file <file>`

```text
Checksum mismatch for file node-v24.0.0.tar.gz:
Expected: sha256:abc123...
Actual:   sha256:def456...
```

The downloaded file does not match the expected checksum. Identify where that expectation
came from: the lockfile, backend registry, or upstream release checksums. Also check that the
URL and selected asset match the intended version, OS, and architecture.

A truncated download can cause a mismatch; retry the download after checking the network or
proxy error. A release asset replaced upstream can also invalidate a previously recorded
checksum. Compare the release publisher's information before updating a lockfile entry.
Do not delete the expected checksum or disable verification just to make the error disappear.

See [lockfiles](/dev-tools/mise-lock.html) for how artifact URLs and checksums are recorded.
Clearing the version cache alone does not change a checksum pinned in `mise.lock`.

## `mise version <X> is required, but you are using <Y>`

The project's config file declares a [`min_version`](/configuration.html) newer than
your installed mise. Update mise with `mise self-update` (if installed via the
standalone installer) or through the package manager you installed it with.

## `no tasks <name> found`

No [task](/tasks/) with that name is defined in the current config hierarchy. Run
`mise tasks ls` to see available tasks. Check the current directory, selected
environment, and task name, including any monorepo namespace. Use `mise --cd path/to/project tasks ls` to inspect another project. See [task configuration](/tasks/) for file tasks and
configuration discovery.

## `<command> exited with non-zero status: exit code <N>` / `command failed: exit code <N>`

These mean a command mise executed failed — a task, a plugin script, or the program run
via `mise exec`/shims. Start with that child command's output, then check
its working directory, arguments, selected tools, and environment. A task or installation
can fail because those inputs differ from the ones in your interactive shell. Re-run with `--verbose` (or `MISE_DEBUG=1`) to see the
command's full output if it isn't already shown.
