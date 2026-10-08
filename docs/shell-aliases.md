---
description: "Define shell aliases in mise.toml that mise sets when you enter a project and removes when you leave."
---

# Shell aliases

Define shell aliases in `mise.toml`. In a Bash, Zsh or Fish shell where
[mise is activated](/shell-setup.html), mise sets them when you enter the
directory and removes them when you leave. Other shells ignore `[shell_alias]`;
see [shell feature support](/shell-setup.html#shell-feature-compatibility).

```toml [mise.toml]
[shell_alias]
ll = "ls -la"
gs = "git status"
```

```sh
cd ~/myproject
ll       # runs ls -la
cd ~     # mise removes ll
```

Aliases exist only in your interactive shell. Tasks, `mise exec` and scripts do
not see them, so define a [task](/tasks/) for a command that must also work in
scripts and CI, or put a wrapper script on `PATH` with
[`env._.path`](/environments/#env-path).

These are not [tool aliases](/dev-tools/aliases.html) (`[tool_alias]`), which
give names to tool versions.

## Manage aliases from the CLI {#cli}

[`mise shell-alias`](/cli/shell-alias.html) lists and edits aliases:

```sh
mise shell-alias ls               # aliases active in the current directory
mise shell-alias get ll           # prints ls -la
mise shell-alias set ll "ls -la"  # adds or updates ll in the global config
mise shell-alias unset ll         # removes ll from the global config
```

`set` and `unset` edit only the global config, `~/.config/mise/config.toml`. To
add a project alias, edit `[shell_alias]` in the project's `mise.toml`.

## Parent and child directories {#hierarchy}

Aliases from a parent directory's config apply in its child directories, and a
child's config can override them:

```toml [~/projects/mise.toml]
[shell_alias]
build = "make build"
```

```toml [~/projects/myapp/mise.toml]
[shell_alias]
build = "npm run build"  # overrides the parent's build
```

When you change an alias in config, mise updates it in the shell at the next
prompt.

## Templates {#templates}

Alias values are [Tera templates](/templates.html). Quote paths with the
`quote` filter so directories that contain spaces work in Bash and Zsh:

```toml [mise.toml]
[shell_alias]
proj = "cd {{config_root | quote}}"
src = "cd {{config_root | quote}}/src"
```

## Examples {#use-cases}

```toml [mise.toml]
[shell_alias]
dev = "npm run dev"
t = "npm test"
dc = "docker compose -f docker-compose.dev.yml"
tf = "terraform -chdir=./infrastructure"
```

Avoid names of shell builtins such as `test`, which an alias would shadow in
your interactive shell.

## Limitations {#limitations}

- mise replaces an alias of the same name that your shell already defines, and
  does not restore it when you leave the directory.
- In [safe mode](/security.html#safe-mode) (`MISE_SAFE=1`), mise ignores
  `[shell_alias]` from project config. Aliases from the global config still
  apply.
