# Complex Configuration Precedence System in Mise

## Overview

The mise project implements a sophisticated hierarchical configuration system that merges settings from multiple sources with complex precedence rules. This system is designed to handle development environments across different scopes (system, global, project, local) while supporting environment-specific configurations and various configuration file formats.

## Key Components

### 1. Configuration File Discovery (`src/config/mod.rs`)

The configuration system is built around several key data structures:

- **`ConfigMap`**: `IndexMap<PathBuf, Arc<dyn ConfigFile>>` - Maps file paths to parsed configuration files
- **`Config`**: Main configuration struct containing merged settings from all sources
- **`LOCAL_CONFIG_FILENAMES`**: Static list defining the precedence order of configuration files

### 2. Configuration File Types

The system supports multiple configuration file formats:

- **`mise.toml`**: Primary TOML configuration format
- **`.tool-versions`**: Legacy format for tool versions
- **Idiomatic version files**: Tool-specific files (e.g., `.node-version`, `.python-version`)
- **Environment-specific configs**: Files like `mise.dev.toml` when `MISE_ENV=dev`

### 3. Precedence Hierarchy

#### File Name Precedence (from `LOCAL_CONFIG_FILENAMES`)

The system defines a strict precedence order for configuration files:

```rust
// Higher precedence (overrides lower)
".tool-versions",
".config/mise/conf.d/*.toml",
".config/mise/config.toml",
".config/mise/mise.toml",
".config/mise.toml",
".mise/config.toml",
"mise/config.toml",
".rtx.toml",
"mise.toml",
".mise.toml",
".config/mise/config.local.toml",
".config/mise/mise.local.toml",
".config/mise.local.toml",
".mise/config.local.toml",
".rtx.local.toml",
"mise.local.toml",
".mise.local.toml",
// Lower precedence
```

#### Directory Hierarchy Precedence

Configuration files are discovered by walking up the directory tree:

```
/
├── etc/mise/config.toml              # System-wide (highest precedence)
└── home/user/
    ├── .config/mise/config.toml      # Global user config
    └── work/
        ├── mise.toml                 # Work-wide settings
        └── myproject/
            ├── mise.local.toml       # Local overrides
            ├── mise.toml             # Project config
            └── backend/
                └── mise.toml         # Service-specific (lowest precedence)
```

**Key principle**: Files closer to the current working directory have higher precedence.

### 4. Environment Variable Overrides

The system provides several environment variables for overriding default behavior:

- **`MISE_OVERRIDE_CONFIG_FILENAMES`**: Completely override the default config file list
- **`MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES`**: Override tool version file names
- **`MISE_GLOBAL_CONFIG_FILE`**: Override global config file location
- **`MISE_SYSTEM_CONFIG_FILE`**: Override system config file location
- **`MISE_IGNORED_CONFIG_PATHS`**: Ignore configs in specific paths

### 5. Configuration Loading Process

The configuration loading follows this process:

1. **Discovery Phase** (`load_config_paths`):
   - Walk up directory tree from current location
   - Collect all config files based on `LOCAL_CONFIG_FILENAMES`
   - Add global and system config files
   - Apply environment variable overrides

2. **Parsing Phase** (`load_all_config_files`):
   - Parse each discovered config file
   - Handle different file formats (TOML, tool-versions, idiomatic)
   - Apply trust/security checks

3. **Merging Phase** (`Config::load`):
   - Merge configurations with proper precedence
   - Handle different merge strategies for different sections

### 6. Merge Strategies by Configuration Section

Different configuration sections use different merge strategies:

#### Tools Section
- **Strategy**: Additive with overrides
- **Behavior**: Tools from all configs are combined, with higher precedence configs overriding specific tool versions

```toml
# Global: node@18, python@3.11
# Project: node@20, go@1.21
# Result: node@20, python@3.11, go@1.21
```

#### Environment Variables
- **Strategy**: Additive with overrides
- **Implementation**: `load_vars` function processes variables in reverse order (lowest to highest precedence)

```rust
let entries = config
    .config_files
    .iter()
    .rev()  // Reverse to process lowest precedence first
    .map(|(source, cf)| {
        cf.vars_entries()
            .map(|ee| ee.into_iter().map(|e| (e, source.clone())))
    })
```

#### Aliases
- **Strategy**: Additive with overrides per backend
- **Implementation**: `load_aliases` function merges aliases from all config files

#### Tasks
- **Strategy**: Complete replacement per task
- **Behavior**: Task definitions completely replace previous definitions

#### Settings
- **Strategy**: Additive with overrides
- **Behavior**: Settings are merged with higher precedence values overriding lower ones

### 7. Advanced Features

#### Environment-Specific Configurations

The system supports environment-specific config files:

```rust
for env in &*env::MISE_ENV {
    filenames.push(format!(".config/mise/config.{env}.toml"));
    filenames.push(format!(".config/mise.{env}.toml"));
    // ... more patterns
}
```

#### Configuration Trust System

The system implements a trust mechanism for configuration files:

- **Trust checks**: Verify file integrity and user consent
- **Ignored paths**: Skip configs in untrusted locations
- **Hash verification**: Detect configuration file changes

#### Glob Pattern Support

Configuration discovery supports glob patterns:

```rust
".config/mise/conf.d/*.toml"  // Load all TOML files in conf.d directory
```

### 8. Performance Optimizations

The configuration system includes several performance optimizations:

- **Lazy loading**: Use `LazyLock` for expensive computations
- **Caching**: Cache parsed configurations and computed paths
- **Efficient data structures**: Use `IndexMap` for ordered collections
- **Parallel processing**: Load multiple config files concurrently where possible

### 9. Configuration Debugging

The system provides tools for debugging configuration precedence:

- **`mise config`**: Show loaded config files in precedence order
- **`mise cfg`**: Display current configuration values
- **Trace logging**: Detailed logging of configuration loading process

### 10. Code Structure

The configuration system is organized across several modules:

- **`src/config/mod.rs`**: Main configuration logic and precedence handling
- **`src/config/config_file/`**: Different configuration file format parsers
- **`src/config/settings.rs`**: Settings management
- **`src/config/env_directive.rs`**: Environment variable handling
- **`src/env.rs`**: Environment variable definitions and overrides

## Key Implementation Details

### Configuration File Resolution

```rust
pub fn load_config_paths(config_filenames: &[String], include_ignored: bool) -> Vec<PathBuf> {
    let dirs = file::all_dirs().unwrap_or_default();
    
    let mut config_files = dirs
        .iter()
        .flat_map(|dir| {
            config_filenames
                .iter()
                .rev()  // Process in reverse order for precedence
                .flat_map(|f| glob(dir, f).unwrap_or_default().into_iter().rev())
                .collect()
        })
        .collect::<Vec<_>>();

    config_files.extend(global_config_files());
    config_files.extend(system_config_files());
    
    // Remove duplicates and apply filters
    config_files.into_iter()
        .unique_by(|p| file::desymlink_path(p))
        .filter(|p| /* trust and ignore checks */)
        .collect()
}
```

### Variable Merging with Precedence

```rust
async fn load_vars(config: &Arc<Config>) -> Result<EnvResults> {
    let entries = config
        .config_files
        .iter()
        .rev()  // Process lowest precedence first
        .map(|(source, cf)| {
            cf.vars_entries()
                .map(|ee| ee.into_iter().map(|e| (e, source.clone())))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    
    EnvResults::resolve(config, config.tera_ctx.clone(), &env::PRISTINE_ENV, entries, options).await
}
```

## Conclusion

The mise configuration system represents a sophisticated approach to hierarchical configuration management that balances flexibility, security, and performance. The complex precedence rules ensure that users can override settings at appropriate levels while maintaining predictable behavior across different environments and use cases.

The system's design allows for:
- **Flexible configuration**: Multiple file formats and locations
- **Predictable precedence**: Clear rules for configuration merging
- **Environment-specific settings**: Support for different deployment environments
- **Security**: Trust mechanisms and path validation
- **Performance**: Optimized loading and caching strategies
- **Debugging**: Tools for understanding configuration resolution

This implementation serves as an excellent example of how to build a robust configuration system for complex development tools.