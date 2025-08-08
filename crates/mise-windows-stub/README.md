# mise-windows-stub

Windows stub launcher for mise tool stubs.

## Overview

This crate provides a lightweight Windows executable that acts as a companion to mise tool stub files. Since Windows doesn't support Unix-style shebangs (`#!/usr/bin/env`), this launcher enables tool stubs to work seamlessly on Windows.

## How It Works

1. When a user runs `toolname.exe`, the launcher:
   - Finds the adjacent TOML stub file (`toolname` or `toolname.toml`)
   - Locates the mise executable in PATH or common installation locations
   - Executes `mise tool-stub <stub_file> [args...]`
   - Propagates the exit code from mise

2. The launcher is automatically created when generating tool stubs on Windows using:
   ```bash
   mise generate tool-stub ./bin/mytool --url https://example.com/tool.tar.gz
   ```

## Building

The stub launcher is built automatically as part of the mise build process on Windows:

```bash
cargo build --release -p mise-windows-stub
```

The resulting `mise-stub.exe` is a small (~200KB) standalone executable that gets bundled with mise.

## File Structure

When a tool stub is generated on Windows, two files are created:
- `mytool` - The TOML configuration file with shebang (for cross-platform compatibility)
- `mytool.exe` - The Windows companion executable (copy of mise-stub.exe)

## Design Decisions

- **Small Binary Size**: Optimized for size with LTO, single codegen unit, and symbols stripped
- **No Dependencies**: Pure Rust with only std library, no external crates in production
- **Error Handling**: Clear error messages when stub file or mise executable cannot be found
- **Compatibility**: Works with all Windows versions supported by Rust's std library

## Testing

Run tests with:
```bash
cargo test -p mise-windows-stub
```
