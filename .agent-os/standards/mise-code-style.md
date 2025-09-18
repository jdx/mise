# Mise Code Style Guidelines

## Rust Code Standards

### Code Organization

- **Module Structure**: Follow the established hierarchy in `src/`
- **One Feature Per File**: Keep modules focused and cohesive
- **Public API**: Use `pub` judiciously, prefer `pub(crate)` for internal APIs
- **Re-exports**: Use `mod.rs` for clean module interfaces

### Naming Conventions

- **Structs/Enums**: PascalCase (e.g., `ToolVersion`, `BackendType`)
- **Functions/Variables**: snake_case (e.g., `load_tools`, `config_file`)
- **Constants**: SCREAMING_SNAKE_CASE (e.g., `CORE_PLUGINS`, `VERSION_REGEX`)
- **Type Aliases**: PascalCase (e.g., `ABackend`, `BackendMap`)

### Error Handling

- **Use `eyre::Result<T>`** for all fallible operations
- **Context on errors**: Use `.wrap_err()` or `.context()` for helpful error messages
- **Custom errors**: Define in `src/errors.rs` when needed
- **No unwrap/expect**: In production code, prefer proper error handling

```rust
// Good
pub fn load_config() -> Result<Config> {
    std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("Failed to read config at {}", path.display()))?;
    // ...
}

// Avoid
pub fn load_config() -> Config {
    std::fs::read_to_string(&path).unwrap();  // Don't do this
}
```

### Async/Await Patterns

- **Async functions**: Use `async fn` for I/O operations
- **Tokio runtime**: Already configured in main.rs
- **Parallel execution**: Use `tokio::task::JoinSet` for concurrent operations
- **Blocking operations**: Wrap in `tokio::task::spawn_blocking`

```rust
// Good parallel execution
let mut tasks = JoinSet::new();
for tool in tools {
    tasks.spawn(install_tool(tool));
}
while let Some(result) = tasks.join_next().await {
    result??;
}
```

### Memory Management

- **Avoid unnecessary clones**: Use references where possible
- **Arc for shared data**: Use `Arc<T>` for shared immutable data
- **String vs &str**: Use `&str` for parameters, `String` for owned data
- **Lazy statics**: Use `std::sync::LazyLock` for expensive initialization

### Testing Standards

- **Unit tests**: In same file as code being tested
- **Integration tests**: In `e2e/` directory
- **Snapshot tests**: Use `insta` crate for CLI output testing
- **Test naming**: Descriptive names explaining what is being tested

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_version_with_prefix() {
        let tv = ToolVersion::parse("node@18.0.0").unwrap();
        assert_eq!(tv.tool, "node");
        assert_eq!(tv.version, "18.0.0");
    }
}
```

## Backend Implementation Standards

### Backend Trait Implementation

- **ID consistency**: Backend ID must match registry entry
- **Version parsing**: Handle various version formats (semver, date-based, etc.)
- **Platform support**: Check OS compatibility in `is_supported_os()`
- **Error handling**: Return appropriate errors for network, parsing, etc.

```rust
#[async_trait]
impl Backend for MyBackend {
    fn id(&self) -> &str { "my-tool" }

    fn get_type(&self) -> BackendType { BackendType::Core }

    async fn list_remote_versions(&self) -> Result<Vec<String>> {
        // Implementation with proper error handling
    }

    async fn install_version(&self, ctx: &InstallContext) -> Result<()> {
        // Implementation with progress reporting
    }
}
```

### Plugin Standards

- **Core plugins**: Implement directly in Rust for best performance
- **External plugins**: Use ASDF/vfox compatibility when possible
- **Metadata**: Include `mise.plugin.toml` for plugin configuration
- **Testing**: Each plugin must have comprehensive tests

## Configuration Standards

### TOML Structure

- **Consistent formatting**: Use `taplo` for TOML formatting
- **Comments**: Document complex configuration options
- **Schema validation**: Use JSON schema for validation
- **Backwards compatibility**: Maintain compatibility when possible

```toml
# Good structure
[tools]
node = "20"              # Comment explaining version choice
python = { version = "3.12", virtualenv = ".venv" }

[env]
DATABASE_URL = "postgres://localhost/myapp"
NODE_ENV = "development"

[tasks.build]
description = "Build the application"
run = "cargo build --release"
```

### Settings Management

- **settings.toml**: Source of truth for all settings
- **Code generation**: Settings auto-generate Rust code
- **Documentation**: Settings auto-generate documentation
- **Validation**: Use proper types and validation rules

## CLI Standards

### Command Structure

- **Subcommands**: One file per major command in `src/cli/`
- **Arguments**: Use clap derive macros consistently
- **Help text**: Provide clear, concise help for all commands
- **Examples**: Include usage examples in help text

```rust
/// Install and use a tool version
#[derive(Debug, clap::Args)]
pub struct Use {
    /// Tools to install and use
    #[clap(value_name = "TOOL[@VERSION]")]
    pub tool: Vec<ToolArg>,

    /// Save to global config instead of project config
    #[clap(short, long)]
    pub global: bool,
}
```

### Output Standards

- **Progress reporting**: Use progress bars for long operations
- **Color support**: Respect NO_COLOR and terminal capabilities
- **Structured output**: Support JSON output for scripting
- **Error messages**: Clear, actionable error messages

## Documentation Standards

### Code Documentation

- **Public APIs**: Document all public functions and structs
- **Examples**: Include usage examples in doc comments
- **Panics/Errors**: Document when functions can panic or error
- **Safety**: Document any unsafe code usage

````rust
/// Installs a tool version to the specified directory
///
/// # Arguments
/// * `ctx` - Installation context with tool and version info
///
/// # Errors
/// Returns an error if the tool cannot be downloaded or installed
///
/// # Example
/// ```
/// let ctx = InstallContext::new(tool, version, install_path);
/// backend.install_version(&ctx).await?;
/// ```
pub async fn install_version(&self, ctx: &InstallContext) -> Result<()> {
    // Implementation
}
````

### README and Guides

- **Clear structure**: Use headings and bullet points effectively
- **Code examples**: Include working code examples
- **Links**: Link to relevant documentation sections
- **Keep updated**: Update docs when code changes

## Performance Standards

### Optimization Guidelines

- **Lazy initialization**: Don't compute until needed
- **Parallel operations**: Use async/await for I/O bound operations
- **Caching**: Cache expensive computations and network requests
- **Memory usage**: Be mindful of memory allocation patterns

```rust
// Good: Lazy loading
static TOOLS: LazyLock<BackendMap> = LazyLock::new(|| {
    load_all_tools()
});

// Good: Parallel downloads
let futures: Vec<_> = versions.iter()
    .map(|v| download_version(v))
    .collect();
let results = try_join_all(futures).await?;
```

### Profiling and Monitoring

- **Timing macros**: Use `time!()` macro for performance monitoring
- **Progress reporting**: Show progress for long-running operations
- **Metrics**: Collect metrics where appropriate
- **Benchmarking**: Profile performance-critical paths

## Security Standards

### Input Validation

- **Sanitize inputs**: Validate all external inputs
- **Path traversal**: Prevent directory traversal attacks
- **Command injection**: Sanitize shell commands
- **Version validation**: Validate version strings

```rust
// Good: Input validation
fn validate_version(version: &str) -> Result<()> {
    if !VERSION_REGEX.is_match(version) {
        bail!("Invalid version format: {}", version);
    }
    Ok(())
}
```

### File Operations

- **Permissions**: Set appropriate file permissions
- **Atomic operations**: Use atomic file operations when possible
- **Temp files**: Clean up temporary files
- **Symlinks**: Handle symlinks securely

### Network Operations

- **HTTPS only**: Use HTTPS for all network operations
- **Certificate validation**: Validate SSL certificates
- **Timeouts**: Set reasonable timeouts for network operations
- **Rate limiting**: Respect API rate limits

## Commit and PR Standards

### Conventional Commits

- **Format**: `<type>(<scope>): <description>`
- **Types**: feat, fix, refactor, docs, style, perf, test, chore
- **Scopes**: Use appropriate scope (cli, config, backend, etc.)
- **Description**: Clear, concise description of changes

### Code Review

- **Small PRs**: Keep PRs focused and reviewable
- **Tests included**: Include tests for all new functionality
- **Documentation**: Update documentation for public API changes
- **Breaking changes**: Clearly mark and document breaking changes

### Pre-commit Checklist

1. Run `mise run lint-fix` and commit any fixes
2. Run relevant tests: `mise run test:e2e [test_files]`
3. Update documentation if needed
4. Verify commit message follows conventional format
5. Check for any sensitive information in commit
