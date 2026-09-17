---
description: "mise can be used to manage multiple versions of swift on the same system."
---

# Swift

`mise` can be used to manage multiple versions of [`swift`](https://swift.org/) on the same system. Swift is supported on macOS and Linux.

## Usage

Install Swift for the current project and check the selected toolchain:

```sh
mise use swift@latest
mise exec -- swift --version
```

Use `mise use -g swift@latest` for a personal default. In an existing Swift package
with `Package.swift`, run `mise exec -- swift build` to build it.

On Linux, Swift archives target specific distributions and require compatible
system libraries. mise records the selected distribution in lockfile options;
use a lock entry built for your target distribution. The Swift core plugin does
not currently support Windows.

### When no build matches your distribution

swift.org publishes one build per distribution family. On a host without a
published build, mise warns and falls back to another family's build. That build
may link against library names your distribution spells differently — an
Arch-family host builds ncurses wide-only, so it has `libncursesw.so.6` where
the fallback build asks for `libncurses.so.6` — and the install then fails when
mise runs `swift --version` to check it. mise names the libraries it could not
resolve:

```
this swift build needs shared libraries missing from this host:
libform.so.6, libncurses.so.6, libpanel.so.6
```

Where the names differ but the libraries are compatible, point the install at a
directory of aliases with [`install_env`](#install_env):

```sh
mkdir -p ~/.local/lib/curses-compat
ln -sf /usr/lib/libncursesw.so.6 ~/.local/lib/curses-compat/libncurses.so.6
ln -sf /usr/lib/libformw.so.6 ~/.local/lib/curses-compat/libform.so.6
ln -sf /usr/lib/libpanelw.so.6 ~/.local/lib/curses-compat/libpanel.so.6
```

```toml
[tools]
swift = { version = "6.3.3", install_env = { LD_LIBRARY_PATH = "{{env.HOME}}/.local/lib/curses-compat" } }
```

The same `LD_LIBRARY_PATH` belongs in `[env]` so the toolchain also works after
it is installed. Substituting a library is a judgement about compatibility that
mise cannot make for you; a distribution that genuinely lacks the library needs
that library installed instead.

See [a mise guide for Swift developers](https://tuist.dev/blog/2025/02/04/mise) for how to use `mise` with `swift`.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `swift` backend.
These options go in the `[tools]` section of `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `swift` backend:

```toml
[tools]
swift = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="swift" :level="3" />
