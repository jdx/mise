---
description: "Look up the terms used across the mise docs, from backends and version requests to trust, shims, and bootstrap."
socialDescription: "Terms used in the mise docs, from backends and version requests to trust and shims."
outline: [2, 3]
---

# Glossary

Each term links to the page that explains it.

## Tools and versions {#tools-and-versions}

### Tool {#tool}

A program that mise installs and puts on `PATH` for a project, such as `node`,
`terraform`, or `jq`. Declare tools under `[tools]` in `mise.toml`. See
[Dev tools](/dev-tools/).

### Backend {#backend}

Where mise gets a tool and how it installs it, such as `aqua`, `github`,
`npm`, or `pypi` (also accepted as `pipx`). Core tools use installers built
into mise. See [Backends](/dev-tools/backends/).

### Backend identifier {#backend-identifier}

A tool name with its backend prefix, such as `aqua:aws/aws-cli` or
`npm:prettier`. Use one in `mise.toml` or on the command line for a tool that
has no registry short name, or to choose a backend yourself. See
[Backend identifiers](/dev-tools/backends/#identifiers).

### Core tools {#core-tools}

Languages whose installers are built into mise, such as Node.js, Python, Ruby,
Go, and Java. They need no plugin. See [Core tools overview](/core-tools.html).

### Registry {#registry}

The list of short tool names that mise knows, each mapped to one or more
backends. For example, `aws-cli` maps to `aqua:aws/aws-cli`. See
[Registry](/registry.html).

### packslip {#packslip}

A format in which a tool's publisher signs a release manifest that lists each
download with its digest. The `packslip:` backend is built into mise: it
verifies the signer and the download before it installs the tool. The registry
prefers this backend for tools that publish packslip releases. See
[packslip backend](/dev-tools/backends/packslip.html).

### Plugin {#plugin}

An extension, written in Lua or as legacy asdf shell scripts, that adds a tool,
a backend, environment directives, or a bootstrap package manager. Most tools
need no plugin. See [Plugins](/plugins.html).

### Toolset {#toolset}

The tools and versions that apply in a directory after mise combines every
config file that applies there. `mise ls --current` lists it.

### Version request {#version-request}

The version you ask for, such as `"24"` in `node = "24"`, `latest`, or
`ref:main`. mise resolves it to a concrete version. See
[Version requests and version files](/dev-tools/versions.html).

### Version prefix {#version-prefix}

A version request that names only the start of a version, such as `node@24` or
`python@3.14`. It matches at separators, so `1.2` matches `1.2.3` but not
`1.20`. See [Request syntax](/dev-tools/versions.html#request-syntax).

### Resolved version {#resolved-version}

The concrete version that a request selects, such as `24.11.1` for
`node = "24"`. mise prefers the version in the lockfile, then a matching
installed version. See
[How a request resolves](/dev-tools/versions.html#how-requests-resolve).

### Pin {#pin}

To record an exact version instead of a prefix. `mise use --pin node@24` writes
the resolved version, such as `node = "24.11.1"`, to the config file. A
[lockfile](#lockfile) records exact versions while the config keeps the prefix.
See [Pin a version or use a lockfile](/dev-tools/versions.html#pin-vs-lockfile).

### Lockfile {#lockfile}

`mise.lock`, a file that records the resolved version of each tool and, for
backends that support it, its download URL and checksum for each platform, so
every machine installs the same build. See
[Lockfile (mise.lock)](/dev-tools/mise-lock.html).

### Idiomatic version file {#idiomatic-version-file}

Another tool's version file, such as `.nvmrc`, `.python-version`, or
`.ruby-version`. mise reads one only for tools you enable with
[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools).
See [Idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

### `.tool-versions` {#tool-versions}

The version file that asdf uses, with one tool per line, such as
`node 24.11.1`. mise reads it alongside `mise.toml`. See
[`.tool-versions`](/dev-tools/versions.html#tool-versions).

### Tool options {#tool-options}

Settings for one tool entry, such as a download URL, an asset pattern, `os`, or
a `postinstall` command, written as a table:
`node = { version = "24", postinstall = "corepack enable" }`. See
[Tool options](/dev-tools/tool-options.html).

### Tool alias {#tool-alias}

A name defined under `[tool_alias]` that points a tool at another backend, or
gives a version request a name. See [Tool aliases](/dev-tools/aliases.html).

### Lazy tool {#lazy-tool}

A tool marked `lazy = true`. `mise install` skips it, and mise installs it the
first time one of its commands runs. See
[Lazy tools](/dev-tools/shims.html#lazy-tools).

### Tool stub {#tool-stub}

An executable file, usually committed to a repository, that names one tool and
version. Running it installs the tool if needed and runs it with your
arguments. See [Tool stubs](/dev-tools/tool-stubs.html).

### Versions host {#versions-host}

[mise-versions](https://mise-versions.jdx.dev), a service that serves version
lists and GitHub release metadata for most tools, so mise makes fewer requests
to GitHub. Turn it off with
[`use_versions_host`](/configuration/settings.html#use_versions_host). See
[A new release is not listed](/troubleshooting.html#new-version-of-a-tool-is-not-available).

### aube {#aube}

A Node.js package manager that is embedded in mise. The `npm` backend installs
`npm:` tools with it by default, so installing them needs neither Node.js nor
npm. To use npm, pnpm, or Bun instead, see
[Choosing an installer](/dev-tools/backends/npm.html#choosing-an-installer).

## Configuration {#configuration}

### mise.toml {#mise-toml}

The project config file. It declares tools, environment variables, tasks,
hooks, and settings. It can also be named `.mise.toml` or live in `.config/`,
`.mise/`, or `mise/`; see [Config file locations](/configuration.html#mise-toml).

### mise.local.toml {#mise-local-toml}

Your personal overrides for a project's `mise.toml`. Keep it out of Git with
`.git/info/exclude` or a global ignore file.

### Global config {#global-config}

`~/.config/mise/config.toml`, which applies in every directory. `mise use -g`
writes to it, and project config overrides it. See
[Global and system config](/configuration.html#global-config).

### System config {#system-config}

Config in `/etc/mise`, such as `/etc/mise/config.toml`, that applies to every
user on the machine. It has the lowest precedence, so global and project config
override it. See [Global and system config](/configuration.html#global-config).

### conf.d {#conf-d}

A directory of `*.toml` fragments that splits config by topic, such as
`~/.config/mise/conf.d/git.toml`. mise loads the fragments in alphabetical
order, before the `config.toml` next to `conf.d`, which overrides them.
Project, global, and system config directories can each have one. See
[Split config with conf.d](/configuration.html#conf-d).

### Config file precedence {#config-file-precedence}

mise loads the config files from the current directory up to the root, plus
global and system config. When two files set the same thing, the file closest
to the current directory wins. `mise config ls` lists the files mise loaded. See
[How config files combine](/configuration.html#configuration-hierarchy).

### Config environment {#config-environment}

An extra config file such as `mise.production.toml`, loaded on top of
`mise.toml` when you select it with `-E production` or `MISE_ENV=production`.
See [Config environments](/configuration/environments.html).

### config_root {#config-root}

The directory that a config file's relative paths resolve against, and the
value of <code v-pre>{{ config_root }}</code> in its templates. For project
config it is the project directory, even when the file lives in `.config/mise/`.
See [config_root](/configuration.html#config-root).

### Project root {#project-root}

The directory of the project you are working in, given to tasks and hooks as
`MISE_PROJECT_ROOT`. In a monorepo, a task gets the root of the subproject that
defines it. See [Task environment](/tasks/running-tasks.html#task-environment).

### Settings {#settings}

Options that control mise itself, set under `[settings]` in a config file, with
`mise settings`, or with `MISE_*` environment variables. Some can only be
set in global config. See [Settings](/configuration/settings.html).

### Tera templates {#templates}

Values such as <code v-pre>{{ env.HOME }}</code> or
<code v-pre>{{ arch() }}</code> that mise renders when it reads a config file.
See [Tera templates](/templates.html).

### Vars {#vars}

Values defined under `[vars]` that [Tera templates](#templates) read as
<code v-pre>{{ vars.NAME }}</code>, such as a version number that several
entries share. Use them for values that mise needs to render config. Unlike
`[env]` values, mise does not export them to commands. See
[Config variables](/configuration/vars.html).

### Directories {#directories}

mise keeps installed tools, cached metadata, global config, and state such as
trust records in separate directories. The defaults and the `MISE_*_DIR`
variables that move them are listed in [Directories](/directories.html).

## Shell and environment {#shell-and-environment}

### Shell activation {#activation}

Running `mise activate` from your shell's startup file, such as
`eval "$(mise activate zsh)"` in `~/.zshrc`. mise then sets `PATH` and `[env]`
variables for the current directory and updates them as you move between
projects. See [Shell setup](/shell-setup.html).

### hook-env {#hook-env}

The internal command that activation runs before each prompt to update the
environment for the current directory. Run it with `MISE_TIMINGS=1` to
[profile a slow prompt](/troubleshooting.html#slow-shell-prompts).

### Shims {#shims}

Small executables named after a tool's commands, such as `node`, kept in a shim
directory. Each one selects the version for the current directory and runs it,
so editors and other programs that never load an activated shell can use mise
tools. See [Shims](/dev-tools/shims.html).

### Reshim {#reshim}

Rebuilding the shims. mise does it when it installs or removes a tool. Run
`mise reshim` after another program adds executables to an installed tool, such
as a global `npm install`. See [Shims](/dev-tools/shims.html#mise-reshim).

### Command wrapper {#command-wrapper}

A command defined under `[wrappers]` that keeps its name but runs another
program, such as `terraform` running `tofu`. See
[Command wrappers](/dev-tools/shims.html#command-wrappers).

### Env directives {#env-directives}

Keys under `[env]` that start with `_.` and do more than set one variable:
`_.file` loads a dotenv file, `_.path` adds directories to `PATH`, `_.source`
runs a script and keeps its exports, and `_.python.venv` activates a
virtualenv. See [Env directives](/environments/#env-directives).

### Tool-dependent environment {#tool-dependent-environment}

An `[env]` entry with `tools = true`, which mise resolves after it adds tools
to `PATH`, so the value can use them. See
[Use values that tools set](/environments/#lazy-eval).

### Secrets {#secrets}

Values such as API keys and passwords that should stay out of plaintext config.
mise secrets, which is experimental, fetches them from
[fnox](/environments/secrets/fnox.html) and passes each one only to the tasks
that list it in `secrets` and to commands run with `--secrets`. A SOPS file or
an age value is decrypted into `[env]` instead, so everything that loads the
config sees it. See [Secrets](/environments/secrets/).

### Redaction {#redaction}

Replacing values marked `redact = true` with `[redacted]` in task output and
logs that mise captures. Raw and interactive task output is not redacted. See
[Redaction and CI masking](/environments/secrets/#redaction).

### Shell aliases {#shell-aliases}

Aliases declared under `[shell_alias]`, such as `ll = "ls -la"`, that mise sets
when you enter a project and removes when you leave. They work in Bash, Zsh,
and Fish. See [Shell aliases](/shell-aliases.html).

### Hooks {#hooks}

Commands that run on an event: `enter`, `leave`, and `cd` when an activated
shell changes directory, `preinstall` and `postinstall` around tool installs,
and `[[watch_files]]` entries when a watched file changes. See
[Hooks](/hooks.html).

### direnv {#direnv}

A separate tool that changes the environment per directory. mise's `[env]`
covers the same needs, and running both together is unsupported. See
[Migrating from direnv](/direnv.html).

## Tasks {#tasks}

### Task {#task}

A named command or script that runs with the project's tools and environment
variables. See [Tasks](/tasks/).

### TOML task {#toml-task}

A task defined under `[tasks]` in `mise.toml`. See
[TOML tasks](/tasks/toml-tasks.html).

### File task {#file-task}

A task defined as an executable script in a task directory such as
`mise-tasks/` or `.mise/tasks/`. See [File tasks](/tasks/file-tasks.html).

### Remote task {#remote-task}

A task whose `file` is an `https://` or `git::` URL instead of a local path.
mise downloads the script, caches it, and runs it, so use only sources you
trust. See [Remote tasks](/tasks/toml-tasks.html#remote-tasks).

### Usage spec {#usage-spec}

A description of a task's arguments and flags in the
[usage](https://usage.jdx.dev) format, written in a TOML task's `usage` field
or in `#USAGE` comments in a file task. mise parses the command line against
it, which gives the task validation, `--help` output, and shell completions,
and passes each value to the script as a `usage_*` environment variable. See
[Task arguments](/tasks/task-arguments.html).

### Task dependencies {#task-dependencies}

The order between tasks: `depends` runs other tasks first, `depends_post` runs
them after, and `wait_for` waits for tasks that are already part of the run.
See [Dependencies and execution order](/tasks/architecture.html).

### Task template {#task-template}

A named set of task properties under `[task_templates.<name>]`, such as a
command, tools, and dependencies, that tasks inherit with `extends`. A template
is not a task, so you cannot run it. It is unrelated to
[Tera templates](#templates). See [Task templates](/tasks/templates.html).

### Monorepo root {#monorepo-root}

The root `mise.toml` of a repository that holds several projects, marked with
`monorepo_root = true`. Each subproject's tasks get a path-based name, such as
`//projects/api:build`, that runs from anywhere in the repository. List the
subprojects in `[monorepo].config_roots`; finding them by walking the
filesystem is deprecated. See [Monorepo tasks](/tasks/monorepo.html).

## Security {#security}

### Trust {#trust}

Your approval for mise to load a project config file that can run code, such
as one with `[env]`, hooks, templates, or tool options. Give it with
`mise trust`. See [Configuration trust](/security.html#configuration-trust).

### Safe mode {#safe-mode}

A mode, turned on with `MISE_SAFE=1`, that loads project config without trust
but skips or refuses everything in it that would run code, such as hooks,
tasks, and `exec()` in templates. Use it for automation that reads config it
does not control. See [Safe mode](/security.html#safe-mode).

### Paranoid mode {#paranoid-mode}

A setting with four categories that you turn on together or one at a time.
`config` requires trust for every project config file, again whenever the file
changes, and turns off automatic trust, including in CI. `env_files` requires
the same approval for dotenv files and sourced scripts. `plugins` refuses to
install an unregistered community plugin by short name. `provenance` re-verifies
provenance on every install. See [Paranoid mode](/paranoid.html).

### Sandbox {#sandbox}

Limits on what a command started by `mise exec` or `mise run` can read, write,
reach on the network, and see in its environment. Turn them on with `--deny-*`
and `--allow-*` flags, the matching task properties, or the `sandbox.*`
settings. The sandbox covers only that command: mise loads config, installs
tools, and runs hooks outside it. See [Sandboxing](/sandboxing.html).

### Checksum {#checksum}

A digest of a downloaded file, such as a SHA-256 hash, that mise compares with
an expected value. A match shows that the bytes are the expected ones, not who
published them. A [lockfile](#lockfile) records checksums so that later
installs get the same bytes. See
[Download verification](/security.html#download-verification).

### Provenance {#provenance}

Signed evidence that ties a release artifact to a signer or to the build that
produced it: SLSA provenance, Cosign and Minisign signatures, and GitHub
artifact attestations. Which of these mise verifies depends on the backend and
on what the project publishes. A lockfile records the provenance mise verified,
and later installs reuse that result. See
[Download verification](/security.html#download-verification).

### Attestation {#attestation}

A GitHub artifact attestation, one kind of [provenance](#provenance). The
`aqua:` and `github:` backends and precompiled Ruby and Python verify one when
a release publishes it, and mise requires it to name the artifact's repository.
See [Download verification](/security.html#download-verification).

### Minimum release age {#minimum-release-age}

A delay, 24 hours by default, before mise picks a newly published version for a
request such as `node@24` or `latest`. It gives a compromised release time to
be noticed. It does not hold back an exact version or one recorded in
`mise.lock`. Change it with
[`minimum_release_age`](/configuration/settings.html#minimum_release_age). See
[Minimum release age](/security.html#minimum-release-age).

## Machine setup {#machine-setup}

### Bootstrap {#bootstrap}

`mise bootstrap`, which sets up a machine from your config: host packages,
system files, services, Git repositories, dotfiles, shell activation, and
tools. See [Bootstrap](/bootstrap.html).

### Bootstrap packages {#bootstrap-packages}

Packages declared in `[bootstrap.packages]` and installed with the host's
package managers, such as apt, Homebrew, or WinGet. The whole machine shares
them, unlike the per-project versions in `[tools]`. See
[Bootstrap packages](/bootstrap/packages/).

### Dotfiles {#dotfiles}

Configuration files in your home directory, such as `~/.zshrc`. mise can keep
a history of their changes, and copy, link, or generate them from your config.
See [Dotfiles](/dotfiles.html).

### Daemon {#daemon}

A process that keeps running between commands, such as a database or a
development server, declared in `mise.toml` and supervised by
[pitchfork](https://pitchfork.jdx.dev/). Daemons are experimental. See
[Daemons](/daemons.html).
