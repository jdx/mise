# Glossary

This glossary defines key terms and concepts used throughout the mise documentation.

## Core Concepts

**Activation**
: The process of loading mise's context (tools, environment variables, PATH modifications) into your shell session. Typically done via `eval "$(mise activate bash)"` in your shell rc file. See [Installing mise](/installing-mise) for setup instructions.

**Backend**
: A package manager or ecosystem that mise uses to install and manage tools. Each backend knows how to fetch, install, and manage tools from its respective source. See [Backends](#backends) below and [Backend Architecture](/dev-tools/backend_architecture) for details.

**Core Tools**
: Built-in tool implementations written in Rust that ship with mise. These provide first-class support for popular languages like Node.js, Python, Ruby, Go, and others. See [Core tools](/core-tools) for the full list.

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
: A user's specification for a tool version, which may be fuzzy or use aliases. Examples: `node@18`, `python@latest`, `go@1.21`. These get resolved to concrete Tool Versions.

**Tool Version**
: A concrete, resolved version of a tool. For example, `node@18` (tool request) might resolve to `node@18.19.0` (tool version).

**Toolset**
: An immutable collection of resolved tools for a specific context, containing all the Tool Versions that should be active for a directory or project.

## Backends

mise supports multiple backends for installing tools from different sources:

**aqua**
: Backend using the [aqua-proj](https://aquaproj.github.io/) registry. Supports SLSA provenance verification and provides access to thousands of tools. See [aqua backend](/dev-tools/backends/aqua).

**asdf**
: Legacy backend compatible with [asdf](https://asdf-vm.com/) shell-script plugins. Linux and macOS only. Slower than native backends but provides access to the asdf plugin ecosystem. See [asdf backend](/dev-tools/backends/asdf).

**cargo**
: Installs Rust tools by compiling them with `cargo install`. See [cargo backend](/dev-tools/backends/cargo).

**conda**
: Installs packages from Conda repositories. See [conda backend](/dev-tools/backends/conda).

**dotnet**
: Installs .NET tools. See [dotnet backend](/dev-tools/backends/dotnet).

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

**pipx**
: Installs Python CLI tools in isolated environments using pipx. See [pipx backend](/dev-tools/backends/pipx).

**spm**
: Installs tools via Swift Package Manager. See [spm backend](/dev-tools/backends/spm).

**ubi**
: Universal Binary Installer for tools distributed as single binaries. See [ubi backend](/dev-tools/backends/ubi).

**vfox**
: Backend compatible with [VersionFox](https://vfox.lhan.me/) plugins. See [vfox backend](/dev-tools/backends/vfox).

## Shell Integration

**hook-env**
: The `mise hook-env` command that exports environment changes for shell integration. Called automatically by the shell hook installed via `mise activate`.

**PATH Activation**
: The default method of shell integration where mise updates the `PATH` environment variable at each prompt to include the appropriate tool binaries.

**Reshim**
: The process of updating the shims directory after tools are installed or removed. Run `mise reshim` if shims get out of sync.

**Shims**
: Small executable scripts that intercept tool commands and delegate to mise, which loads the appropriate tool context before execution. An alternative to PATH activation. See [Shims](/dev-tools/shims).

## Configuration

**config_root**
: The canonical project root directory that mise uses when resolving relative paths in configuration files. Set via the `MISE_PROJECT_ROOT` environment variable or detected automatically.

**Configuration Environments**
: Environment-specific configuration files like `mise.dev.toml` or `mise.prod.toml`, activated via the `MISE_ENV` environment variable. See [Configuration Environments](/configuration/environments).

**Configuration Hierarchy**
: The system where mise.toml files at different levels (system, global, project) are merged together, with files closer to the current directory taking precedence over parent directories.

**Settings**
: Global mise configuration options stored in `~/.config/mise/settings.toml` that define behavior across all projects. See [Settings](/configuration/settings).

**Templates**
: Dynamic values in configuration using Tera template syntax, like <span v-pre>`{{env.HOME}}`</span> or <span v-pre>`{{arch()}}`</span>. See [Templates](/templates).

## Environment Variables

**env.\_ directives**
: Special environment configuration directives for advanced setup:

- `env._.file` - Load variables from a file (e.g., `.env`)
- `env._.path` - Prepend directories to PATH
- `env._.source` - Source a shell script

**Lazy Evaluation**
: Environment variables configured with `tools = true` that can access tool-provided environment variables. These are evaluated after tools are loaded.

**Redaction**
: Marking sensitive environment variables with `redact = true` to hide their values from mise output and logs.

## Hooks

**Hooks**
: Scripts that automatically execute during mise activation at specific events. An experimental feature. See [Hooks](/hooks).

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
: Runs when specified files change. Requires `mise activate` for file watching.

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
: Directory where mise stores installed tools and other persistent data. Defaults to `~/.local/share/mise`.

**MISE_PROJECT_ROOT**
: Environment variable automatically set to the root directory of the current project (where the mise.toml is located).

## Other Terms

**Aliases**
: Alternative names for tool versions, allowing shortcuts like `lts` for Node.js LTS versions. See [Tool Aliases](/dev-tools/aliases).

**direnv**
: An external tool for environment management that mise can work alongside. See [direnv integration](/direnv).

**mise-en-place**
: French culinary phrase meaning "everything in its place" - the philosophy behind mise. Chefs prepare all ingredients before cooking; developers should have all tools ready before coding.

**mise.lock**
: A lockfile that records exact resolved versions for reproducible environments across machines and CI. See [mise.lock](/dev-tools/mise-lock).

**Tool Options**
: Configuration in mise.toml that changes tool behavior, such as setting a Python `virtualenv` path or Node.js `corepack` preferences.
