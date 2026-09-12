# mise-test-tool

A simple test tool for validating mise lockfile functionality across multiple platforms and backends.

## Purpose

This tool is designed to test mise's lockfile generation and validation features by providing:

- Simple Node.js CLI script (no compilation needed)
- Multi-platform support (linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64)
- Environment variable inspection for mise testing
- Platform information reporting

## Usage

```bash
# Direct usage
./bin/mise-test-tool

# Via mise (when integrated)
mise use github:mise-plugins/mise-test-tool@1.0.0
mise-test-tool

# Test lockfile generation
mise lock --platforms macos-arm64,linux-x64 github:mise-plugins/mise-test-tool
```

## Output

The tool prints:

- Version information from package.json
- Current platform (OS and architecture)
- Command line arguments
- MISE\_\* environment variables

## Testing Scenarios

This tool enables testing of:

- Multi-platform lockfile generation
- Platform-specific metadata collection
- Backend compatibility (github, ubi, aqua, http)
- Frozen installs with lockfile validation
- Incremental updates and platform additions

## Implementation

- **Main script**: `bin/mise-test-tool` (Node.js with shebang)
- **Windows shim**: `bin/mise-test-tool.cmd` (batch file)
- **Metadata**: `package.json` (version and package info)

## Integration

This tool will be:

1. Created as a separate GitHub repository: `mise-plugins/mise-test-tool`
2. Integrated into mise via `git subtree` at `test/fixtures/mise-test-tool/`
3. Used for comprehensive lockfile testing across all supported backends
