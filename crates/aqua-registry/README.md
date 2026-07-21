# aqua-registry

Aqua registry primitives for [mise](https://mise.en.dev).

This crate provides support for the [Aqua](https://aquaproj.github.io/) registry format.
It owns parsing, package lookup, package serialization codecs, and the on-disk source/compiled cache layout. mise owns remote fetching policy, baked registry fallback, settings, and integration behavior.

## Features

- Parse and validate Aqua registry YAML files
- Resolve package versions and platform-specific assets
- Template string evaluation for dynamic asset URLs
- Source and compiled registry cache mechanics
- Support for checksums, signatures, and provenance verification
- Platform-aware asset resolution for cross-platform tool installation

## Usage

This crate is primarily used internally by mise. For more information about mise, visit [mise.en.dev](https://mise.en.dev).

## License

MIT
