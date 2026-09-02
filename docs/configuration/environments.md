# Config Environments

It's possible to have separate `mise.toml` files in the same directory for different
environments like `development` and `production`. To enable this, set `MISE_ENV` to an
environment name using one of these methods:

- CLI flag: `-E development` or `--env development`
- Environment variable: `MISE_ENV=development`
- `.miserc.toml` file: `env = ["development"]`

mise then looks for a `mise.{MISE_ENV}.toml` file in the current directory,
parent directories, and the `MISE_CONFIG_DIR` directory.

## Setting MISE_ENV in .miserc.toml

You can set `MISE_ENV` in a `.miserc.toml` file, which is loaded early, before
other config files are discovered. This lets you commit your environment
configuration to version control:

```toml
# .miserc.toml
env = ["development"]
```

### Templates in .miserc.toml

`.miserc.toml` supports [Tera templates](/templates#miserc-template-support),
which is useful for settings like `ceiling_paths` that reference home or XDG directories:

<div v-pre>

```toml
# .miserc.toml

# Stop config search at $HOME
ceiling_paths = ["{{ env.HOME }}"]

# Or use the XDG config home variable
ignored_config_paths = ["{{ xdg_config_home }}/mise/shared.toml"]
```

</div>

Only OS-level context is available (environment variables, `cwd`, `arch()`, `os()`,
etc.); settings from `mise.toml` are not yet loaded at this stage.

File locations searched (in order of precedence):

1. `.miserc.toml` and `.config/miserc.toml` in the current directory and parent directories
2. `~/.config/mise/miserc.toml` (global)
3. `/etc/mise/miserc.toml` (system)

`MISE_ENV` cannot be set in `mise.toml` because it determines which config
files are loaded in the first place.

mise also looks for "local" files like `mise.local.toml` and `mise.{MISE_ENV}.local.toml`
in the current directory and parent directories.
These are not intended to be committed to version control
(add `mise.local.toml` and `mise.*.local.toml` to your `.gitignore` file).

These files take priority in this order (top overrides bottom):

- `mise.{MISE_ENV}.local.toml`
- `mise.local.toml`
- `mise.{MISE_ENV}.toml`
- `mise.toml`

If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, it is used instead of all of the above.

You can also use paths like `mise/config.{MISE_ENV}.toml` or `.config/mise.{MISE_ENV}.toml`. These
follow the order described in [Configuration](/configuration).

## conf.d environments

::: warning Migration in progress
Environment-specific `conf.d` filenames are opt-in until mise 2027.8.10. By default, all non-hidden
TOML fragments still load unconditionally, including names such as `node.tools.toml`.

Dots in unconditional fragment names are deprecated. Rename them to use hyphens (for example,
`node-tools.toml`) before mise 2027.8.10. At that point, the suffix after the first dot will select
an environment.
:::

To opt into the new behavior now, set `env_conf_d = true` in any `miserc.toml` file (see the
locations listed above) or set `MISE_ENV_CONF_D=true`. Files in `mise/conf.d`, `.mise/conf.d`, and
`.config/mise/conf.d` then use the same environment suffixes as other config files:

```text
mise/conf.d/tools.toml                    # always loaded
mise/conf.d/tools.local.toml              # always loaded, usually gitignored
mise/conf.d/tools.development.toml        # MISE_ENV=development
mise/conf.d/tools.development.local.toml  # MISE_ENV=development, usually gitignored
.mise/conf.d/tools.toml                   # always loaded
.mise/conf.d/tools.local.toml             # always loaded, usually gitignored
.mise/conf.d/tools.development.toml       # MISE_ENV=development
.mise/conf.d/tools.development.local.toml # MISE_ENV=development, usually gitignored
```

Because this setting controls config discovery, it must be set in a `miserc.toml` file or the
environment; setting it in `mise.toml` is too late. To keep the old behavior without the
deprecation warning during the migration window, set `env_conf_d = false` explicitly.

Use `mise config` to see which files are being used.

The rules for which file is written to are different, because one file ultimately has to be chosen. See
the docs for [`mise use`](/cli/use.html) for more information.

Multiple environments can be specified, e.g. `MISE_ENV=ci,test`, with the last one taking precedence.

## Platform environments

With the [`auto_env` setting](/configuration/settings.html#auto_env) enabled, mise automatically
treats the following as active config environments, based on the current platform:

| Environment   | Values                                         |
| ------------- | ---------------------------------------------- |
| `{os_family}` | `unix` (not defined on Windows—use `windows`)  |
| `{os}`        | `linux`, `macos`, `windows`                    |
| `{os}-{arch}` | e.g. `linux-x64`, `macos-arm64`, `windows-x64` |

Architectures use mise's remapped names: `x86_64` → `x64` and `aarch64` → `arm64`.

This makes config files like `mise.windows.toml`, `mise.macos-arm64.toml`, or `mise.unix.toml`
load automatically and selects matching lockfiles like `mise.windows.lock`. All of the
usual config file locations and `.local.toml` variants work.

Platform environments have lower precedence than explicit `MISE_ENV` entries. The full order is
(later overrides earlier): `unix` < `{os}` < `{os}-{arch}` < explicit `MISE_ENV` entries.

Platform environments only affect config file discovery and lockfile selection. They are not
added to `MISE_ENV` itself: the `{{ mise_env }}` template variable and the `MISE_ENV` variable
passed to subprocesses and tasks only reflect explicit environments.

### Rollout

`auto_env` is currently **disabled by default**. Starting with mise `2027.6.0`, it will be enabled
by default; from `2026.12.0` until then, mise warns when it finds a platform-specific config file
that would be newly loaded. To control the behavior explicitly:

```toml
# .miserc.toml
auto_env = true # adopt the new behavior now
# or
auto_env = false # keep the old behavior and silence the warning
```

Alternatively, set `MISE_AUTO_ENV=true` / `MISE_AUTO_ENV=false`. Like `MISE_ENV`, this is an early-init
setting: it must be set in `.miserc.toml` or via the environment variable — setting it in
`mise.toml` has no effect because config file discovery has already happened by the time
`mise.toml` is read.
