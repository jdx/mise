---
description: "Load extra config files such as mise.production.toml by selecting a config environment with -E or MISE_ENV."
socialDescription: "Load extra config files such as mise.production.toml with -E or MISE_ENV."
---

# Config environments

A config environment loads extra config files, such as `mise.development.toml`
or `mise.production.toml`, on top of `mise.toml`. Select one with `-E` or
`MISE_ENV`; the base `mise.toml` still loads, and the environment file overrides
it in the same directory.

Selecting a config environment does not set variables such as `NODE_ENV` for
your application. Define those in [`[env]`](/environments/) in the environment
file.

## Try an environment

::: code-group

```toml [mise.toml]
[env]
APP_MODE = "development"
```

```toml [mise.production.toml]
[env]
APP_MODE = "production"
```

:::

```sh
mise exec -- sh -c 'echo "$APP_MODE"'               # development
mise -E production exec -- sh -c 'echo "$APP_MODE"' # production
mise -E production config                           # list the loaded files
```

## Select environments

You cannot select an environment in `mise.toml`, because the selection decides
which config files load. Use one of these instead, listed from highest to
lowest precedence:

| Method               | Example                                                               |
| -------------------- | --------------------------------------------------------------------- |
| CLI flag             | `mise -E production run deploy` or `--env production`                 |
| Environment variable | `MISE_ENV=production`                                                 |
| `.miserc.toml`       | `env = ["production"]`; see [below](#setting-mise-env-in-miserc-toml) |

To select several environments, separate them with commas:
`mise -E ci,test run build` or `MISE_ENV=ci,test`. When two selected
environments set the same value in one directory, the last one wins among files
of the same kind, but a local file still overrides every shared one (see
[File names and precedence](#file-names-and-precedence)): with `ci,test`,
`mise.ci.local.toml` overrides `mise.test.toml`. Run `mise -E ci,test config` to
see the combined selection.

### Set a default in .miserc.toml {#setting-mise-env-in-miserc-toml}

Commit a default selection in `.miserc.toml`, which mise reads before any other
config file:

```toml [.miserc.toml]
env = ["development"]
```

Use `.miserc.local.toml` (not committed) for a personal choice, and
`~/.config/mise/miserc.local.toml` for one machine. See
[`.miserc.toml`](/configuration.html#miserc) for every location and how the
files combine.

## File names and precedence {#file-names-and-precedence}

In a project directory, a file higher in this table overrides one lower down:

| File                    | Use                                                   |
| ----------------------- | ----------------------------------------------------- |
| `mise.<env>.local.toml` | Personal overrides for one environment, not committed |
| `mise.local.toml`       | Personal overrides, not committed                     |
| `mise.<env>.toml`       | Shared config for one environment                     |
| `mise.toml`             | Shared config                                         |

So your `mise.local.toml` overrides a committed `mise.production.toml` when the
`production` environment is selected. To override a value for one environment
only, put it in `mise.production.local.toml`. Add `mise.local.toml` and
`mise.*.local.toml` to `.gitignore`.

The other project locations take environment names the same way, such as
`mise/config.<env>.toml`, `.mise/config.<env>.toml`, and
`.config/mise.<env>.toml`; see [Config file locations](/configuration.html#mise-toml).
In the same directory, environment files such as `mise/config.<env>.toml`
override every shared file without an environment, local files such as
`mise/config.local.toml` override both, and environment local files such as
`mise/config.<env>.local.toml` override all of them. Within each of these
layers, a later selected environment wins.

The global config directory (`~/.config/mise`) uses `config.<env>.toml` and
`config.<env>.local.toml` in the same order, so `config.local.toml` overrides
`config.<env>.toml`.

If [`override_config_filenames`](/configuration/settings.html#override_config_filenames)
is set, its filenames replace `mise.toml`, `mise.local.toml`, and the other
default names. Environment files such as `mise.<env>.toml` and
`mise.<env>.local.toml` are still loaded.

## Write to an environment file

`mise use` and `mise set` write to the shared file unless you name an
environment with the subcommand's own `--env` flag:

```sh
mise use --env staging node@24                             # writes mise.staging.toml
mise set --env staging API_URL=https://staging.example.com # writes mise.staging.toml
```

The global `-E` placed before the subcommand, as in
`mise -E staging set ...`, only selects which files load; the write still goes
to the shared `mise.toml`. Both commands create the environment file if it does
not exist. See
[which file mise writes to](/configuration.html#target-file-for-write-operations)
for the full rules.

## Use the environment in tasks and templates

mise exports the selected environments to tasks and other commands it runs as
`MISE_ENV`, separated by commas. Templates read the same list as
<code v-pre>{{ mise_env }}</code>:

```mise-toml [mise.toml]
[tasks.deploy]
run = """
{% if mise_env is not defined %}echo "select an environment with -E" >&2; exit 1{% endif %}
echo deploying to {{ mise_env | first }}
"""
```

`mise -E staging run deploy` prints `deploying to staging`, and `mise run deploy`
fails with `select an environment with -E`. When no environment is selected,
`mise_env` is undefined. A bare <code v-pre>{{ mise_env }}</code> then fails to
render, and so do filters such as `join`, but `first` returns an empty value
without an error. Check `mise_env is defined` before you use it.
[Platform environments](#platform-environments) are not included in `MISE_ENV`
or `mise_env`.

## conf.d environments

::: warning Migration in progress
Environment-specific `conf.d` filenames are opt-in until mise 2027.8.10. By
default, every non-hidden TOML fragment still loads, including names such as
`node.tools.toml`.

Dots in fragment names are deprecated. Rename them to use hyphens, such as
`node-tools.toml`, before mise 2027.8.10. From that release, the suffix after
the first dot selects an environment.
:::

To opt in now, set `env_conf_d = true` in a `miserc.toml` file or set
`MISE_ENV_CONF_D=true`. Fragments in `mise/conf.d`, `.mise/conf.d`, and
`.config/mise/conf.d` then use the same environment suffixes as other config
files:

```text
mise/conf.d/tools.toml                    # always loaded
mise/conf.d/tools.local.toml              # always loaded, usually gitignored
mise/conf.d/tools.development.toml        # MISE_ENV=development
mise/conf.d/tools.development.local.toml  # MISE_ENV=development, usually gitignored
```

Because this setting controls config discovery, set it in `miserc.toml` or the
environment; in `mise.toml` it is too late. To keep the old behavior without the
deprecation warning during the migration, set `env_conf_d = false` explicitly.

## Platform environments

With the [`auto_env`](/configuration/settings.html#auto_env) setting enabled,
mise also treats these as active config environments, based on the current
platform:

| Environment   | Values                                               |
| ------------- | ---------------------------------------------------- |
| `{os_family}` | `unix` (not defined on Windows; use `windows`)       |
| `{os}`        | `linux`, `macos`, `windows`                          |
| `{os}-{arch}` | such as `linux-x64`, `macos-arm64`, or `windows-x64` |

Architectures use mise's names: `x86_64` becomes `x64` and `aarch64` becomes
`arm64`.

Files such as `mise.windows.toml`, `mise.macos-arm64.toml`, and
`mise.unix.toml` then load automatically, in every config location and with
their `.local.toml` variants, and mise selects matching lockfiles such as
`mise.windows.lock`.

Platform environments have lower precedence than environments you select. From
lowest to highest: `unix`, `{os}`, `{os}-{arch}`, then your `-E` or `MISE_ENV`
entries. As with several selected environments, this order applies among files
of the same kind, so `mise.linux.local.toml` still overrides `mise.ci.toml`.
They affect only config file discovery and lockfile selection, so
<code v-pre>{{ mise_env }}</code> and the `MISE_ENV` variable passed to tasks
list only the environments you selected.

### Rollout

`auto_env` is off by default. mise 2027.6.0 turns it on by default, and from
2026.12.0 until then mise warns when it finds a platform config file that the
new default would load. To choose now, set it in `.miserc.toml`:

```toml [.miserc.toml]
auto_env = false # keep the old behavior and silence the warning
```

Set `auto_env = true` to adopt the new behavior, or use `MISE_AUTO_ENV=true` or
`MISE_AUTO_ENV=false`. Like `env`, this setting controls config discovery, so it
has no effect in `mise.toml`.

## Related

- [Bootstrap modules](/bootstrap/modules.html): one environment file per machine
  role, selected on each machine
