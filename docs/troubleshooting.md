# Troubleshooting

If you're looking for help with a specific error message, see [Errors](/errors.html) — this
page is organized by symptom instead.

## `mise activate` doesn't work in `~/.profile`, `~/.bash_profile`, `~/.zprofile`

Normal `mise activate` installs shell hooks that refresh the environment around prompts and,
for supported shells, directory changes. Put it in your interactive shell's rc file, such as
`~/.bashrc` or `~/.zshrc`. A profile or noninteractive script may never run those hooks.

For scripts, use `mise exec -- command` to compute the project environment for that command.
For editors, [shims](/dev-tools/shims.html) resolve a tool using the process's working
directory. `mise activate --shims` can go in a login profile that your editor reads.

`mise env` also computes tools and variables for the **current project**, not just global
tools. Evaluating its output updates the current shell once; it does not keep updating after
you change directories. See [IDE integration](/ide-integration.html) and [CI setup](/continuous-integration.html).

::: warning
`mise activate --shims` does not support all the features of `mise activate`.<br>
See [shims vs path](/dev-tools/shims.html#shims-vs-path) for more info.
:::

Also see the [shebang](/tips-and-tricks#shebang) example for a way to have scripts call mise to get
the tool they need, another way to use mise without activation.

## Slow shell prompts {#slow-shell-prompts}

`mise activate` runs a hook on every prompt to check if tools or env vars need updating. This typically takes only a few milliseconds, but if your prompts feel sluggish you can profile it with `MISE_TIMINGS`.

In an activated Bash or Zsh session, temporarily deactivate mise, then time `hook-env`
manually. This measures one environment calculation; repeat only when comparing a change:

```sh
mise deactivate

# Show timing per major step (color-coded: red = slow)
MISE_TIMINGS=1 mise hook-env -s bash 2>&1 >/dev/null

# Or use =2 for detailed per-step breakdowns with cumulative time
MISE_TIMINGS=2 mise hook-env -s bash 2>&1 >/dev/null
```

Replace `bash` with your shell. Open a new terminal afterward to restore normal activation.
Common causes of slow environment calculations:

- Expensive `_.source` scripts when the environment needs recomputing
- Large numbers of tools or plugins
- Network-dependent operations in env directives

Use the timing output to identify the slow step before changing settings.
[Environment caching](/cache-behavior.html#environment-caching) and watched files can reduce
repeated work for environment providers.

[`mise activate --shims`](/dev-tools/shims) moves the cost from every prompt to every tool invocation, which may or may not be faster depending on your workflow. See [Shims vs PATH](/dev-tools/shims.html#shims-vs-path) for tradeoffs.

## mise is failing or not working right

Run diagnostics from the directory where the problem occurs:

```sh
mise --version
mise doctor
```

Then rerun the failing command with `--verbose`, `MISE_DEBUG=1`, or `MISE_TRACE=1`.
To keep a debug log, set `MISE_LOG_FILE_LEVEL=debug MISE_LOG_FILE=/path/to/logfile`.
Review diagnostic output before sharing it; environment values and private paths may appear.

For an activation issue, compare `mise exec -- command` with the same command in your shell.
`mise env` shows the computed shell assignments, but computing them can run environment
directives and the output may include secrets. Do not paste it into a public issue unchanged.

For installation failures, `mise install --raw` installs serially and connects the installer's
input/output directly to the terminal. This can reveal an interactive prompt or a nested
build error that was obscured in grouped output.

Update mise through the package manager that installed it, or use `mise self-update` for a
standalone installation. Clear the relevant [cache](/cache-behavior.html) if the symptom is
stale metadata. Reinstalling everything or deleting mise's state should not be the first
step in diagnosing a version-selection or shell problem.

If the problem remains, include the command, relevant configuration, operating system,
shell, mise version, and reviewed `mise doctor` output in a bug report. See [Errors](/errors.html)
for common messages and their underlying causes.

## The wrong version of a tool is being used

Compare the project's selection with the command your shell runs. For Node.js:

```sh
mise ls --current node
mise which node
mise exec -- node --version
node --version
type -a node
```

If `mise ls` shows a missing version, install it with `mise install`. If it shows the wrong
request or configuration source, check the current directory, environment selection, and
[configuration precedence](/configuration.html).

If `mise exec` uses the expected version but `node` does not, inspect `type -a node` for a
shell alias, function, or executable that takes precedence. Remove conflicting activation
from another version manager, or correct the order of `PATH` setup in your shell startup
files. Open a new shell after editing them. For editor-only failures, check the
[editor's process environment](/ide-integration.html).

[`activate_aggressive`](/configuration/settings.html#activate_aggressive) makes activation
prepend tools ahead of other `PATH` entries. It can help with competing PATH updates, but
another hook that runs afterward can still change the order. `mise exec -- command` remains
an explicit way to select the project environment.

## New version of a tool is not available

Versions are cached in two places, so a brand new release might not appear right away.

The first is the mise CLI's own version cache, which can be cleared for Node with `mise cache clear node` (substitute your tool).

The second is the <https://mise-versions.jdx.dev> host, a centralized
place that lists all versions of most tools. It speeds up mise and
avoids GitHub rate limits when querying for new versions. Check that site for your tool to
see if it has the updated version. This service can be disabled by
setting `MISE_USE_VERSIONS_HOST=0`. For a one-command check:

```sh
mise cache clear node
MISE_USE_VERSIONS_HOST=0 mise ls-remote node
```

This queries the backend directly and may require its authentication credentials.

mise also uses the versions host as a shared cache for public GitHub release metadata and
GitHub artifact attestations. This means normal installs of public `github:` and many
`aqua:` tools can avoid unauthenticated GitHub API calls even in Docker builds or CI jobs
that do not have a token configured. If the versions host does not have the requested
metadata yet, mise falls back to GitHub's API.

mise-versions itself also struggles with rate limits, but you can help it fetch more frequently by authenticating
with its [GitHub app](https://github.com/apps/mise-versions). The app requires no permissions since it only
fetches public repository information. The more people do this, the quicker
mise can fetch new versions of tools.

## Windows problems

::: warning
Windows support is available, but asdf plugins can't run on Windows, so tools must use another
backend such as core, vfox, aqua, github, or http—which means some registry tools are not
available on Windows.
:::

### Path limits

If you have many tools defined in your `mise.toml` hierarchy, `mise x` may produce a `Path` environment variable that is too long for certain tools to handle, most notably `cmd.exe`. This affects `mise` tools that invoke `cmd.exe` (like `npm install`).

The limit is **8191 characters**, and `cmd.exe` does not truncate a longer `Path` — it [ignores the variable entirely](https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/command-line-string-limitation). So the symptom is not that one tool goes missing: everything that was found through `Path` stops resolving at once and reports `is not recognized`. Programs in `C:\Windows\System32` keep working, because `cmd.exe` finds those without consulting `Path` — which is what makes the failure look arbitrary, and why the test below matters.

You have a few options:

1. Set the `MISE_INSTALLS_DIR` environment variable to a shorter location, e.g. `C:\.mise-installs`.
1. Use `powershell.exe` or `pwsh.exe` instead of `cmd.exe`, since they can handle a longer `Path`.
1. Re-organise the `mise.toml` files in your monorepo, to specify only the tools they need.
1. Use [shims](/dev-tools/shims.html) to keep your **shell's** `Path` from growing with your toolset — `mise activate --shims` adds one directory rather than one per tool. Be aware of what this does not cover: running a tool through a shim still builds an environment containing every active tool's directory, so a mise-managed tool that itself invokes `cmd.exe` (like `npm`) sees the same long `Path` either way. Shims also [do not support all the features](/dev-tools/shims.html#shims-vs-path) of `mise activate`.

You can run the following command to test whether you have hit the `cmd.exe` `Path` limitation:

```powershell
# Path is within limits
❯ mise x -- cmd.exe /d /s /c "git --version"
git version 2.55.0.windows.3
# Path exceeds cmd.exe limits
❯ mise x -- cmd.exe /d /s /c "git --version"
'git' is not recognized as an internal or external command,
operable program or batch file.
mise ERROR command failed: exit code 1
mise ERROR Run with --verbose or MISE_VERBOSE=1 for more information
```

Two things to get right about that test. First, pick a program that is **not** in `C:\Windows\System32` and not in the directory you run the test from: `cmd.exe` searches the current directory before `Path`, and finds system-directory programs without consulting `Path` at all, so a probe in either place succeeds however long `Path` is. That is exactly why `where.exe` tells you nothing. Second, check that your chosen program runs normally (`git --version` in your shell), since a program you do not have produces the same `is not recognized` that the limit does.

Duplicate `Path` entries are less of a factor than they used to be: on reactivation mise now drops the stale install directories it finds on the inherited `Path` before adding the current toolset's (v2026.5.18), and it collapses exact duplicates in the environments it computes (`mise x`, `mise run`, `mise env`, `mise doctor`) as of v2026.7.18. That lowers what mise contributes, but it does not raise the ceiling — enough distinct tools will still reach 8191.

### Shims leaking into WSL

When `windows_shim_mode` is set to `file`, mise writes an extension-less bash
script next to each `<tool>.cmd` shim (so Git Bash / Cygwin can resolve the
tool). WSL's default Windows-PATH interop exposes the shims directory at
`/mnt/c/...`, where every file is treated as executable, so running a shimmed
tool inside WSL executes that script natively. mise guards the generated script:
when it detects WSL, it drops the shims directory from `PATH` and runs a native
Linux tool if one is installed; otherwise it fails with a plain `<tool>: not
found` rather than recursing endlessly or erroring with `mise: not found`.

The default `exe` mode is not affected: it writes only native `<tool>.exe`
files, which WSL ignores, so nothing leaks into Linux.

Manage Linux tools with a Linux installation of mise inside WSL. To keep Windows PATH entries
out of WSL entirely, disable Windows-PATH interop in `/etc/wsl.conf`:

```ini
[interop]
appendWindowsPath = false
```

### `shell = "bash -c"` task fails with `command not found` from PowerShell

If a task pinned to `shell = "bash -c"` works from Git Bash but fails with
`command not found` from PowerShell, mise is most likely resolving `bash` to
the WSL launcher at `C:\Windows\System32\bash.exe` instead of a real POSIX
bash. The launcher dispatches into the WSL distribution's Linux user-space,
where mise-managed Windows tools aren't visible.

mise prefers a real POSIX bash (Git Bash / MSYS2) automatically when it can
find one in a standard install location. If yours is installed elsewhere, set
`MISE_BASH_PATH` to override:

```powershell
$env:MISE_BASH_PATH = "C:\tools\msys64\usr\bin\bash.exe"
mise run my-bash-task
```

```toml
# Alternatively, scope it to one project from mise.toml
[env]
MISE_BASH_PATH = "C:/tools/msys64/usr/bin/bash.exe"
```

mise honors an **explicit** bash path as-is. If you set `shell` (in a task) or
`windows_default_inline_shell_args` to an absolute path such as
`C:/msys64/usr/bin/bash.exe -c`, mise uses exactly that binary — the
`MISE_BASH_PATH` override and the Git Bash / MSYS2 auto-detection apply only
when the shell is the bare name `bash`.

The same resolution (auto-detection, `MISE_BASH_PATH`, never the WSL launcher)
also applies to the bash mise spawns to source
[`[env] _.source`](/environments/#env-source) scripts.

If your shell path contains spaces (e.g. `C:\Program Files\Git\bin\bash.exe`),
wrap the program in double quotes so the space is not treated as an argument
separator. On Windows, backslashes are treated literally, so they need no
escaping; forward slashes work too:

```toml
[tasks.build]
run = "echo hi"
shell = '"C:\Program Files\Git\bin\bash.exe" -c'
```

(On macOS/Linux, `shell` follows POSIX quoting rules instead.)

#### Cygwin

Native Windows mise can activate Bash, Zsh, and Fish inside Git Bash, MSYS2, or Cygwin.
Run `mise activate` in the shell that will consume its output. mise identifies the
calling shell executable and its runtime DLL, rather than relying on `SHELL` or
`MSYSTEM`, which may be missing or inherited from another shell. Generating an
activation script in PowerShell and sourcing it later in a different runtime is
not supported.

PATH assignments and the executable references in generated hooks use that
runtime's paths. Internally, mise retains native Windows PATH values, including
its saved original PATH. No `cygpath` subprocess is started by activation or hooks.
Mapping supports runtime defaults and persistent `etc/fstab` and `etc/fstab.d`
mounts, including custom drive prefixes. Session-only `mount` changes and arbitrary
filesystem symlinks are not reconstructed; use persistent mounts for custom PATH
locations. Restart the shell after changing its mount configuration.

For tasks:

Point `MISE_BASH_PATH` at your Cygwin bash so the intended one is used:

```powershell
$env:MISE_BASH_PATH = "C:\cygwin64\bin\bash.exe"
```

mise passes PATH through unchanged. Git Bash, MSYS2 and Cygwin all convert it to Unix form on the
way into the shell and back to Windows form on the way out to a native program, so nothing needs
configuring for any of them.

Where they differ is everything _except_ PATH: MSYS2 / Git Bash rewrites POSIX-looking arguments and
other environment variables on the way to a native program — `/c` becomes `C:/` — while Cygwin
leaves both untouched. So a native program launched from a Git Bash task may see an argument you did
not intend; `MSYS_NO_PATHCONV=1` suppresses that for a single command.

## mise isn't working when calling from tmux or another shell initialization script

Shell initialization can run before mise's first environment hook. If you need a tool at that
point, use `mise exec -- python --version`, or
[add the shims to your PATH](/dev-tools/shims.html#how-to-add-mise-shims-to-path), e.g.

```bash
export PATH="$HOME/.local/share/mise/shims:$PATH"
python --version # assumes Python is configured and installed
```

or call `hook-env` manually:

```bash
eval "$(mise activate bash)"
eval "$(mise hook-env)"
python --version # assumes Python is configured and installed
```

For more information, see [What does `mise activate` do?](/faq#what-does-mise-activate-do)

## Is mise secure?

mise can verify downloads and restrict untrusted configuration, but the guarantees depend
on the backend and settings in use. Read [Security](/security.html) for verification methods,
safe mode, and configuration trust, and [Paranoid mode](/paranoid.html) for stricter checks.
Report vulnerabilities through [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md).

## 403 Forbidden when installing a tool

You may get an error like one of the following:

```text
HTTP status client error (403 Forbidden) for url
403 API rate limit exceeded for
```

This can happen if the tool is hosted on GitHub and you've hit the API rate limit, which is especially
common when running mise in a CI environment like GitHub Actions.

By default, mise uses <https://mise-versions.jdx.dev> to avoid most public GitHub API calls
for release metadata and artifact attestation checks. If you still see this error, it usually
means the metadata was not available from the versions host yet, `MISE_USE_VERSIONS_HOST=0`
is set, the tool uses a private repository, or the tool uses GitHub Enterprise/custom API
settings.

A 403 can also indicate missing repository access or an organization policy. Check the
response and authentication diagnostics before treating it as a rate limit. See
[GitHub Tokens](/dev-tools/github-tokens.html) and [403 errors](/errors.html).

## Tool not found after `mise install` or `mise use` in a script

Installing a tool changes files on disk; it cannot change the parent script's environment.
Use `mise exec` for the next command. For a project that declares Node.js:

```sh
mise install
mise exec -- node --version
```

If many later commands need the same environment, evaluate `mise env` for your shell after
installation, or put [shims](/dev-tools/shims.html) on `PATH`. Keep the script in the intended
project directory so mise finds its configuration.

## Creating `~/.bash_profile` breaks existing `~/.profile` on Ubuntu/Debian

On many Linux distributions, `~/.profile` sources `~/.bashrc` and sets up your environment.
However, if `~/.bash_profile` exists, bash reads that **instead of** `~/.profile`.

If you followed setup instructions that created `~/.bash_profile` for mise, your existing
`~/.profile` configuration (including PATH, environment variables, etc.) may stop loading.

**Fix:** Add mise activation to `~/.bashrc` instead, or source `~/.profile` from your
`~/.bash_profile`:

```bash
# ~/.bash_profile
[[ -f ~/.profile ]] && source ~/.profile
```

## Tasks with `redact` env vars and `raw` output {#tasks-with-redact-env-vars-break-raw-output}

Raw and interactive tasks inherit the terminal's input/output. mise cannot redact output
that bypasses its output processing, and it emits a hint when redactions are configured.
Use normal task output when redaction is required; removing `redact` is not a fix for secret
handling. See [task output](/tasks/task-configuration.html#raw).

If an older mise release produces no output for a raw task with redactions, update mise and
retry with a harmless test value. Current raw mode passes output through directly.

## `mise activate` in CI / non-interactive shells

Use `mise exec -- command` or `mise run task` in CI. They select the environment without
requiring a shell prompt. Shims are another option when commands need to resolve tools
through `PATH`. See [Continuous integration](/continuous-integration.html) for complete
provider examples and [script installation](#tool-not-found-after-mise-install-or-mise-use-in-a-script)
for the install-then-execute pattern.

## Auto-install on command not found does not trigger

When you run a command that is not found, mise can install the tool that provides it (the [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) feature). It maps the command back to a tool using the `bins` metadata in mise's registry, which means a tool that is configured but has never been installed is handled too — not only a missing version of a tool you already have.

If nothing happens, the cause is usually one of these:

- **The tool is configured by a raw backend spec.** `"cargo:some-crate" = "1.0.0"` or `"github:owner/repo" = "1.0.0"` is not a registry entry, so it carries no bin metadata and nothing connects the command you typed to it.
- **The tool is not configured at all.** The handler only installs tools your config already asks for in the current directory; it will not pick a tool for a command you have never declared.
- **The feature is off for that tool** — either [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) is `false`, or the tool is listed in [`auto_install_disable_tools`](/configuration/settings.html#auto_install_disable_tools).

**Workarounds:**

- Where a registry entry exists, refer to the tool by its registry name (`ripgrep`) rather than by a raw backend spec (`github:BurntSushi/ripgrep`), so the handler can map the command to it.
- Otherwise, install it explicitly instead of on demand: `mise install`, or [`mise x|exec`](/cli/exec) to install and then run something in one step. Both materialise the whole configured toolset, so the backend does not matter. [`mise r|run`](/cli/run) does the same, but only as part of running a task.
- Installing once by hand is enough to make the handler work from then on: with a version present, mise can also discover the mapping from the installed executables.
