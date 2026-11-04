---
description: "Diagnose problems with shell activation, tool versions, downloads, tasks, and Windows setups."
outline: [2, 3]
---

# Troubleshooting

Fix common problems with shell activation, tool versions, downloads, tasks,
and Windows setups, grouped by symptom. To look up a message that mise printed,
see [Error messages](/errors.html).

## Collect diagnostics {#mise-is-failing-or-not-working-right}

Run these from the directory where the problem happens:

```sh
mise --version
mise doctor        # setup problems, config files, tools, and activation status
mise doctor path   # the PATH entries mise adds
```

Rerun the failing command with `--verbose`, or with `MISE_DEBUG=1`
(`MISE_TRACE=1` for more detail). To keep a log, add
`MISE_LOG_FILE=mise.log MISE_LOG_FILE_LEVEL=debug`. If an install fails
without a clear cause, `mise install --raw` installs one tool at a time and
connects the installer to your terminal, which shows prompts and build errors
that the progress display hides.

For a shell problem, compare the command in your shell with
`mise exec -- <command>`, which computes the project environment for that one
command.

Logs, trace output and `mise env` output can contain secrets and private paths,
so review them before you share them.

Update mise with the package manager that installed it, or with
`mise self-update`; see [Updating mise](/installing-mise.html#updating). Clear
the [cache](/cache-behavior.html) when the symptom is stale metadata. Deleting
mise's data or reinstalling every tool rarely fixes a version-selection or
shell problem.

If the problem remains, open a [GitHub Issue](https://github.com/jdx/mise/issues)
with the command, your config, OS, shell, mise version, and `mise doctor`
output. For questions, start a [Discussion](/contact.html).

## Shell activation and PATH {#shell-activation-and-path}

### Tools are missing in a login shell, editor, script, or CI {#mise-activate-doesn-t-work-in-profile-bash-profile-zprofile}

<a id="tool-not-found-after-mise-install-or-mise-use-in-a-script"></a>
<a id="mise-activate-in-ci-non-interactive-shells"></a>
<a id="mise-isn-t-working-when-calling-from-tmux-or-another-shell-initialization-script"></a>

`mise activate` applies the environment when it runs, then updates it from a
hook that runs before each prompt and, in most shells, after each `cd`. A
process that never runs that hook keeps the environment it started with:

- Login profiles (`~/.profile`, `~/.bash_profile`, `~/.zprofile`) are read
  only by login shells. Shells and programs started without reading them, such
  as a subshell or a terminal that opens a non-login shell, inherit the `PATH`
  but not the hook, so tools stop following you between projects. Put
  `mise activate` in your interactive startup file (`~/.bashrc`, `~/.zshrc`,
  `~/.config/fish/config.fish`) and `mise activate --shims` in the profile; see
  [Add shims to PATH](/dev-tools/shims.html#how-to-add-mise-shims-to-path).
- Editors and other GUI programs read a login profile at most once, when they
  start. Put the shims on their `PATH` as above, then restart the editor. See
  [Editors and IDEs](/ide-integration.html). Shims
  [do not cover everything activation does](/dev-tools/shims.html#shims-vs-path).
- Scripts and CI jobs never show a prompt. Run commands with
  `mise exec -- <command>` or `mise run <task>`, which compute the environment
  for that command. Keep the script in the project directory so mise finds its
  config. See [Continuous integration](/continuous-integration.html).
- Installing a tool in a script does not change that script's `PATH`. Run the
  next command through `mise exec`:

  ```sh
  mise install
  mise exec -- node --version
  ```

- In Bash, Zsh, Fish and PowerShell, and in Elvish after `mise:activate`,
  lines after the activation line in the same startup file already see mise
  tools. Lines before it, and shells that never read that file, such as the
  `sh -c` commands that tmux runs from `tmux.conf`, do not. Use
  `mise exec -- <command>` there, or put the shims on `PATH` first. In Nushell
  and Xonsh, activation applies the environment at the first prompt. See
  [Scripts and shell startup files](/dev-tools/shims.html#using-mise-in-rc-files).

`eval "$(mise env -s bash)"` loads the current project's tools and variables
into a shell once; it does not update when you change directories. A
standalone script can also name its tool in a [shebang](/tips-and-tricks.html#shebang).

### Creating `~/.bash_profile` stopped `~/.profile` from loading {#creating-bash-profile-breaks-existing-profile-on-ubuntu-debian}

A login Bash reads only the first of `~/.bash_profile`, `~/.bash_login` and
`~/.profile` that exists. Ubuntu and Debian keep `PATH` setup in `~/.profile`,
so a new `~/.bash_profile` hides it, whether you created the file by hand, for
[shims](/dev-tools/shims.html#how-to-add-mise-shims-to-path), or with
`bash = true` in [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html).
Load `~/.profile` from the new file:

```sh [~/.bash_profile]
[[ -f ~/.profile ]] && source ~/.profile
```

### The wrong version of a tool runs {#the-wrong-version-of-a-tool-is-being-used}

Compare what the project selects with what your shell runs. For Node.js:

```sh
mise ls --current node       # the version and the config file that selects it
mise which node              # the executable mise would run
mise exec -- node --version  # the version mise runs
node --version               # the version your shell runs
type -a node                 # every node your shell can find, in order
mise doctor path             # the PATH entries mise adds
```

If `mise ls` shows the version as missing, run `mise install`. If it shows the
wrong request or config file, check the current directory, the selected
[config environment](/configuration/environments.html), and
[how config files combine](/configuration.html#configuration-hierarchy). See
[How a request resolves](/dev-tools/versions.html#how-requests-resolve) for
how mise picks among installed versions.

If `mise exec` runs the expected version but `node` does not, `type -a node`
shows the alias, function, or executable that comes first. Remove activation
for other version managers, and put `mise activate` after anything that edits
`PATH` in your startup file, such as a Zinit or Oh My Zsh plugin. Open a new
shell after editing it.
[`activate_aggressive`](/configuration/settings.html#activate_aggressive) keeps
tool directories ahead of `PATH` entries added after activation, such as a later
line in your startup file. A prompt hook that runs after mise's can still
change the order at each prompt; move that hook before `mise activate`, or use
`mise exec -- <command>`. For a problem that happens only in an editor, see
[Editors and IDEs](/ide-integration.html).

### Shell prompts are slow {#slow-shell-prompts}

`mise activate` runs a hook at every prompt. It returns early when nothing
relevant changed, so the cost is usually a few milliseconds. To time one full
environment calculation:

```sh
MISE_TIMINGS=1 mise hook-env --force -s bash >/dev/null  # time per step
MISE_TIMINGS=2 mise hook-env --force -s bash >/dev/null  # with sub-steps
```

Replace `bash` with your shell. `--force` skips the early-exit check, so the
command measures the whole calculation without changing your session. The
usual causes of a slow calculation are:

- `_.source` scripts that run when the environment needs recomputing
- many tools or environment plugins
- env directives or templates that make network requests

[Environment caching](/cache-behavior.html#environment-caching) and
`watch_files` reduce repeated work for environment plugins. On slow
filesystems such as NFS,
[`hook_env.chpwd_only`](/configuration/settings.html#hook_env.chpwd_only)
checks config only when you change directories, and
[`hook_env.cache_ttl`](/configuration/settings.html#hook_env.cache_ttl) caches
those checks.

[`mise activate --shims`](/dev-tools/shims.html) moves the cost from every
prompt to every tool call, which is faster or slower depending on how often you
run tools. See [Performance](/dev-tools/shims.html#performance).

## Versions and downloads {#versions-and-downloads}

### A new release is not listed {#new-version-of-a-tool-is-not-available}

Three things can hide a release from the last day:

- The [minimum release age](/security.html#minimum-release-age). For most
  backends, mise does not list a release, or pick it for a prefix or `latest`,
  until it is 24 hours old. `mise ls-remote` then warns
  `1 newer node release hidden by minimum_release_age`. To install the release
  now, name the exact version, such as `mise use node@24.11.1`. A prefix whose
  matching releases are all hidden, such as `node@24.12` in the day after
  24.12.0 comes out, fails with a [404](/errors.html#http-404).
- mise's cached copy of the version list. See
  [When mise refreshes version lists](/cache-behavior.html#version-list-refresh)
  for which commands use the cache.
- The [mise-versions](https://mise-versions.jdx.dev) host, which serves most
  tools' version lists and checks upstream on a schedule.

To see every release the backend has, clear the cache for the tool, then skip
the versions host and the age filter:

```sh
mise cache clear node
mise ls-remote --no-versions-host --minimum-release-age 0s node
```

Without the versions host, mise may need the backend's credentials, such as a
[GitHub token](/dev-tools/github-tokens.html). The
[`use_versions_host`](/configuration/settings.html#use_versions_host) setting
turns the host off for every command.

The versions host is rate-limited by GitHub too. Authorizing the
[mise-versions GitHub app](https://github.com/apps/mise-versions), which
requests no permissions, gives it more API quota, so new releases appear
sooner.

### Downloads fail with 403 Forbidden {#_403-forbidden-when-installing-a-tool}

A 403 from GitHub is usually a rate limit, especially in CI. See
[`HTTP status client error (403 Forbidden)`](/errors.html#http-403).

### Typing a missing command does not install it {#auto-install-on-command-not-found-does-not-trigger}

When you type a command that is not found, mise can install the configured
tool that provides it
([`not_found_auto_install`](/configuration/settings.html#not_found_auto_install)).
It finds the tool from the registry's list of each tool's commands, so it also
covers a configured tool that has never been installed. Nothing happens when:

- mise is not activated with `mise activate` in Bash, Zsh, Fish, or PowerShell.
  Other shells have no command-not-found hook, and shims exist only for the
  commands of tools that are already installed.
- The tool is configured with a backend identifier such as `"cargo:some-crate"`
  or `"github:owner/repo"` rather than a registry name such as `ripgrep`.
  Backend identifiers carry no command list, so use the registry name where one
  exists.
- The tool is not in your config. Set
  [`not_found_auto_install_registry`](/configuration/settings.html#not_found_auto_install_registry)
  to install a tool that exactly one registry entry provides; mise adds it to
  your global config.
- [`auto_install`](/configuration/settings.html#auto_install) or
  `not_found_auto_install` is `false`, or the tool is listed in
  [`auto_install_disable_tools`](/configuration/settings.html#auto_install_disable_tools).

For a tool with a backend identifier, mark it `lazy = true` and list its
commands in `lazy_bins`. mise then creates shims that install it the first time
one of its commands runs; see [Lazy tools](/dev-tools/shims.html#lazy-tools):

```toml [mise.toml]
[tools]
"github:owner/repo" = { version = "1.2.3", lazy = true, lazy_bins = ["tool"] }
```

Otherwise, run `mise install`, or [`mise exec`](/cli/exec.html), which installs
missing tools before it runs the command. Both skip tools marked `lazy = true`
until their commands run; `mise install --include-lazy` installs those too.
Once any version of a tool is installed, mise also finds its commands in that
installation, so the handler can install other versions the project asks for
later.

## Tasks {#tasks}

### Secrets are not redacted in raw task output {#tasks-with-redact-env-vars-break-raw-output}

Tasks with [`raw = true`](/tasks/task-configuration.html#raw) or
[`interactive = true`](/tasks/task-configuration.html#interactive), and runs
with `--raw`, write straight to your terminal, so mise cannot mask
[redacted](/environments/secrets/#redaction) values in their output. It prints a
hint when this applies. Leave `raw` off for tasks whose output may contain
secrets. See [What redaction covers](/environments/secrets/#what-redaction-covers).

For tasks that run in the wrong order or that mise does not find, see
[Dependencies and execution order](/tasks/architecture.html) and
[`no task <name> found`](/errors.html#task-not-found).

## Windows {#windows-problems}

For which tools and backends work on Windows, see
[Does mise work on Windows?](/faq.html#windows-support)

### `cmd.exe` stops finding programs: PATH too long {#path-limits}

mise warns when a `PATH` it builds is longer than `cmd.exe` accepts:

```text
mise WARN  PATH is 9120 characters, longer than the 8191 cmd.exe accepts. ...
```

`cmd.exe` does not truncate a longer `PATH`; it
[ignores the variable entirely](https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/command-line-string-limitation).
Everything it would find through `PATH` then fails with `is not recognized`,
while programs in `C:\Windows\System32` keep working. Tools that run through
`cmd.exe`, such as `npm`, `npx` and batch scripts, are affected.

To shorten `PATH`:

1. Set `MISE_INSTALLS_DIR` to a short directory, such as `C:\.mise-installs`.
2. Declare tools only in the `mise.toml` files that need them. In a monorepo,
   move tools out of the root config.
3. Run commands from PowerShell, which accepts a longer `PATH`.

mise already drops exact duplicates from the `PATH` it gives `mise exec`,
`mise run`, and `mise env`, so the length comes from distinct directories.
[Shims](/dev-tools/shims.html) keep your own shell's `PATH` short, but a tool
started through a shim still gets every active tool's directory, so a tool that
calls `cmd.exe` sees the same long `PATH`.

To confirm, run a program that is not in `C:\Windows\System32` through
`cmd.exe`, from a directory that does not contain it. `cmd.exe` finds programs
in those two places without reading `PATH`:

```powershell
mise exec -- cmd.exe /d /s /c "git --version"
```

If `git --version` works in your shell but this prints
`'git' is not recognized as an internal or external command`, `PATH` is over
the limit.

### An editor reports `spawn EINVAL` {#vscode-for-windows-extension-with-error-spawn-einval}

An editor extension that starts a `.cmd` shim directly fails with
`spawn EINVAL` since a
[Node.js security fix](https://nodejs.org/en/blog/vulnerability/april-2024-security-releases-2#command-injection-via-args-parameter-of-child_processspawn-without-shell-option-enabled-on-windows-cve-2024-27980---high).
Use the default
[`windows_shim_mode = "exe"`](/configuration/settings.html#windows_shim_mode),
run `mise reshim`, and restart the extension or language server. If the
extension still starts a `.cmd` path, change its tool-path setting to the
`.exe` shim in `%LOCALAPPDATA%\mise\shims`, or to the path that
`mise which <tool>` prints.

### A `bash -c` task fails with `command not found` from PowerShell {#shell-bash-c-task-fails-with-command-not-found-from-powershell}

A task with `shell = "bash -c"` needs a POSIX bash such as Git Bash or MSYS2.
From PowerShell, the first `bash` on `PATH` is often the WSL launcher,
`C:\Windows\System32\bash.exe`, which runs the command inside Linux, where
mise's Windows tools are not visible. mise skips the WSL launcher and uses Git
Bash or MSYS2 from their standard install locations or from `PATH`. When it
finds neither, it warns
`no real POSIX bash found on PATH (only the WSL launcher)`. Install Git for
Windows or MSYS2, or point `MISE_BASH_PATH` at a bash:

```powershell
$env:MISE_BASH_PATH = "C:\tools\msys64\usr\bin\bash.exe"
mise run my-bash-task
```

To set it for one project, put it in `mise.toml`:

```toml [mise.toml]
[env]
MISE_BASH_PATH = "C:/tools/msys64/usr/bin/bash.exe"
```

`MISE_BASH_PATH` and the detection apply only when the shell is the bare name
`bash`. If a task's `shell` or
[`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
names a path, such as `C:/msys64/usr/bin/bash.exe -c`, mise runs that binary.
The same rules choose the bash that runs
[`_.source`](/environments/#env-source) scripts.

Quote a path that contains spaces. On Windows, backslashes in `shell` are
literal and forward slashes also work; on macOS and Linux, `shell` follows
POSIX quoting rules:

```toml [mise.toml]
[tasks.build]
run = "echo hi"
shell = '"C:\Program Files\Git\bin\bash.exe" -c'
```

### Git Bash, MSYS2, and Cygwin {#cygwin}

Native Windows mise can activate Bash, Zsh and Fish running under Git Bash,
MSYS2, or Cygwin. Run `mise activate` from that shell's own startup file, as
shown in [Shell setup](/shell-setup.html#windows): mise detects the runtime when
the line runs and writes `PATH` in the form that runtime expects. A script
generated in PowerShell and sourced later in another shell does not work.

If directories on `PATH` use custom mount points, declare them in the runtime's
`/etc/fstab` or `/etc/fstab.d`. mise converts paths with the runtime's default
mounts and those files only, so mounts made with `mount` in the current session
and symlinked directories are not converted. Restart the shell after changing
mounts.

For tasks with `shell = "bash -c"`, set `MISE_BASH_PATH` to choose Cygwin's
bash:

```powershell
$env:MISE_BASH_PATH = "C:\cygwin64\bin\bash.exe"
```

mise passes `PATH` to these shells unchanged, and they convert it in both
directions, so `PATH` needs no setup. Other arguments differ: when Git Bash or
MSYS2 starts a native program, it rewrites arguments and environment variables
that look like POSIX paths (`/c` becomes `C:/`), and Cygwin does not. Prefix a
command with `MSYS_NO_PATHCONV=1` to turn that off for one command.

### Windows tools run inside WSL {#shims-leaking-into-wsl}

WSL adds the Windows `PATH` to Linux by default. With
[`windows_shim_mode = "file"`](/configuration/settings.html#windows_shim_mode),
mise writes an extensionless bash script beside each `.cmd` shim, and a command
typed in WSL can find it under `/mnt/c`. The script detects WSL, removes its
own directory from `PATH`, and runs the Linux tool if one is installed;
otherwise it fails with `<tool>: not found`. The default `exe` mode writes only
`<tool>.exe` files, which a command named `<tool>` in WSL does not match.

Install the Linux build of mise inside WSL to manage Linux tools there. To keep
Windows paths out of WSL entirely, add this to `/etc/wsl.conf`, run
`wsl --shutdown` from PowerShell, and reopen WSL:

```ini [/etc/wsl.conf]
[interop]
appendWindowsPath = false
```

`wsl --shutdown` stops every running WSL distribution, so save your work first.
