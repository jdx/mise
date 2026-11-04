---
description: "Move .envrc settings into mise.toml and remove direnv from a project that mise now manages."
---

# Migrating from direnv

mise sets environment variables, loads dotenv files and adds `PATH` entries when
you enter a directory, so a project that uses [direnv](https://direnv.net) for
those things can drop it. Move the `.envrc` lines into `mise.toml`, then remove
direnv's hook.

::: warning Unsupported
Running direnv and mise together is unsupported. Both change the environment
when you enter a directory, and their shell hooks can disagree about which
`PATH` entries to add, restore or remove. Compatibility issues are not
considered mise bugs, and pull requests for direnv compatibility are not
accepted.
:::

## Do you need direnv? {#do-you-need-direnv}

Each common `.envrc` line has a mise equivalent:

| `.envrc`                            | `mise.toml`                                                             |
| ----------------------------------- | ----------------------------------------------------------------------- |
| `export NODE_ENV=development`       | `NODE_ENV = "development"` under `[env]`                                |
| `dotenv` or `dotenv_if_exists`      | [`_.file = ".env"`](/environments/#env-file); a missing file is skipped |
| `PATH_add bin`                      | [`_.path = "bin"`](/environments/#env-path)                             |
| `source script.sh`                  | [`_.source = "script.sh"`](/environments/#env-source)                   |
| `source_env_if_exists .envrc.local` | A `mise.local.toml` next to `mise.toml`, kept out of version control    |
| `source_up`                         | Nothing; mise already merges the config files of parent directories     |
| `layout python`                     | [`_.python.venv`](/lang/python.html#automatic-virtualenv-activation)    |
| `layout node`                       | `_.path = "node_modules/.bin"`                                          |

mise also does what direnv does not: it puts the project's tool versions on
`PATH`, runs [hooks](/hooks.html) when you enter or leave a project, and sets
[shell aliases](/shell-aliases.html).

## Move an `.envrc` to `mise.toml` {#move-envrc}

For an `.envrc` that sets a variable, loads `.env` and adds `bin` to `PATH`:

```toml [mise.toml]
[env]
NODE_ENV = "development"
_.file = ".env"
_.path = "bin"
```

If the project has no `.env` file, mise skips the `_.file` directive, so you can
leave it in place or remove it. See [Environment variables](/environments/) for
defaults, unsetting values and sourcing scripts.

## Remove direnv {#remove-direnv}

1. Delete the lines you moved from each `.envrc`, or delete the whole file.
2. If you used the `use mise` integration, delete
   `~/.config/direnv/lib/use_mise.sh`.
3. If you no longer use direnv anywhere, remove its hook, such as
   `eval "$(direnv hook zsh)"`, from your shell startup file.
4. [Activate mise](/shell-setup.html) if you have not already, and open a new
   shell.
5. Run `mise env` in the project to check the result. `mise exec -- <command>`
   checks a command without depending on the state of your interactive shell.

## `use mise` in `.envrc` <Badge type="danger" text="deprecated" /> {#mise-inside-of-direnv-use-mise-in-envrc}

The `use mise` integration let direnv load mise's environment. It is deprecated
and unsupported; use [`mise activate`](/shell-setup.html) instead.
`mise direnv` and `mise direnv activate` print a deprecation warning and will be
removed in mise 2027.10.4. The integration gives
direnv control of the exported environment, so it does not run hooks, set shell
aliases or install missing tools.

To find it in an existing setup, look for `use mise` in `.envrc` files, for the
`use_mise` function in `~/.config/direnv/lib/use_mise.sh` (written by
`mise direnv activate`), and for setups that load it from a parent `.envrc`
with `source_up` or from `~/.config/direnv/direnvrc`.

If you keep it while you migrate:

- Do not let both tools manage the same runtime or virtualenv. A common conflict
  is direnv's `layout python` alongside a Python version that mise selects.
- The integration adds only the `.tool-versions` file next to `.envrc` to
  direnv's watch list. direnv does not notice changes to `mise.toml` or to
  config files in other directories, so run `direnv reload` after you edit
  them.

[Shims](/dev-tools/shims.html) are another way to run mise-managed tools without
direnv, but they do not reproduce everything `mise activate` does.
