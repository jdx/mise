---
outline: [2, 3]
---

# FAQs

Quick answers about daily commands, shell integration, and configuration. For a failed
command, start with [Troubleshooting](/troubleshooting.html) or [Errors](/errors.html).

## Daily commands

### What is the difference between `mise install` and `mise use`?

`mise install` installs requested tools without adding tool declarations to configuration.
`mise use` installs a tool and writes its version request to a config file.

```sh
mise use node@24       # select Node 24 for the project and install it
mise install          # install tools already declared by the project
mise install node@24  # install Node 24 without adding a declaration
```

Installation alone does not change your parent shell's environment. Use activation, shims,
`mise exec`, or `mise run` to run the selected tools. To record a concrete version, add
`--pin` to `mise use`; to reproduce a project's lockfile, use `mise install --locked`.

`mise install node` uses the configured request when present, otherwise `latest`.
`mise install` without tool arguments installs the configured toolset.

### Where does `mise use` write to?

By default, mise chooses the lowest-precedence config file in the nearest applicable config
directory. This can be a parent directory or a `.tool-versions` file, not necessarily a
`mise.toml` in the current directory. With both `mise.toml` and `mise.local.toml` in that
directory, shared configuration is preferred. See [write target selection](/configuration.html#target-file-for-write-operations).

Use an explicit target when location matters:

```sh
mise use --path mise.toml node@24  # this file
mise use --global node@24          # global configuration
mise use --env local node@24       # personal local environment
```

`mise use --dry-run node@24` previews the operation. `mise config` shows the files loaded
for the current directory.

### Does a partial version select the newest release? {#does-node-20-mean-the-newest-available-version-of-node}

A partial request such as `node@20` restricts the matching versions. For normal execution,
mise can reuse an installed version that satisfies the request. Installation and upgrade
commands can resolve a newer available match. A lockfile can pin the resolved result.

```sh
mise latest --installed node@20  # inspect an installed match
mise latest node@20              # inspect an available match
mise install node@20             # install a matching release
```

Use `mise ls --current node` and `mise which node` to inspect what this project actually uses.
Do not infer the selected version from a directory name or assume that every tool follows
Node's version scheme. See [version selection](/dev-tools/) and [lockfiles](/dev-tools/mise-lock.html).

### Does `latest` mean the newest remote version?

`latest` is resolved by the tool's backend. For normal execution it can reuse an installed
version, so a newly published release does not automatically change your environment.
A lockfile can constrain the selection further.

To query an available release, run `mise latest node`. An explicit request such as
`mise install node@latest` or `mise exec node@latest -- node --version` asks for an available
release rather than only reusing the current installed match, subject to lock policy.

To upgrade within your configured requests, use `mise upgrade node`. To also change the
request in configuration, use `mise upgrade --bump node`. See [upgrading tools](/dev-tools/)
and [lockfiles](/dev-tools/mise-lock.html). Different backends define “latest” differently;
it does not universally mean the largest semantic version or include prereleases.

### How does `mise exec` work?

`mise exec` reads configuration, resolves and installs missing requested tools as needed,
computes the environment, and runs the command after `--`:

```sh
mise exec -- node --version
mise exec node@24 -- node --version
```

The first command uses the project's configured Node version. The second supplies an
override for that invocation. You do not need to repeat `node@24` when that is already the
request in `mise.toml`. The child receives mise's environment; the parent shell is unchanged.

## Shells and editors

### What does `mise activate` do?

`mise activate` prints a shell script. Evaluating it in your shell installs the mise function
and hooks that refresh tool paths and environment variables. Prompt hooks notice changes to
configuration; supported shells also have directory-change hooks.

`mise hook-env` computes the assignments needed for the current directory, including removal
of values from the previous project. It can exit early when nothing relevant changed. It
prints shell code; running the executable alone does not modify its parent shell.

The shell function lets commands such as `mise shell` and `mise deactivate` update the current
session. Put activation in your [shell's startup file](/getting-started.html#activate-mise).
For a script or CI job, use `mise exec -- command` or `mise run task` so execution does not
depend on a prompt hook. See [shell integration choices](#how-do-mise-activate-shims-mise-exec-and-mise-env-relate).

### How do `mise activate`, shims, `mise exec`, and `mise env` relate?

Each method makes mise tools available at a different boundary:

| Method                  | Environment applies to                            | Typical use                    |
| ----------------------- | ------------------------------------------------- | ------------------------------ |
| `mise activate`         | Current shell, refreshed by hooks                 | Interactive terminals          |
| `mise activate --shims` | Adds a shim directory to the current shell's PATH | Editors and simple shell setup |
| A tool shim             | The launched tool and its children                | Commands found through PATH    |
| `mise exec` / `mise x`  | One command and its children                      | Scripts and CI                 |
| `mise env`              | Prints assignments for another program to consume | Environment integrations       |
| `mise run`              | A task and its dependencies                       | Named project commands         |

Shims load `[env]` for the tool they launch, but do not export it into the parent shell or
install prompt hooks there. Use normal activation for shell hooks and automatic shell
variable updates. See [Shims vs PATH](/dev-tools/shims.html#shims-vs-path).

### Windows support?

mise supports native Windows, including PowerShell activation. Follow the
[Windows installation instructions](/installing-mise.html#windows-winget) and the
[shell compatibility table](/getting-started.html#shell-feature-compatibility).

Shims, `mise exec`, and `mise run` are also available. A shim loads the mise environment for
the tool it launches; it does not update the parent PowerShell session.

Backend and tool support varies by platform. asdf shell plugins require Unix; use a compatible
core, binary-download, or vfox implementation on Windows. WSL uses Linux tools and should have
its own Linux mise installation. See [Windows troubleshooting](/troubleshooting.html#windows-problems).

### Why does a Windows editor report `spawn EINVAL`? {#vscode-for-windows-extension-with-error-spawn-einval}

An extension that tries to spawn a `.cmd` shim directly can fail with `spawn EINVAL` after a [Node.js security fix](https://nodejs.org/en/blog/vulnerability/april-2024-security-releases-2#command-injection-via-args-parameter-of-child_processspawn-without-shell-option-enabled-on-windows-cve-2024-27980---high).

Use the default [`windows_shim_mode = "exe"`](/configuration/settings.html#windows_shim_mode),
run `mise reshim`, and restart the affected extension or language server. See
[IDE integration](/ide-integration.html) if it still resolves the old shim path.

### How do I disable/force CLI color output?

Use `NO_COLOR=1` or `MISE_COLOR=0` to disable ANSI color, and `CLICOLOR_FORCE=1` to force it,
including when piping output. `CLICOLOR_FORCE=1` takes precedence over `NO_COLOR`,
so remove conflicting overrides when diagnosing unexpected color.

```sh
NO_COLOR=1 mise ls
CLICOLOR_FORCE=1 mise ls
```

These settings control mise's output. Child tools may have their own color options.

## Configuration and networking

### How do I keep personal configuration out of Git? {#i-don-t-want-to-put-a-mise-toml-tool-versions-file-into-my-project-since-git-shows-it-as-an-untracked-file}

Use `mise.local.toml` for personal project settings. Add it to `.git/info/exclude` to ignore it
in just your checkout, or to your global Git ignore file to ignore it across projects.
Keep shared tool versions and tasks in a committed `mise.toml`.

If you need to keep `mise.toml` itself private to a checkout, the same ignore mechanisms
work. A project's `.gitignore` is another option when the team agrees to the policy.
Ignore rules only affect untracked files; they do not hide changes to a file already in Git.

### What is the difference between "nodejs" and "node" (or "golang" and "go")?

These are aliased. For example, `mise install nodejs@24` is the same as `mise install node@24`. This
means they cannot be different plugins.

This is for convenience, so you don't need to remember which one is the "official" name. If
the aliasing misbehaves, use the canonical names `node` and `go`, and [report the mismatch](/contact.html).
Under the hood, when mise reads a config file or CLI input, it swaps out "nodejs" and
"golang".

When mise _writes_ to a `mise.toml` (`mise use`, `mise unuse`), it writes the canonical name — a
`nodejs` entry becomes `node`, keeping its comments. `.tool-versions` files are unaffected and still
use the asdf spellings.

### My config file is being ignored / `mise trust` issues

Trust depends on the configuration contents and the command, not file authorship. Safe config files —
those that only contain `min_version`, `[tools]` entries whose values are plain version
strings or arrays of strings, and `[tasks]` without templates — load without trust. Tool-option
tables and other top-level settings require trust. In normal mode, `mise run`, naked task
invocations such as `mise <TASK>`, `mise install`, `mise exec`, and `mise watch` automatically
trust the active config because they explicitly execute project-defined behavior. Other unsafe
config requires trust. Common issues:

- **Accidentally denied trust**: If mise prompted you to trust a file and you said no, it is
  added to the ignore list. Inspect `mise trust --show`, then run
  `mise trust path/to/mise.toml` to trust the reviewed file again.
- **Symlinked configs**: If your config is symlinked (e.g., via GNU Stow), mise may track the
  symlink target path. Try `mise trust` pointing to the actual file path.
- **CI**: In detected CI, mise assumes configs are trusted unless paranoid mode is enabled.
- **Non-interactive mode**: In a non-interactive shell, such as an IDE extension or script without
  a TTY, mise cannot prompt you to trust a config. Outside normal-mode `mise run`, `mise <TASK>`,
  `mise install`, `mise exec`, and `mise watch`, commands that directly load an untrusted
  `mise.toml` can fail with an untrusted-config error. Commands that discover previously tracked
  configs may skip untrusted entries instead. Either run `mise trust` beforehand or set
  [`trusted_config_paths`](/configuration/settings.html#trusted_config_paths) in your global settings
  for configs you trust.
- **Global config** is operator-owned and does not need project trust. Check `mise config`
  if a file you thought was global is being discovered as project configuration.

Run `mise doctor` (`mise dr`) to see if any config files are untrusted — it will
list them under "problems".

Also check your current directory and selected [configuration environment](/configuration/environments.html).
A profile file that is not selected is not a trust failure.

### How do idiomatic version files (`.python-version`, `.node-version`, etc.) work?

Idiomatic version files (`.python-version`, `.node-version`, `.ruby-version`, etc.) are
**disabled by default** in mise. They are only read if you explicitly opt in per tool using
[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools):

```sh
# Enable reading .node-version files
mise settings add idiomatic_version_file_enable_tools node
```

If you previously enabled idiomatic files and now want to stop mise from reading them
(e.g., because `uv` manages `.python-version`), remove that tool from the configured list. Unset the setting entirely to restore its empty default.

See [Idiomatic Version Files](/configuration.html#idiomatic-version-files) for more information.

### How do the shorthand plugin names map to repositories?

The bundled [registry](/registry.html) maps short names to backend specifications, such as
`aqua:owner/repo` or `vfox:owner/plugin`. It is maintained in the repository's
[`registry/`](https://github.com/jdx/mise/tree/main/registry) directory and ships with mise.

Most tools do not need an external plugin. Inspect a tool's selected backend with
`mise tool ripgrep`, or see its available choices with `mise registry ripgrep`.
For plugin-backed tools, the backend specification identifies the plugin repository.
See [backend selection](/dev-tools/backend_architecture.html#how-backend-selection-works).

### How do I use mise with HTTP proxies?

Set `http_proxy` and `https_proxy` in the environment that starts mise. For example, replace
the proxy host and port below with your organization's proxy:

```sh
https_proxy=http://proxy.example.com:8080 mise install
```

Plugin scripts and package managers may have separate proxy or certificate settings. If a
single backend fails, identify the subprocess or URL in verbose output and check that tool's
proxy configuration. See [Errors](/errors.html) for network and authentication failures.

## Migration

### How do I migrate from asdf?

1. Install mise and [configure shell activation](/getting-started.html#activate-mise).
2. Remove asdf activation from the shell's startup files, then open a new shell.
3. Run `mise install` in a project with `.tool-versions`, then verify a configured tool with
   `mise exec -- node --version` (substitute your project's tool).

mise reads `.tool-versions`, but its global configuration normally lives at
`~/.config/mise/config.toml`. Review your `~/.tool-versions` and add the defaults you want with
`mise use --global`. For example, after choosing the versions you need:

```sh
mise use --global node@24 python@3.14
```

This example chooses new defaults; it is not a lossless conversion of an arbitrary asdf file.
Keep the old file until you have checked every tool, including entries with multiple versions
or aliases. Do not share the installation directories between asdf and mise. Once verified,
you can [uninstall asdf](https://asdf-vm.com/manage/core.html#uninstall).

### How compatible is mise with asdf?

mise supports `.tool-versions` and the asdf shell-plugin interface on Unix. Compatibility
with every asdf command or plugin is not guaranteed. Prefer the mise syntax shown in each
command's help, such as `mise install node@24`.

The asdf Go rewrite introduced commands such as `asdf set`. `mise set` has a different
purpose: it sets environment variables. Use `mise use` to select tool versions.

If a team shares `.tool-versions` between both tools, use concrete versions that asdf accepts.
`mise use --pin` writes a resolved version instead of a fuzzy request. You can also keep
`mise.toml` alongside `.tool-versions`; the mise file takes precedence for tools declared in
both at the same directory level. See [asdf compatibility](/asdf-legacy-plugins.html) and
[plugin usage](/plugin-usage.html).

## Scope and related tools

### Can mise manage system packages and desktop applications? {#mise-is-for-dev-tools-not-applications-or-system-packages}

`[tools]` manages versioned development tools and runtimes. Host packages, desktop
applications, and system libraries belong in [`[bootstrap.packages]`](/bootstrap/packages/).

For example, a compiler may need an OS development-library package before a tool can build.
Declare that package with the appropriate manager and apply it using `mise bootstrap`.
Most managers delegate to the OS package manager; mise's built-in Homebrew installers can
handle `brew:` and `brew-cask:` entries without requiring Homebrew itself.

Host packages share the machine's package database or prefix. They do not gain per-project
version switching simply because they are declared in `mise.toml`.

### How do I install tools other users can run without mise?

Two features install binaries that work on `PATH` with no mise involved at runtime.

Use [`[bootstrap.packages]`](/bootstrap/packages/) with `brew:` entries for tools that have
a Homebrew formula:

```toml
[bootstrap.packages]
"brew:ffmpeg" = "latest"
"brew:jq" = "latest"
```

mise pours bottles into the canonical prefix (`/home/linuxbrew/.linuxbrew` on Linux,
`/opt/homebrew` on arm64 macOS) with the usual `<prefix>/bin` links, and does not require
Homebrew itself to be installed. Once `<prefix>/bin` is on `PATH`, the binaries behave like
any other Homebrew install.
[Keg-only](https://docs.brew.sh/FAQ#what-does-keg-only-mean) formulae are the exception:
like brew, mise leaves them out of the prefix, so their binaries stay at
`<prefix>/opt/<name>/bin`.

On arm64 macOS and x86_64/arm64 Linux, where mise's brew manager runs,
`mise bootstrap packages import --manager brew` snapshots an existing Homebrew or Linuxbrew
setup into your config — the formulae you installed on request, or every linked formula
with `--all`.

Use [`mise install-into`](/cli/install-into.html) for any backend mise supports. It installs
one tool version into a directory you pick, for use outside of mise:

```sh
mise install-into node@24 "$HOME/standalone-node"
"$HOME/standalone-node/bin/node" --version
```

Point it at a new or empty directory: `install-into` deletes whatever is already at the
destination, after a confirmation prompt that defaults to no, or without asking under
`--yes`. The tool installation goes to that directory; backends may also use mise caches and
install dependencies. Add its `bin` to `PATH` yourself the way you
would the brew prefix above. Tools that expect environment variables such as `JAVA_HOME`,
or other configuration mise normally applies at runtime, still need those set up by hand.

Both approaches make the same trade Homebrew makes: one version on `PATH` for everyone,
with no per-project version selection. When you want that selection, keep the tools in
`[tools]` and let [`mise bootstrap`](/bootstrap.html) converge each user's activation,
config, and tools in one command — or across many hosts with
[`mise bootstrap remote`](/bootstrap/remote.html).

### Is mise secure?

mise supports download verification, configuration trust, and optional restrictions such
as safe and paranoid modes. Their guarantees depend on the backend and operation. Start with
[Security](/security.html) for the threat model and available controls. Report vulnerabilities
through [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md).

### What is usage?

[usage](https://usage.jdx.dev/) is a spec and CLI for defining CLI tools.

Arguments, flags, environment variables, and config files can all be defined in a usage spec. A single definition can drive help text, argument parsing, and completions.

mise embeds usage for task argument parsing, help, and autocompletion, so the separate `usage` CLI is not required. See [autocompletion](/installing-mise.html#autocompletion).

You can use usage in file tasks to get autocompletion working; see [file task arguments](/tasks/file-tasks.html#arguments).

### What is pitchfork?

[pitchfork](https://pitchfork.jdx.dev/) is a process manager for developers.

It handles daemon management with features like automatic restarts on failure, smart readiness checks, shell-based auto-start/stop when entering project directories, and cron-style scheduling for periodic tasks.

Use [mise tasks](/tasks/) for commands and dependency ordering; use a process supervisor when
a service needs to keep running independently of a task invocation.

### How does mise versioning work?

mise uses calendar versions in the form `YYYY.MONTH.RELEASE`, such as `2026.9.1`.
The final number is a release counter within the month, not a day of the month or a
compatibility indicator.

New features can be introduced behind settings such as `experimental = true`. Deprecation
warnings and [release notes](https://github.com/jdx/mise/releases) describe behavior changes;
do not infer compatibility from a SemVer-style major version. For a team's required mise
version, use [`min_version`](/configuration.html).
