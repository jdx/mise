# Zig

`mise` can be used to install and manage multiple versions of [zig](https://ziglang.org/) on the same system.

> The following are instructions for using the zig mise core plugin.

The code for this is inside the mise repository at
[`./src/plugins/core/zig.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/zig.rs).

## Usage

The following installs zig and makes it the global default:

```sh
mise use -g zig@0.14           # install zig 0.14.x
mise use -g zig@latest         # install latest zig release
mise use -g zig@master         # install latest nightly from master
mise use -g zig@2024.11.0-mach # install Mach nominated zig
mise use -g zig@mach-latest    # install latest Mach nominated zig
```

See available stable versions with `mise ls-remote zig`.

Note that [Mach](https://machengine.org/) versions
won't show in `mise ls-remote zig` due to workaround for
[version ordering bug](https://github.com/jdx/mise/discussions/5232).
Despite of that, you still can install Mach versions listed in
[Mach version index](https://machengine.org/zig/index.json). The following
command will list available Mach versions:

```sh
curl https://machengine.org/zig/index.json | yq 'keys'
```

### `master` (nightly channel)

`zig@master` tracks a moving nightly. mise resolves it to the concrete dev version
it currently points at (e.g. `0.17.0-dev.836+...`) at install time, so the install
lands in a versioned directory and `mise upgrade zig` / `mise outdated` pick up
newer nightlies — instead of the channel staying pinned to the build it was first
installed from. Run `mise upgrade zig` (or `mise install -f zig@master`) to move to
the current nightly.

## zig Language Server

The `zig` language server ([zls](https://github.com/zigtools/zls)) needs to be installed separately.
You can install it with `mise`:

```sh
mise use -g zls@0.14   # install zls 0.14.x
mise use -g zls@latest # install latest zls release
```

Note that a tagged release of `zig` should be used with
the same tagged release of `zls`. Currently there is no Mach version of `zls`.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `zig` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `zig` backend:

```toml
[tools]
zig = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="zig" :level="3" />
