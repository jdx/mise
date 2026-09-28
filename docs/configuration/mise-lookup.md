---
description: "Use a repository's mise executable for tool resolution and execution in an activated shell."
---

# Select a repository's mise

Use [`activate_mise_lookup`](/configuration/settings.html#activate_mise_lookup) when a repository supplies its own mise executable. This can be a prebuilt binary, a local development build, or an executable launcher. The mise that activates your shell maintains PATH and shell integration; the repository's mise resolves and installs tools and runs project commands.

## Configuration

Add the setting and a directory containing the repository's mise executable to the project's `mise.toml`:

```toml
[settings]
activate_mise_lookup = "env_path"

[env]
_.path = ["{{config_root}}/tools/bin"]
```

Put an executable named `mise` in `tools/bin`. On Windows, use `mise.exe`. Selection searches only trusted project `env._.path` entries. Static paths may be literal or relative and may contain `{{config_root}}`; dynamic templates are rejected in this mode.

Both executables must support this activation mode. The selected mise must support command-name discovery and strict resolution of configured tool commands. An executable that lacks this support causes activation to fail with an upgrade hint. Activation asks the selected mise for command names on checkout entry, so an executable launcher may download or prepare mise at that point.

The default `self` mode keeps the existing full activation behavior.

If you use DotSlash on Windows, place its launcher at `tools/bin/mise.exe` and its manifest at `tools/bin/mise`. Changes to that adjacent manifest refresh command discovery and compatibility validation.

## Responsibilities

| Executable                         | Responsibilities                                                                                                                                                                        |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The mise that activates the shell. | It refreshes the parent shell's PATH with trusted, static `env._.path` entries and dispatcher shims. It handles `mise shell`, `mise sh`, and `mise deactivate`.                         |
| The repository's selected mise.    | It reads the full configuration, enforces `min_version`, resolves and installs tools, and supplies the environment for ordinary `mise` commands and commands invoked through the shims. |

The parent shell receives only PATH changes. Other project environment values, aliases, hooks, and daemons are not applied to that shell. Use [`mise exec`](/cli/exec.html), [`mise run`](/cli/run.html), or a tool's dispatcher shim to run commands with the selected mise's project environment.

Child `mise` commands and known tool commands use dispatcher shims that select the repository's mise from the invoking process's current directory. A child that changes directories can therefore select a different checkout without waiting for the parent shell's next prompt.

## Command lookup and failures

Before a tool is installed, a bare command name needs registry bin metadata or an explicit [`lazy_bins`](/dev-tools/shims.html#lazy-tools) declaration so a shim can exist. An unknown name requires an explicit install or `mise exec` step. Installed tools and configured wrappers also supply command names.

If the selected executable is missing, untrusted, or cannot be acquired, dispatch fails. Refresh retains previously known dispatchers and removes stale checkout PATH entries. If selection fails before the first successful activation, names from trusted `lazy_bins`, registry metadata, and wrappers still receive dispatchers that reject the failed selection instead of running global executables. The activating mise uses these names to create shims without resolving or installing tools.

Refresh repairs deleted dispatch shims. Known commands invoked through dispatchers or `mise exec -- command` must exist in their configured tool installations; a missing executable produces a reinstall hint instead of running a same-named global command. Tools explicitly configured with `version = "system"` and unrelated system commands remain available.

Mise remembers discovered executable names as installation state, so clearing or pruning its cache preserves these checks even if an installed executable has disappeared.

A child that changes into an unvisited checkout may lack a shim for a newly encountered command name. A later user PATH prepend can also take precedence over the shim directory until the next refresh. For scripts and CI, invoke the repository's mise by explicit path and use `mise exec -- command` to load its tool environment.
