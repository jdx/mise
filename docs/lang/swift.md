# Swift <Badge type="warning" text="experimental" />

`mise` can be used to manage multiple versions of [`swift`](https://swift.org/) on the same system. Swift is supported for macos and linux.

## Usage

Use the latest stable version of swift:

```sh
mise use -g swift
swift --version
```

See [a mise guide for Swift developers](https://tuist.dev/blog/2025/02/04/mise) on how to use `mise` with `swift`.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="swift" :level="3" />
