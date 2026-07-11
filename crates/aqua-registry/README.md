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

## Compiled cache compatibility

The compiled registry cache uses rkyv and is not self-describing. Any change to an archived cache type—including adding, removing, reordering, or changing fields on `AquaPackage` or the compiled registry index—must bump `COMPILED_REGISTRY_CACHE_VERSION` in `src/cache.rs`. This moves new builds to a fresh cache directory instead of attempting to read incompatible bytes.

## License

MIT
