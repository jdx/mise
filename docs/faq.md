---
description: "Get short answers about mise commands, version requests, shell activation, config trust, and related tools."
socialDescription: "Short answers about mise commands, version requests, activation, and config trust."
outline: [2, 3]
---

# FAQ

If a command fails, see [Troubleshooting](/troubleshooting.html), or look up
the message in [Error messages](/errors.html).

## Daily commands

### What is the difference between `mise install` and `mise use`?

`mise install` installs tools. `mise use` installs a tool and also writes its
version request to a config file:

```sh
mise use node@24      # add Node 24 to mise.toml and install it
mise install          # install every tool the project declares
mise install node@24  # install Node 24 without changing config
```

Neither command changes the environment of the shell you run it from. With
shell activation, the next prompt picks up the change; otherwise, run the tools
with `mise exec` or `mise run`. Given no version, `mise install node`
installs the version the project asks for, or `latest` if it asks for none.
Without arguments, `mise install` skips tools marked `lazy = true`; add
`--include-lazy` to install those too.

### Where does `mise use` write to?

`mise use` writes to the nearest config file that applies to the current
directory. That can be a `mise.toml` in a parent directory, or a
`.tool-versions` file. When a directory has both `mise.toml` and
`mise.local.toml`, mise writes to `mise.toml`. See
[Which file mise writes to](/configuration.html#target-file-for-write-operations).

To choose the file yourself:

```sh
mise use --path mise.toml node@24  # this file
mise use --global node@24          # your global config
mise use --env local node@24       # mise.local.toml, for personal versions
```

`mise use --dry-run node@24` shows what would change, and `mise config ls`
lists the config files mise loads in the current directory.

### Does `node@20` use the newest Node 20 release? {#does-node20-mean-the-newest-available-version-of-node}

Not automatically. When a project asks for `node = "20"`, mise uses the newest
Node 20 release you already have installed. It picks from the available
releases only when no 20.x is installed, or when you run
`mise install node@20` or `mise upgrade node`:

```sh
mise latest --installed node@20  # the installed 20.x that mise uses
mise latest node@20              # the newest 20.x available
mise upgrade node                # install that release and use it
```

With a [lockfile](/dev-tools/mise-lock.html), mise uses the version recorded
there. See [How a request resolves](/dev-tools/versions.html#how-requests-resolve).

### Does `latest` mean the newest release? {#does-latest-mean-the-newest-remote-version}

Not in a config file. `node = "latest"` uses the newest version you have
installed, so a new upstream release does not change your environment until you
run `mise upgrade node`. On the command line, `mise install node@latest`,
`mise exec node@latest -- node -v`, and `mise latest node` ask the backend for
its newest release.

Each backend decides what `latest` means. It is not always the highest version
number, and most backends leave out prereleases. See
[Version requests and version files](/dev-tools/versions.html) and
[Upgrade tools](/dev-tools/#upgrade-tools).

### How does `mise exec` work?

[`mise exec`](/cli/exec.html) reads the config for the current directory,
installs any missing tools, and runs the command after `--` with the project's
tools and environment variables:

```sh
mise exec -- node --version          # the Node version the project selects
mise exec node@24 -- node --version  # Node 24 for this one command
```

The command and its children get the environment; your shell does not change.

## Shells and editors

### What does `mise activate` do?

`mise activate` prints a script for your shell. When your startup file runs it,
mise sets `PATH` and your `[env]` variables for the current directory, then
adds a hook that runs before each prompt and, in most shells, after each `cd`.
The hook updates the environment for the directory you are in and removes the
previous project's values. Activation also defines a `mise` shell function so
that `mise shell` and `mise deactivate` can change the current session.

Set it up from [Shell setup](/shell-setup.html). Scripts and CI jobs never show
a prompt, so use `mise exec` or `mise run` there.

### How do `mise activate`, shims, `mise exec`, and `mise env` relate?

They make mise tools available in different places. `mise activate` updates
your interactive shell, shims serve editors and other programs that find tools
through `PATH`, `mise exec` and `mise run` set up one command or task, and
`mise env` prints the variables for another program to load. See
[Choose how to use mise tools](/dev-tools/shims.html#overview) for a comparison.

### Does mise work on Windows? {#windows-support}

Yes. mise runs natively on Windows, with PowerShell activation, shims,
`mise exec`, and `mise run`. Install it with `winget install jdx.mise`, or see
[Windows](/installing-mise.html#windows) for Scoop and Chocolatey. To set up
PowerShell, Git Bash, or `cmd.exe`, see [Shell setup](/shell-setup.html#windows).

Tools from asdf plugins do not run on native Windows, because the plugins are
shell scripts. Core tools and backends such as aqua, github, http, npm, and
vfox work when the tool itself supports Windows. `mise registry <tool>` lists the
backends a tool can use. In WSL, install the Linux build of mise and manage
Linux tools there. For Windows problems, see
[Troubleshooting](/troubleshooting.html#windows-problems).

### How do I turn color output off or force it on? {#how-do-i-disable-force-cli-color-output}

Set `NO_COLOR=1` or `MISE_COLOR=0` to turn color off, and `CLICOLOR_FORCE=1` to
keep it on when output goes to a pipe:

```sh
NO_COLOR=1 mise ls
CLICOLOR_FORCE=1 mise ls | less -R
```

`NO_COLOR=1` and `MISE_COLOR=0` win over `CLICOLOR_FORCE=1`. These variables
control mise's own output; tools that mise runs have their own color options.

## Configuration and networking

### How do I keep personal configuration out of Git? {#i-don-t-want-to-put-a-mise-toml-tool-versions-file-into-my-project-since-git-shows-it-as-an-untracked-file}

Put personal settings in `mise.local.toml` and keep shared tools and tasks in a
committed `mise.toml`. To ignore `mise.local.toml` in one checkout, add it to
`.git/info/exclude`; to ignore it in every project, add it to your global Git
ignore file.

The same works for keeping `mise.toml` itself private, or the team can list it
in the project's `.gitignore`. Ignore rules apply only to untracked files, so
they do not hide changes to a file that is already in Git.

### What is the difference between "nodejs" and "node" (or "golang" and "go")?

They are the same tool. mise accepts `nodejs` and `golang` as aliases for
`node` and `go` on the command line, in `mise.toml`, and in `.tool-versions`.
When `mise use` changes a `mise.toml` entry written as `nodejs`, it renames the
entry to `node` and keeps its comments. In `.tool-versions`, mise writes
`nodejs` and `golang`, the names asdf reads.

### Why does mise ask me to trust a config file? {#my-config-file-is-being-ignored-mise-trust-issues}

A project's config file can run code: templates can run commands, `[env]`
directives and hooks run scripts, and tool options such as `postinstall` run
during installs. mise loads a file that only lists tool versions,
`min_version`, and plain tasks without asking. Anything else waits until you
trust it, so cloning a repository and entering its directory does not run its
code. Read the file, then run [`mise trust`](/cli/trust.html).

`mise run`, `mise install`, `mise exec`, `mise watch`, and
`mise daemons start`, `restart`, and `register` trust the project's config
themselves, because running them is already a decision to run its code. In CI,
mise treats config as trusted. Outside CI, `--yes` or `MISE_YES=1` answers the
trust prompt with yes, so use them only for configs you have reviewed.
[Paranoid mode](/paranoid.html) turns off this automatic trust. Your global
config, `~/.config/mise/config.toml`, never needs trust.

If mise seems to ignore a config file:

- Run `mise trust --show` to see the trust status of each config directory from
  the current one up. `mise doctor` reports an untrusted file only as
  `failed to load config: error parsing config file: <path>`.
- If you answered No at the trust prompt, mise skips that config without asking
  again. Run `mise trust path/to/mise.toml` after you review it.
- Without a terminal, such as in an editor extension, mise cannot ask, and the
  command fails with
  [`Config files in <path> are not trusted`](/errors.html#untrusted-config).
  Trust the file from a terminal first.
- A `mise.<env>.toml` file loads only when that
  [config environment](/configuration/environments.html) is selected.

See [Configuration trust](/security.html#configuration-trust) for the full
rules, including symlinked configs and trusting every project under a
directory.

### Does mise read `.nvmrc` or `.python-version`? {#how-do-idiomatic-version-files-python-version-node-version-etc-work}

Only for tools you enable. mise reads other version managers' files, such as
`.nvmrc`, `.node-version`, `.python-version`, and `.ruby-version`, only after
you turn them on for a tool:

```sh
mise settings add idiomatic_version_file_enable_tools node  # read .nvmrc, .node-version, package.json
mise settings unset idiomatic_version_file_enable_tools     # stop reading them for every tool
```

To keep one file off while the tool's other files stay on, add a `tool:file`
pair to
[`idiomatic_version_file_disable_files`](/configuration/settings.html#idiomatic_version_file_disable_files),
such as `node:package.json`. See
[Idiomatic version files](/dev-tools/versions.html#idiomatic-version-files) for
the files each tool reads.

### How does mise map a short tool name to a source? {#how-do-the-shorthand-plugin-names-map-to-repositories}

The [registry](/registry.html), which ships with mise, maps short names such as
`ripgrep` to backend identifiers such as `aqua:BurntSushi/ripgrep`. Its source
is the [`registry/`](https://github.com/jdx/mise/tree/main/registry) directory
of the mise repository. Most tools need no plugin.

`mise registry ripgrep` lists the backends a tool can use, and
`mise tool ripgrep` shows the one mise selected. See
[How mise picks a backend](/dev-tools/backends/#how-backend-selection-works).

### How do I use mise with HTTP proxies?

mise reads the standard `https_proxy`, `http_proxy`, `all_proxy`, and
`no_proxy` variables, in lowercase or uppercase:

```sh
export https_proxy=http://proxy.example.com:8080
mise install
```

To route downloads through an internal mirror instead, use
[URL replacements](/url-replacements.html). Package managers and plugins that
mise runs, such as npm, pip, cargo, and asdf plugins, read their own proxy and
certificate settings.

## Coming from asdf {#migration}

### How do I switch from asdf? {#how-do-i-migrate-from-asdf}

<a id="how-compatible-is-mise-with-asdf"></a>

mise reads `.tool-versions` files, so you can try it in one project before you
change your shell setup, and teammates can keep using asdf.
[Migrating from asdf](/dev-tools/comparison-to-asdf.html) gives the steps, the
command equivalents, and how to share `.tool-versions` with asdf users. One
difference to know up front: `mise set` sets environment variables, while
`mise use` chooses tool versions.

## Scope and related tools

### Can mise install system packages, or tools that work without mise? {#mise-is-for-dev-tools-not-applications-or-system-packages}

<a id="how-do-i-install-tools-other-users-can-run-without-mise"></a>

`[tools]` is for per-project versions that mise switches as you change
directories. For something installed once for the whole machine, such as a
system library, a desktop app, or a command other users run without mise, use
one of these:

- [`[bootstrap.packages]`](/bootstrap/packages/) declares packages for apt,
  dnf, Homebrew, WinGet, and other package managers, and `mise bootstrap`
  installs them. mise installs `brew:` and `brew-cask:` entries itself, so
  Homebrew does not need to be installed; see
  [Homebrew formulae](/bootstrap/packages/brew.html).
- [`mise install-into`](/cli/install-into.html) installs one version of any
  tool mise supports into a directory you choose:

  ```sh
  mise install-into node@24 ~/standalone-node
  ~/standalone-node/bin/node --version
  ```

  Use a new or empty directory: `install-into` deletes what is already there
  after asking, or without asking under `--yes`. Add its `bin` directory to
  `PATH`, and set variables such as `JAVA_HOME` yourself.

Both give every user one version, with no switching per project. To give each
user mise-managed tools instead, keep them in `[tools]` and let
[`mise bootstrap`](/bootstrap.html) set up each user's activation and tools.

### Is mise secure?

mise checks downloads where the backend supports it, asks before it runs code
from a project's config, and has safe and paranoid modes for stricter limits.
What each check covers depends on the backend. See [Security](/security.html),
and report vulnerabilities as described in
[Reporting a vulnerability](/security.html#reporting-a-vulnerability).

### What is usage?

[usage](https://usage.jdx.dev/) is a spec for describing a command's arguments,
flags, and completions. mise has it built in: task arguments,
`mise run <task> --help`, and completions for mise and its tasks all come from
usage specs, so you do not need the separate `usage` CLI. See
[Task arguments](/tasks/task-arguments.html).

### What is pitchfork?

[pitchfork](https://pitchfork.jdx.dev/) is a process supervisor for development
services. mise's experimental [daemons](/daemons.html) feature uses it to run
the databases and development servers declared in `mise.toml`, check that they
are ready, and restart them. Use [tasks](/tasks/) for commands that finish, and
daemons for processes that keep running.

### How does mise versioning work?

mise uses calendar versions such as `2026.10.4`, and the numbers say nothing
about compatibility. See [Version numbers](/releases.html#versioning).
