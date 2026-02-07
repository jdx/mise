# Shell Aliases

mise can manage shell aliases that are set dynamically when you enter a directory and unset when you leave, similar to how environment variables work.

## Configuration

Shell aliases are defined in `mise.toml` under the `[shell_alias]` section:

```toml
[shell_alias]
ll = "ls -la"
la = "ls -A"
gs = "git status"
gc = "git commit"
```

When you enter a directory with this configuration, these aliases will be automatically set in your shell. When you leave the directory (and the new directory doesn't have the same aliases), they will be unset.

## Supported Shells

Shell aliases are currently supported in:

- **bash** - Uses `alias`/`unalias` commands
- **zsh** - Uses `alias`/`unalias` commands
- **fish** - Uses `alias`/`functions -e` commands

Other shells (nushell, elvish, xonsh, powershell) do not currently support shell aliases.

## Dynamic Behavior

Shell aliases work similarly to environment variables managed by mise:

1. **Set on entry**: When you `cd` into a directory with `[shell_alias]` config, the aliases are set
2. **Updated on change**: If an alias value changes in your config, it will be updated
3. **Unset on exit**: When you leave the directory (or the alias is removed from config), it will be unset

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

```toml
# ~/projects/mise.toml
[shell_alias]
build = "make build"

# ~/projects/myapp/mise.toml
[shell_alias]
build = "npm run build"  # Overrides parent
```

## Templates

Alias values support [templates](/templates), allowing dynamic values:

```toml
[shell_alias]
proj = "cd {{config_root}}"
node_version = "echo {{exec(command='node --version')}}"
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

```toml
[shell_alias]
src = "cd {{config_root}}/src"
tests = "cd {{config_root}}/tests"
docs = "cd {{config_root}}/docs"
```

## Comparison to Tool Aliases

mise has two different alias features that serve different purposes:

| Feature           | Purpose                                                | Config Key      |
| ----------------- | ------------------------------------------------------ | --------------- |
| **Shell Aliases** | Define shell command shortcuts (`alias ll='ls -la'`)   | `[shell_alias]` |
| **Tool Aliases**  | Define version aliases for tools (`node@lts` → `20.x`) | `[tool_alias]`  |

See [Tool Aliases](/dev-tools/aliases) for documentation on aliasing tool versions.
