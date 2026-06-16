# Swift

`mise` can be used to manage multiple versions of [`swift`](https://swift.org/) on the same system. Swift is supported for macos and linux.

## Usage

Use the latest stable version of swift:

```sh
mise use -g swift
swift --version
```

See [a mise guide for Swift developers](https://tuist.dev/blog/2025/02/04/mise) on how to use `mise` with `swift`.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `swift` backend.
These options go in the `[tools]` section in `mise.toml`.

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
