# aqua-registry

Aqua registry backend for [mise](https://mise.jdx.dev).

This crate provides support for the [Aqua](https://aquaproj.github.io/) registry format, allowing mise to install tools from the Aqua ecosystem.

## Features

- Parse and validate Aqua registry YAML files
- Resolve package versions and platform-specific assets
- Template string evaluation for dynamic asset URLs
- Support for checksums, signatures, and provenance verification
- Platform-aware asset resolution for cross-platform tool installation

## Usage

This crate is primarily used internally by mise. For more information about mise, visit [mise.jdx.dev](https://mise.jdx.dev).

## License

MIT
