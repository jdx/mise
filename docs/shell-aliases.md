# Shell Aliases

Define project shortcuts for an interactive Bash, Zsh, or Fish shell with
[`mise activate`](/getting-started.html#activate-mise). mise sets the aliases as
you enter a directory and removes them when they are no longer configured.

For commands that also need to work in scripts and CI, define [tasks](/tasks/).
Shell aliases are not available inside tasks or through `mise exec`.

## Configuration

Shell aliases are defined in `mise.toml` under the `[shell_alias]` section:

```toml
[shell_alias]
ll = "ls -la"
la = "ls -A"
gs = "git status"
gc = "git commit"
```

When you enter a directory with this configuration, these aliases are automatically set in your shell. When you leave the directory (and the new directory doesn't define the same aliases), they are unset.

## Supported Shells

Shell aliases are currently supported in:

- **bash** - Uses `alias`/`unalias` commands
- **zsh** - Uses `alias`/`unalias` commands
- **fish** - Uses `alias`/`functions -e` commands

Other shells (nushell, elvish, xonsh, powershell) do not currently support shell aliases.

## Dynamic Behavior

Shell aliases work similarly to environment variables managed by mise:

1. **Set on entry**: When you `cd` into a directory with `[shell_alias]` config, the aliases are set
2. **Updated on change**: If an alias value changes in your config, the alias is updated
3. **Unset on exit**: When you leave the directory (or the alias is removed from config), the alias is unset

```bash
$ cd ~/myproject
# mise sets: alias ll='ls -la'

$ ll
# Runs: ls -la

$ cd ~
# mise runs: unalias ll
```

## Hierarchy

Like other mise config, shell aliases from parent directories are available in child directories. A child directory can override a parent's alias:

```toml [~/projects/mise.toml]
[shell_alias]
build = "make build"
```

```toml [~/projects/myapp/mise.toml]
[shell_alias]
build = "npm run build"  # Overrides parent
```

## Templates

Alias values support [templates](/templates). This Bash/Zsh example quotes the
project path for the shell and runs `node --version` when you invoke the alias:

```toml
[shell_alias]
proj = "cd {{config_root | quote}}"
node_version = "node --version"
```

## Use Cases

### Project-Specific Shortcuts

Define shortcuts that only make sense within a specific project:

```toml
[shell_alias]
dev = "npm run dev"
test = "npm test"
build = "npm run build"
deploy = "./scripts/deploy.sh"
```

### Tool Wrappers

Create aliases that wrap tools with project-specific defaults:

```toml
[shell_alias]
docker-compose = "docker compose -f docker-compose.dev.yml"
terraform = "terraform -chdir=./infrastructure"
```

### Quick Navigation

Quote project paths so directories containing spaces work in Bash and Zsh:

```toml
[shell_alias]
src = "cd {{config_root | quote}}/src"
tests = "cd {{config_root | quote}}/tests"
docs = "cd {{config_root | quote}}/docs"
```

## Limitations

- **Not available in tasks**: Shell aliases are only active in interactive shells where `mise activate` is running. They are **not** available inside TOML task `run` blocks or file tasks, since tasks run in non-interactive subshells. Use the underlying command directly in tasks, or add wrapper scripts to your `PATH` via [`env._.path`](/environments/#env-path).
- **Shell support**: Only bash, zsh, and fish are supported. See the [shell feature compatibility matrix](/getting-started.html#shell-feature-compatibility) for details.

## Comparison to Tool Aliases

mise has two alias features that serve different purposes:

| Feature           | Purpose                                                     | Config Key      |
| ----------------- | ----------------------------------------------------------- | --------------- |
| **Shell Aliases** | Define shell command shortcuts (`alias ll='ls -la'`)        | `[shell_alias]` |
| **Tool Aliases**  | Define version aliases for tools (`node@my-version` → `24`) | `[tool_alias]`  |

See [Tool Aliases](/dev-tools/aliases) for documentation on aliasing tool versions.
