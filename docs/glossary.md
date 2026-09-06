# Glossary

Definitions for the concepts used in the guides and CLI reference. Follow a term's link for
configuration syntax and examples.

## Core Concepts

**Activation**
: The process of loading mise's context (tools, environment variables, PATH modifications) into your shell session. Typically done via `eval "$(mise activate bash)"` in your shell rc file. See [Installing mise](/installing-mise.html) for setup instructions.

**Backend**
: An implementation that resolves versions and installs tools from a particular source. A backend may download releases directly or use a package manager; it is not necessarily a separate program. See [Backends](#backends) below and [Backend Architecture](/dev-tools/backend_architecture) for details.

**Core Tools**
: Built-in tool implementations written in Rust that ship with mise. These provide first-class support for popular languages such as Node.js, Python, Ruby, and Go. See [Core tools](/core-tools) for the full list.

**mise.toml**
: The primary configuration file for mise projects. Contains tool versions, environment variables, tasks, and hooks. See [Configuration](/configuration) for the full specification.

**mise.local.toml**
: A user-local configuration file that overrides `mise.toml`. Typically added to `.gitignore` for personal settings that shouldn't be shared with the team.

**Plugin**
: An extension that adds functionality to mise, such as managing additional tools or setting up environment variables. See [Plugins](/plugins) for an overview.

**Registry**
: The collection of tool aliases that map user-friendly short names to their full backend specifications. For example, `aws-cli` maps to `aqua:aws/aws-cli`. See [Registry](/registry).

**Tool**
: A development tool or runtime that mise can install and manage, such as `node`, `python`, `terraform`, or `jq`.

**Tool Request**
: A user's specification for a tool version, which may be fuzzy or use aliases. Examples: `node@24`, `python@latest`, `go@1.26`. These are resolved to concrete Tool Versions.

**Tool Version**
: A concrete, resolved version of a tool. For example, `node@24` (tool request) might resolve to `node@24.0.0` (tool version).

**Toolset**
: The collection of requested and resolved tools for a specific context, containing all the Tool Versions that should be active for a directory or project.

## Backends

mise supports multiple backends for installing tools from different sources:

**aqua**
: Backend using the [aqua](https://aquaproj.github.io/) registry. Supplies release selection and verification metadata for supported tools. See [aqua backend](/dev-tools/backends/aqua).

**asdf**
: Legacy backend compatible with [asdf](https://asdf-vm.com/) shell-script plugins. Linux and macOS only. Slower than native backends but provides access to the asdf plugin ecosystem. See [asdf backend](/dev-tools/backends/asdf).

**cargo**
: Installs Rust CLI tools using `cargo-binstall` when available and enabled, or compiles them with `cargo install`. See [cargo backend](/dev-tools/backends/cargo).

**conda**
: Downloads and resolves packages from Conda channels directly, without requiring a conda executable. See [conda backend](/dev-tools/backends/conda).

**dotnet**
: Installs .NET tools. See [dotnet backend](/dev-tools/backends/dotnet).

**forgejo**
: Installs tools from Forgejo releases. See [Forgejo backend](/dev-tools/backends/forgejo.html).

**gem**
: Installs Ruby gems as tools. See [gem backend](/dev-tools/backends/gem).

**github**
: Installs tools directly from GitHub releases. See [github backend](/dev-tools/backends/github).

**gitlab**
: Installs tools directly from GitLab releases. See [gitlab backend](/dev-tools/backends/gitlab).

**go**
: Installs Go tools using `go install`. See [go backend](/dev-tools/backends/go).

**http**
: Installs tools from arbitrary HTTP/HTTPS URLs. See [http backend](/dev-tools/backends/http).

**npm**
: Installs Node.js packages and CLI tools from the npm registry. See [npm backend](/dev-tools/backends/npm).

**packslip**
: Installs releases from signed manifests and verifies artifact digests and the signer. See [packslip backend](/dev-tools/backends/packslip.html).

**pipx**
: Installs Python CLI tools in isolated environments using uv by default, or pipx when configured. See [pipx backend](/dev-tools/backends/pipx).

**pkgx**
: Installs packages through pkgx. See [pkgx backend](/dev-tools/backends/pkgx.html).

**s3**
: Downloads tool artifacts from S3 or compatible storage. See [S3 backend](/dev-tools/backends/s3.html).

**spm**
: Installs tools via Swift Package Manager. See [spm backend](/dev-tools/backends/spm).

**ubi**
: Universal Binary Installer for tools distributed as single binaries (deprecated; use the `github` or `aqua` backend instead). See [ubi backend](/dev-tools/backends/ubi).

**vfox**
: Backend compatible with [VersionFox](https://vfox.dev/) plugins. See [vfox backend](/dev-tools/backends/vfox).

## Shell Integration

**hook-env**
: The `mise hook-env` command that exports environment changes for shell integration. Called automatically by the shell hook installed via `mise activate`.

**PATH Activation**
: The default method of shell integration where mise updates the `PATH` environment variable at each prompt to include the appropriate tool binaries.

**Reshim**
: The process of updating the shims directory after tools are installed or removed. Run `mise reshim` if shims get out of sync.

**Shims**
: Executable launchers that intercept tool commands and delegate to mise, which loads the appropriate tool context before execution. An alternative to PATH activation. See [Shims](/dev-tools/shims).

## Configuration

**config_root**
: The canonical project root directory that mise uses when resolving relative paths in configuration files. Derived from the configuration file's location. An imported file can have a different `config_root` from the active project's `MISE_PROJECT_ROOT`.

**Configuration Environments**
: Environment-specific configuration files like `mise.dev.toml` or `mise.prod.toml`, selected with `MISE_ENV`, `mise -E`, or `.miserc.toml`. See [Configuration Environments](/configuration/environments).

**Configuration Hierarchy**
: The system where mise.toml files at different levels (system, global, project) are merged, with files closer to the current directory taking precedence over those in parent directories.

**Settings**
: Options that control mise itself, normally under `[settings]` in a config file. Some can be project-specific; settings marked global-only must be configured globally. See [Settings](/configuration/settings).

**Templates**
: Dynamic values in configuration using Tera template syntax, like <span v-pre>`{{env.HOME}}`</span> or <span v-pre>`{{arch()}}`</span>. See [Templates](/templates).

## Environment Variables

**env.\_ directives**
: Special environment configuration directives for advanced setup:

- `env._.file` - Load variables from a file (e.g., `.env`)
- `env._.path` - Prepend directories to PATH
- `env._.source` - Source a bash script

**Tool-dependent environment**
: Directives with `tools = true` run after the tool environment is available. This is evaluation order, not lazy installation or evaluation only when a variable is read.

**Redaction**
: Masking selected values in output processed by mise. `redact = true` marks an environment value; raw or interactive child output bypasses this processing. See [redaction](/environments/#redactions).

## Hooks

**Hooks**
: Commands triggered by events such as entering a project or installing tools. Shell events require normal activation; installation hooks do not. See [Hooks](/hooks).

**cd hook**
: Runs whenever you change directories while mise is active.

**enter hook**
: Runs when entering a directory where a mise.toml becomes active.

**leave hook**
: Runs when leaving a directory where a mise.toml was active.

**postinstall hook**
: Runs after a tool is successfully installed.

**preinstall hook**
: Runs before a tool installation begins.

**watch_files hook**
: Runs when an activation hook detects changes to matching files. It is not a background watcher; `mise watch` is a separate command.

## Tasks

**Dependency Graph**
: A Directed Acyclic Graph (DAG) used internally to resolve task execution order based on dependencies.

**File Tasks**
: Tasks defined as standalone executable scripts in directories like `mise-tasks/` or `.mise/tasks/`. See [File Tasks](/tasks/file-tasks).

**Task**
: A reusable command defined in mise.toml or as a standalone script that executes within the mise environment. See [Tasks](/tasks/).

**Task Dependencies**
: Relationships between tasks defined via `depends` (run before), `depends_post` (run after), or `wait_for` (wait but don't trigger). See [Task Configuration](/tasks/task-configuration).

**TOML Tasks**
: Tasks defined directly in the `[tasks]` section of mise.toml files. See [TOML Tasks](/tasks/toml-tasks).

## Directories & Environment

**MISE_CACHE_DIR**
: Directory where mise caches downloaded files and metadata. Defaults to `~/.cache/mise` on Linux, `~/Library/Caches/mise` on macOS.

**MISE_DATA_DIR**
: Directory where mise stores installed tools and other persistent data. Defaults to `~/.local/share/mise` on Unix and `%LOCALAPPDATA%\mise` on Windows. See [directories](/directories.html).

**MISE_PROJECT_ROOT**
: The active project root passed to tasks and hooks. Nested configuration layouts such as `.config/mise/config.toml` resolve to the owning project directory, not the config file's immediate parent.

## Other Terms

**Tool Aliases**
: Alternative names for tool backends or tool versions, managed via `mise tool-alias` or the `[tool_alias]` config section. Backend aliases let a short name like `node` point to a custom backend. Version aliases let symbolic names like `lts-iron` map to a concrete version number. See [Tool Aliases](/dev-tools/aliases).

**Shell Aliases**
: Shell command aliases (example: `ll = "ls -la"`) managed via `mise shell-alias` or the `[shell_alias]` config section. They are set dynamically when entering a directory and unset when leaving it, similar to environment variables. Support varies by shell; see the [shell compatibility table](/getting-started.html#shell-feature-compatibility). See [Shell Aliases](/shell-aliases).

**direnv**
: An external tool for environment management that mise can work alongside. See [direnv integration](/direnv).

**mise-en-place**
: French culinary phrase meaning "everything in its place" - the philosophy behind mise. Chefs prepare all ingredients before cooking; developers should have all tools ready before coding.

**mise.lock**
: A file that records concrete versions and supported artifact metadata for selected platforms. It complements `mise.toml`, which records the requested versions. See [mise.lock](/dev-tools/mise-lock).

**Tool Options**
: Configuration in mise.toml that changes tool behavior, such as an HTTP download URL, asset pattern, or backend-specific installation arguments. Python virtualenv activation is an environment directive, not a generic tool option.

**Bootstrap packages**
: Host packages declared in `[bootstrap.packages]`, applied during machine setup. They use a shared system package database or prefix, unlike project-selected `[tools]` versions. See [bootstrap packages](/bootstrap/packages/).
