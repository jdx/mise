# aqua-registry

Reads the [aqua](https://aquaproj.github.io/) registry format for
[mise](https://mise.jdx.dev). mise's
[aqua backend](https://mise.jdx.dev/dev-tools/backends/aqua.html) uses this
crate to turn a registry entry into the release asset to download for a tool
version and platform.

The crate:

- parses registry YAML files and looks up a package by ID
- applies a package's version overrides and platform rules
- evaluates aqua's template strings to produce asset names and URLs
- reads each package's checksum, signature and provenance settings
- stores registry sources and compiled packages in an on-disk cache

mise does the rest: it fetches registries, falls back to the registry built
into the binary, reads settings, and verifies downloads.

This is an internal component of mise; its API may change between releases.

## License

MIT
