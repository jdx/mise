# Config Environments

It's possible to have separate `mise.toml` files in the same directory for different
environments like `development` and `production`. To enable, set `MISE_ENV` to an
environment like `development` or `production` using one of these methods:

- CLI flag: `-E development` or `--env development`
- Environment variable: `MISE_ENV=development`
- `.miserc.toml` file: `env = ["development"]`

mise will then look for a `mise.{MISE_ENV}.toml` file in the current directory,
parent directories and the `MISE_CONFIG_DIR` directory.

## Setting MISE_ENV in .miserc.toml

You can set `MISE_ENV` in a `.miserc.toml` file, which is loaded very early before
other config files are discovered. This allows you to commit your environment
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

Note that only OS-level context is available (environment variables, `cwd`, `arch()`, `os()`,
etc.) — settings from `mise.toml` are not yet loaded at this stage.

File locations searched (in order of precedence):

1. `.miserc.toml` and `.config/miserc.toml` in current directory and parent directories
2. `~/.config/mise/miserc.toml` (global)
3. `/etc/mise/miserc.toml` (system)

Note: `MISE_ENV` cannot be set in `mise.toml` because it determines which config
files to load in the first place.

mise will also look for "local" files like `mise.local.toml` and `mise.{MISE_ENV}.local.toml`
in the current directory and parent directories.
These are intended to not be committed to version control.
(Add `mise.local.toml` and `mise.*.local.toml` to your `.gitignore` file.)

The priority of these files goes in this order (top overrides bottom):

- `mise.{MISE_ENV}.local.toml`
- `mise.local.toml`
- `mise.{MISE_ENV}.toml`
- `mise.toml`

If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, that will be used instead of all of this.

You can also use paths like `mise/config.{MISE_ENV}.toml` or `.config/mise.{MISE_ENV}.toml` Those rules
follow the order in [Configuration](/configuration).

Use `mise config` to see which files are being used.

The rules around which file is written are different because we ultimately need to choose one. See
the docs for [`mise use`](/cli/use.html) for more information.

Multiple environments can be specified, e.g. `MISE_ENV=ci,test` with the last one taking precedence.

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
load automatically, and matching lockfiles like `mise.windows.lock` get selected. All of the
usual config file locations and `.local.toml` variants work.

Platform environments have lower precedence than explicit `MISE_ENV` entries. The full order is
(later overrides earlier): `unix` < `{os}` < `{os}-{arch}` < explicit `MISE_ENV` entries.

Platform environments only affect config file discovery and lockfile selection. They are not
added to `MISE_ENV` itself: the `{{ mise_env }}` template variable and the `MISE_ENV` variable
passed to subprocesses and tasks only reflect explicit environments.

### Rollout

`auto_env` is currently **disabled by default**. Starting with mise `2027.6.0` it will default
to enabled; from `2026.12.0` until then, mise warns if it finds a platform-specific config file
that would be newly loaded. To control the behavior explicitly:

```toml
# .miserc.toml
auto_env = true # adopt the new behavior now
# or
auto_env = false # keep the old behavior and silence the warning
```

or set `MISE_AUTO_ENV=true` / `MISE_AUTO_ENV=false`. Like `MISE_ENV`, this is an early-init
setting: it must be set in `.miserc.toml` or via the environment variable — setting it in
`mise.toml` has no effect because config file discovery has already happened by the time
`mise.toml` is read.
