# Mise Technology Stack

## Core Language and Runtime

### Rust (Edition 2024)

- **Minimum version**: 1.85
- **Features**: Full async/await, latest language features
- **Toolchain**: Managed via rustup, using stable channel

### Tokio Async Runtime

- **Version**: Latest stable (1.x)
- **Configuration**: Multi-threaded runtime with all features enabled
- **Thread Pool**: Dynamically sized based on CPU cores (minimum 8 threads)
- **Purpose**: All I/O operations, parallel tool installations, HTTP requests

## Primary Dependencies

### CLI Framework

- **clap**: Version 4.x with derive features
- **Features**: env, derive, string parsing
- **Usage**: All command-line argument parsing and help generation
- **Extensions**: clap_mangen for man page generation

### Configuration Management

- **confique**: Type-safe configuration loading
- **toml/toml_edit**: TOML parsing and manipulation
- **serde/serde_derive**: Serialization framework
- **taplo**: TOML formatting and linting

### HTTP and Networking

- **reqwest**: HTTP client with async support
- **Features**: json, gzip, zstd, charset, http2, macos-system-configuration
- **TLS**: Configurable backends (native-tls, rustls, rustls-native-roots)
- **Purpose**: Tool downloads, API interactions, registry access

### Error Handling

- **eyre**: Enhanced error handling and reporting
- **color-eyre**: Colored error output with context
- **thiserror**: Custom error type derivation
- **async-backtrace**: Async-aware backtraces

### Serialization and Data

- **serde_json**: JSON handling
- **serde_yaml**: YAML configuration support
- **indexmap**: Ordered hash maps
- **rmp-serde**: MessagePack serialization

### Cryptography and Security

- **sha1, sha2, md-5, blake3**: Hash algorithms for verification
- **minisign-verify**: Binary signature verification
- **digest**: Cryptographic hash trait implementations
- **base64**: Base64 encoding/decoding

### File System and Compression

- **tar**: TAR archive handling
- **zip**: ZIP archive support
- **flate2**: Gzip compression
- **xz2**: LZMA compression
- **zstd**: Zstandard compression
- **bzip2**: Bzip2 compression

### Templating and Text Processing

- **tera**: Template engine for environment variable interpolation
- **regex**: Regular expression matching
- **globset**: Glob pattern matching
- **fuzzy-matcher**: Fuzzy string matching

### UI and Terminal

- **console**: Terminal detection and styling
- **indicatif**: Progress bars and spinners
- **color-print**: Colored terminal output
- **terminal_size**: Terminal dimensions
- **comfy-table/tabled**: Table formatting

### Concurrency and Parallelism

- **dashmap**: Concurrent hash map
- **tokio**: Async runtime and utilities
- **async-trait**: Async traits
- **once_cell**: Thread-safe lazy initialization
- **fslock**: File system locking

### Version Management

- **versions**: Semantic version parsing and comparison
- **semver**: Semantic versioning (via versions crate)

### System Integration

- **nix**: Unix system calls and utilities (Unix only)
- **winapi**: Windows API bindings (Windows only)
- **which**: Executable location
- **homedir**: Home directory detection

### Git Integration

- **gix**: Pure Rust Git implementation
- **Features**: worktree-mutation, blocking-http-transport-reqwest-native-tls

## Testing Framework

### Unit Testing

- **Built-in**: Rust's built-in test framework
- **ctor**: Test initialization and setup
- **pretty_assertions**: Enhanced assertion output

### Snapshot Testing

- **insta**: Snapshot testing for CLI output
- **Features**: filters, json support
- **Usage**: Verify CLI command output across changes

### E2E Testing

- **Custom framework**: Located in `e2e/` directory
- **Real installations**: Test actual tool installations
- **Cross-platform**: Separate Windows tests in `e2e-win/`

### Mocking and Fixtures

- **mockito**: HTTP mocking for tests
- **test-log**: Log output in tests

## Build System

### Cargo Configuration

- **Workspace**: Main package + `crates/vfox` subcrate
- **Build script**: `build.rs` for metadata generation
- **Profiles**:
  - `release`: Standard optimized build
  - `serious`: LTO enabled for maximum optimization

### Code Generation

- **built**: Build-time information generation
- **Usage generation**: CLI help and documentation from code

### Cross-compilation

- **Cross.toml**: Configuration for cross-compilation targets
- **Supported targets**: Linux, macOS, Windows (x86_64, aarch64)

## Plugin Ecosystems

### Vfox Plugin System

- **Location**: `crates/vfox/`
- **Language**: Rust + Lua integration
- **Features**: Modern plugin architecture with security

### ASDF Compatibility

- **Plugin support**: Existing ASDF plugin ecosystem
- **Script execution**: Shell script plugin compatibility
- **Migration**: Seamless migration from ASDF

### Core Plugin Implementation

- **Native Rust**: Built-in plugins for major tools
- **Performance**: Fastest execution path
- **Tools**: Node, Python, Go, Java, Ruby, Rust, Swift, Zig, etc.

## External Integrations

### Package Managers

- **cargo**: Rust package installation
- **npm**: Node.js package installation
- **gem**: Ruby gem installation
- **pipx**: Python application installation
- **go install**: Go binary installation

### Tool Registries

- **Aqua Registry**: Security-verified tool registry
- **GitHub Releases**: Direct GitHub release downloads
- **Custom registries**: Support for private tool registries

### Cloud Services

- **GitHub API**: Release information, repository access
- **GitLab API**: Release information for GitLab projects
- **Rate limiting**: Automatic rate limit handling

## Development Tools

### Code Quality

- **clippy**: Rust linter with custom configuration
- **rustfmt**: Code formatting
- **cargo-audit**: Security vulnerability scanning

### Documentation

- **VitePress**: Documentation site generator (in `docs/`)
- **mdbook**: Alternative documentation format
- **cargo doc**: API documentation generation

### Release Management

- **cargo-release**: Automated release management
- **git-cliff**: Changelog generation
- **GitHub Actions**: CI/CD pipeline

## Performance Optimizations

### Caching

- **File system cache**: Tool version caches
- **HTTP cache**: Network request caching
- **Build cache**: Cargo build caching

### Parallelization

- **Tool installations**: Parallel downloads and installs
- **Configuration loading**: Concurrent file parsing
- **Task execution**: Parallel task graph execution

### Memory Management

- **Arc**: Reference counting for shared data
- **LazyLock**: Lazy initialization of expensive resources
- **Streaming**: Stream processing for large files

## Security Features

### Signature Verification

- **minisign**: Binary signature verification
- **GPG**: GPG signature support where available
- **Checksums**: SHA256/SHA512 checksum verification

### Sandboxing

- **Process isolation**: Separate processes for plugin execution
- **File system restrictions**: Limited file system access for plugins
- **Network restrictions**: Controlled network access

## Platform Support

### Operating Systems

- **Linux**: Full support (primary development platform)
- **macOS**: Full support with native features
- **Windows**: Full support with Windows-specific adaptations

### Architecture

- **x86_64**: Primary architecture support
- **aarch64**: ARM64 support (Apple Silicon, ARM Linux)
- **Cross-compilation**: Support for additional architectures

## Deployment and Distribution

### Binary Distribution

- **GitHub Releases**: Pre-compiled binaries
- **Package managers**: cargo-binstall, homebrew, etc.
- **Installation script**: curl installer (mise.run)

### Container Support

- **Docker**: Official Docker images
- **Multi-stage builds**: Optimized container sizes
- **Base images**: Alpine, Ubuntu, Debian support

This technology stack provides a robust, performant, and secure foundation for mise's comprehensive development environment management capabilities.
