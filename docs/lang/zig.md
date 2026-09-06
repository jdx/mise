# Zig

`mise` can be used to install and manage multiple versions of [zig](https://ziglang.org/) on the same system.

## Usage

Install the latest stable Zig for the current project:

```sh
mise use zig@latest
mise exec -- zig version
```

Choose one request for the release stream your project needs:

| Request           | Selects                           |
| ----------------- | --------------------------------- |
| `zig@0.14`        | A release in the 0.14 series      |
| `zig@latest`      | The latest stable release         |
| `zig@master`      | The moving nightly channel        |
| `zig@mach-latest` | The latest Mach-nominated version |

Use `mise use -g <request>` for a personal default. A later `mise use zig@...`
replaces the project's previous Zig request.

See available stable versions with `mise ls-remote zig`.

[Mach](https://machengine.org/) versions
don't appear in `mise ls-remote zig` because of a workaround for a
[version ordering bug](https://github.com/jdx/mise/discussions/5232).
You can still install the Mach versions listed in the
[Mach version index](https://machengine.org/zig/index.json). The following
command lists available Mach versions and requires `curl` and `jq`:

```sh
curl --fail --show-error --silent --location https://machengine.org/zig/index.json | jq 'keys'
```

### `master` (nightly channel)

`zig@master` tracks a moving nightly. mise resolves it to the concrete dev version
it currently points at (e.g. `0.17.0-dev.836+...`) at install time, so the install
lands in a versioned directory and `mise upgrade zig` / `mise outdated` pick up
newer nightlies — instead of the channel staying pinned to the build it was first
installed from. Run `mise upgrade zig` (or `mise install -f zig@master`) to move to
the current nightly.

These instructions use mise's built-in zig support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/zig.rs)
for backend details.

## zig Language Server

The `zig` language server ([zls](https://github.com/zigtools/zls)) needs to be installed separately.
You can install it with `mise`:

```sh
mise use zig@0.14 zls@0.14
mise exec -- zls --version
```

Choose a ZLS version compatible with your Zig version; see the
[ZLS installation guide](https://zigtools.org/zls/install/). Installing both at
`latest` independently is not a compatibility check. There is currently no
Mach-specific ZLS release.

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
