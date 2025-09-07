# Implement `mise lock` Command with Multi-Platform Support

## Overview

**Feature:** Implement `mise lock` command to generate lockfiles without installing tools  
**Status:** Specification  
**Priority:** High  
**Epic:** Lockfile System Implementation  

## Problem Statement

Currently, `mise lock` is largely unimplemented (only shows analysis). Teams need a way to generate lockfiles with exact tool versions, URLs, and checksums for multiple platforms without requiring installation or being on each target platform. This enables:

1. Generate lockfiles that work across multiple platforms without requiring each platform to run `mise lock`
2. Ensure consistent tool versions and checksums across development, CI, and production environments
3. Pre-populate lockfiles for target deployment platforms during development
4. Maintain security through checksum verification for all target platforms

## Solution Overview

Extend `mise lock` to support multi-platform targeting. The command already has a `--platform` flag (short `-p`) that accepts comma-separated platforms. The implementation will build upon the existing CLI structure to fetch platform-specific metadata (URLs, checksums, sizes) without performing full installations.

## Technical Specification

### CLI Interface

#### Current Flags (Placeholder Implementation)
- `-p, --platform <PLATFORMS>`: Comma-separated list of target platforms (parsing only)
- `--force`: Update even if data exists (parsing only)
- `--dry-run`: Show changes without saving (working for analysis)
- `--jobs <N>`: Number of concurrent jobs (parsing only)
- `tool`: Positional arguments for specific tools (filtering works)

#### Planned Flag Changes
- Rename `-p, --platform` to `-p, --platforms` for clarity
- Implement actual functionality for `--force` flag
- Add `--check` flag for lockfile validation without modification

#### Platform Format
- `<os>-<arch>` (e.g., `macos-arm64`, `linux-x64`, `windows-x64`)
- Extended format: `<os>-<arch>-<qualifier>` (e.g., `linux-x64-gnu`, `linux-x64-musl`)

### Backend Architecture Overview

Mise supports multiple types of backends for tool management. Each backend type handles lockfile generation differently:

#### Core Tools (Backend: Core)
Built-in tools with native mise implementations:
- **node** - Node.js from nodejs.org (SHASUMS256.txt parsing)
- **python** - Python from python.org (releases API)
- **java** - Java from Eclipse Adoptium/OpenJDK (API integration)
- **go** - Go from go.dev (downloads API)
- **ruby** - Ruby from ruby-lang.org (releases)
- **rust** - Rust from forge.rust-lang.org (releases)
- **swift** - Swift from swift.org (releases)
- **zig** - Zig from ziglang.org (downloads API)
- **deno** - Deno from GitHub releases
- **bun** - Bun from GitHub releases
- **elixir** - Elixir from GitHub releases
- **erlang** - Erlang from erlang.org

#### Package Manager Backends
Tools distributed through package registries:
- **npm** - npm registry for Node.js packages (`npm:package-name`)
- **cargo** - crates.io for Rust packages (`cargo:package-name`)
- **gem** - RubyGems for Ruby gems (`gem:package-name`)  
- **pipx** - PyPI for Python CLI tools (`pipx:package-name`)
- **go** (backend) - Go modules proxy (`go:module-name`)
- **spm** - Swift Package Manager (`spm:package-name`)
- **dotnet** - NuGet for .NET tools (`dotnet:package-name`)

#### Distribution Backends
Tools from generic distribution sources:
- **github** - GitHub releases (`github:owner/repo`)
- **gitlab** - GitLab releases (`gitlab:owner/repo`)
- **http** - Generic HTTP downloads (`http:tool-name`)
- **ubi** - GitHub releases with smart asset detection (`ubi:owner/repo`)
- **aqua** - Aqua registry tools (`aqua:package-name`)

#### Plugin Backends
Plugin-based tool management:
- **asdf** - ASDF plugin ecosystem (`asdf:plugin-name`)
- **vfox** - VFox plugin ecosystem (`vfox:plugin-name`)
- **custom** - User-written plugin backends (`custom:backend-name`)

#### Backend-Tool Relationship Examples
```bash
# Core tools - single backend, direct implementation
mise use node@20.0.0           # Uses Core backend, Node.js implementation
mise use python@3.11.0         # Uses Core backend, Python implementation

# Package managers - tool name matches backend
mise use npm:typescript@5.0.0  # Uses NPM backend for TypeScript package  
mise use cargo:ripgrep@13.0.0  # Uses Cargo backend for ripgrep crate
mise use pipx:black@23.0.0     # Uses Pipx backend for Black formatter

# Distribution - generic backends with specific tools
mise use github:cli/cli@2.0.0  # Uses GitHub backend for GitHub CLI
mise use ubi:BurntSushi/ripgrep@13.0.0  # Uses UBI backend with asset detection

# Plugin backends - community extensions  
mise use asdf:terraform@1.5.0  # Uses ASDF backend with Terraform plugin
mise use vfox:kubectl@1.28.0   # Uses VFox backend with kubectl plugin

# Note: Some tools available through multiple backends
mise use go@1.21.0            # Core backend - official Go releases
mise use ubi:golang/go@1.21.0 # UBI backend - GitHub releases (same source)
```

#### Lockfile Generation Priority
1. **Core Tools**: Highest priority, most reliable platform support
2. **Distribution Backends**: GitHub/HTTP sources, good platform support
3. **Package Managers**: Limited platform support, mostly source-based
4. **Plugin Backends**: Varies by plugin implementation

#### Default Behavior Logic
```rust
// 1. Determine lockfile paths (handles MISE_ENV)
let lockfile_paths = resolve_lockfile_paths(env::MISE_ENV.as_deref());

for lockfile_path in lockfile_paths {
    // 2. Load existing lockfile if it exists
    let existing_lockfile = if lockfile_path.exists() {
        Some(Lockfile::read(&lockfile_path)?)
    } else {
        None
    };
    
    // 3. Determine target platforms
    let target_platforms = if platforms_specified_via_flag {
        specified_platforms
    } else if let Some(lockfile) = &existing_lockfile {
        if lockfile.has_platform_data() {
            lockfile.extract_platforms()  // Update existing platforms
        } else {
            vec![current_platform()]      // First time with platforms
        }
    } else {
        vec![current_platform()]         // New lockfile
    };
    
    // 4. Determine target tools
    let target_tools = if tool_args_specified {
        specified_tools_from_args
    } else if let Some(lockfile) = &existing_lockfile {
        lockfile.extract_tools()         // Update existing tools
    } else {
        resolve_tools_from_config(env_context)  // New lockfile from config
    };
    
    // 5. Generate/update lockfile
    let mut lockfile = existing_lockfile.unwrap_or_default();
    update_lockfile_with_platform_info(&mut lockfile, target_tools, target_platforms);
    
    // 6. Save updated lockfile
    lockfile.save(&lockfile_path)?;
}
```

**Update Behavior Summary:**
- **New lockfile**: Create from current config + current platform
- **Existing lockfile + no flags**: Update all existing tools for all existing platforms  
- **Existing lockfile + --platforms**: Update all existing tools for specified platforms
- **Existing lockfile + tool args**: Update specified tools for all existing platforms
- **Existing lockfile + both flags**: Update specified tools for specified platforms
- **--force**: Re-fetch all metadata even if already exists

#### Preserved Flags
- `--force`: Re-fetch all platform metadata
- `--dry-run`: Show changes without saving
- `--jobs <N>`: Concurrent platform fetching

#### Usage Examples
```bash
# Create new lockfile from config
mise lock                                # Creates mise.lock with tools from config + current platform

# Target specific platforms
mise lock --platforms macos-arm64,linux-x64,windows-x64  # Create/update with specific platforms

# Update existing lockfile (preserves existing platforms and tools)
mise lock                                # Updates all tools for all existing platforms

# Update specific tools only
mise lock node python                    # Updates only node and python for existing platforms

# Update specific tools for specific platforms  
mise lock --platforms linux-x64 node    # Updates only node for only linux-x64

# Force refresh (re-fetch metadata)
mise lock --force node python           # Re-fetch node/python even if data exists

# Check lockfile validity  
mise lock --check                        # Validate lockfile completeness and consistency

# Dry run to preview changes
mise lock --dry-run --platforms macos-arm64,linux-x64

# Environment-specific lockfiles
MISE_ENV=dev mise lock                   # Creates/updates mise.dev.lock + mise.lock
MISE_ENV=dev,test mise lock             # Creates/updates 4 lockfiles

# Install from lockfile only (frozen install)
mise install --frozen                   # Uses mise.lock
MISE_ENV=dev mise install --frozen      # Uses mise.dev.lock  
MISE_ENV=dev,test mise install --frozen # Uses mise.dev+test.lock
```

#### Update Behavior Examples
```bash
# Scenario 1: No lockfile exists
$ mise lock
# → Creates mise.lock with all tools from config for current platform

# Scenario 2: Lockfile exists with node@20.10.0 for macos-arm64
$ mise lock  
# → Updates node@20.10.0 for macos-arm64 (preserves existing)

# Scenario 3: Same lockfile, add new platform
$ mise lock --platforms linux-x64
# → Updates node@20.10.0 for linux-x64 (adds new platform)

# Scenario 4: Add new tool to existing lockfile
$ mise lock python  
# → Adds python for macos-arm64 (existing platform)

# Scenario 5: Config changes (node 20.10.0 → 21.0.0)
$ mise lock
# → Updates to node@21.0.0 for macos-arm64 (follows config)
```

#### Frozen Install Behavior
```bash
# Install only tools/versions from lockfile
mise install --frozen

# Error cases:
# 1. No lockfile exists -> Error: "No lockfile found, run 'mise lock' first"
# 2. Tool not in lockfile -> Error: "Tool 'node' not found in lockfile" 
# 3. Different version in config -> Error: "Version mismatch between config and lockfile"

# Environment-specific frozen install
MISE_ENV=dev mise install --frozen          # Uses mise.dev.lock
MISE_ENV=dev,test mise install --frozen     # Uses mise.dev+test.lock

# Validate lockfile vs current environment  
mise lock --check                                     # Check if lockfile is valid/complete
MISE_ENV=dev,test mise lock --check                   # Check environment-specific lockfiles
mise install --frozen --check-only                   # Validates base lockfile for installation
MISE_ENV=dev,test mise install --frozen --check-only # Validates combined lockfile for installation
```

### Data Model

#### Schema Migration: Single to Multi-Version

**Current Mixed Schema (to be migrated):**
```toml
# Single-version format (will be deprecated)
[tools.node]
version = "20.10.0"
backend = "core:node"

[tools.node.platforms.macos-arm64]
url = "https://nodejs.org/dist/v20.10.0/node-v20.10.0-darwin-arm64.tar.gz"
checksum = "sha256:abc123..."
size = 12345678

# Multi-version format (preferred)
[[tools.python]]
version = "3.11.0"
backend = "core:python"
```

**Target Unified Schema (array format only):**
```toml
# All tools use array format for consistency
[[tools.node]]
version = "20.10.0"
backend = "core:node"

[[tools.node.platforms.macos-arm64]]
url = "https://nodejs.org/dist/v20.10.0/node-v20.10.0-darwin-arm64.tar.gz"
checksum = "sha256:abc123..."
size = 12345678

[[tools.python]]
version = "3.11.0"
backend = "core:python"
```

**Migration Strategy:**
1. **Auto-migration**: Automatically detect and migrate single-version format when reading lockfiles
2. **Transparent upgrade**: No user action required, happens during any lockfile read operation
3. **Preserve all data**: Migration maintains all existing platform information
4. **Backward compatibility**: Support reading both formats during transition period
5. **Write new format**: Always write unified array format for new entries

#### MISE_ENV Integration

**Multi-Environment Support:**
MISE_ENV can specify multiple environments (e.g., `MISE_ENV=dev,test`), so lockfile generation must handle each environment individually plus the base configuration.

**Environment-Specific Lockfiles:**
```bash
# Individual lockfiles for each environment (no combined lockfiles)
config_root/
├── mise.lock         # Base lockfile (no MISE_ENV)
├── mise.dev.lock     # Development-specific tools only
├── mise.test.lock    # Test-specific tools only  
├── mise.prod.lock    # Production-specific tools only
└── mise.staging.lock # Staging-specific tools only
```

**Lockfile Generation Strategy:**
```rust
/// Generate lockfiles for current MISE_ENV - each environment gets its own lockfile
fn resolve_lockfiles_to_generate(mise_env: Option<&str>) -> Vec<PathBuf> {
    let config_root = Config::get().config_root();
    let mut paths = vec![];
    
    if let Some(env_string) = mise_env {
        let environments: Vec<&str> = env_string.split(',').map(str::trim).collect();
        
        // Generate individual lockfile for each active environment
        for env in environments {
            paths.push(config_root.join(format!("mise.{}.lock", env)));
        }
    }
    
    // Always generate base lockfile
    paths.push(config_root.join("mise.lock"));
    paths
}

/// Read and merge lockfiles for current MISE_ENV to build effective lockfile
fn resolve_effective_lockfile(mise_env: Option<&str>) -> Result<Lockfile> {
    let config_root = Config::get().config_root();
    let mut effective_lockfile = Lockfile::new();
    
    // Start with base lockfile
    let base_path = config_root.join("mise.lock");
    if base_path.exists() {
        let base_lockfile = Lockfile::read(&base_path)?;
        effective_lockfile.merge(base_lockfile);
    }
    
    // Layer environment-specific lockfiles on top
    if let Some(env_string) = mise_env {
        let environments: Vec<&str> = env_string.split(',').map(str::trim).collect();
        
        for env in environments {
            let env_path = config_root.join(format!("mise.{}.lock", env));
            if env_path.exists() {
                let env_lockfile = Lockfile::read(&env_path)?;
                effective_lockfile.merge(env_lockfile);  // Environment overrides base
            }
        }
    }
    
    Ok(effective_lockfile)
}
```

**Lockfile Generation Examples:**
```bash
# Single environment - generates 2 individual lockfiles
MISE_ENV=dev mise lock
# Creates:
# - mise.dev.lock (dev-specific tools only)
# - mise.lock (base tools only)

# Multiple environments - generates 3 individual lockfiles  
MISE_ENV=dev,test mise lock
# Creates:
# - mise.dev.lock (dev-specific tools only)
# - mise.test.lock (test-specific tools only)  
# - mise.lock (base tools only)
# No combined lockfile - resolved in memory when needed
```

**Environment-Specific Tool Resolution:**
```toml
# mise.toml (base config)
[tools]
node = "20.10.0"
python = "3.11.0"

[env.dev.tools]  
node = "21.0.0"        # Override to latest for dev
nodemon = "3.0.0"      # Dev-only tool

[env.test.tools]
pytest = "7.0.0"       # Test-only tool

[env.prod.tools]
node = "20.10.0"       # Explicit LTS for production
```

**Generated Individual Lockfiles:**
```toml
# mise.lock (base tools only)
[[tools.node]]
version = "20.10.0"
backend = "core:node"

[[tools.python]]
version = "3.11.0"
backend = "core:python"

# mise.dev.lock (dev-specific tools/overrides only)
[[tools.node]]
version = "21.0.0"     # Override of base node version
backend = "core:node"

[[tools.nodemon]]     # Dev-only tool
version = "3.0.0"
backend = "npm:nodemon"

# mise.test.lock (test-specific tools only)  
[[tools.pytest]]      # Test-only tool
version = "7.0.0"
backend = "pipx:pytest"
```

**Effective Lockfile Resolution:**
When `MISE_ENV=dev,test`, the system builds an effective lockfile in memory:
```toml
# Effective lockfile (in memory, not saved)
# Built by merging: mise.lock + mise.dev.lock + mise.test.lock

[[tools.node]]        # From mise.dev.lock (overrides base)
version = "21.0.0"     
backend = "core:node"

[[tools.python]]      # From mise.lock (no override)
version = "3.11.0"
backend = "core:python"

[[tools.nodemon]]     # From mise.dev.lock
version = "3.0.0"
backend = "npm:nodemon"

[[tools.pytest]]      # From mise.test.lock
version = "7.0.0"
backend = "pipx:pytest"
```

**Command Behavior:**
```bash
# Generate lockfiles for single environment + base
MISE_ENV=dev mise lock
# → Creates/updates mise.dev.lock and mise.lock

# Generate lockfiles for multiple environments (individual only)
MISE_ENV=dev,test mise lock  
# → Creates/updates:
#   - mise.dev.lock (dev-specific tools only)
#   - mise.test.lock (test-specific tools only)
#   - mise.lock (base tools only)

# Install from effective lockfile (merged in memory)
MISE_ENV=dev mise install --frozen          # Uses effective lockfile: base + dev
MISE_ENV=dev,test mise install --frozen     # Uses effective lockfile: base + dev + test

# Default behavior uses base lockfile
mise install --frozen                       # Uses mise.lock
```

#### Toolset Integration Strategy

**Lockfile and Toolset Interaction:**
The lockfile system needs to integrate with mise's existing `Toolset` for consistent tool resolution and version management.

```rust
// Extend Toolset to work with lockfiles
impl Toolset {
    /// Create toolset from effective lockfile for current MISE_ENV
    pub async fn from_lockfile(config: &Config) -> Result<Self> {
        let effective_lockfile = resolve_effective_lockfile(env::MISE_ENV.as_deref())?;
        let mut toolset = Self::new();
        
        // Convert lockfile tools to ToolVersionRequest
        for (tool_name, tool_entries) in effective_lockfile.tools {
            for entry in tool_entries {
                let tool_request = ToolRequest::new(
                    BackendArg::new(&tool_name, &entry.backend)?,
                    &entry.version
                )?;
                
                // Create ToolVersion with lockfile platform data
                let mut tool_version = ToolVersion::new(
                    &tool_request,
                    ToolVersionType::Version(entry.version),
                    config.project_root.clone()
                );
                
                // Populate platform info from lockfile
                tool_version.lock_platforms = entry.platforms;
                
                toolset.add_version(tool_version);
            }
        }
        
        Ok(toolset)
    }
    
    /// Validate current toolset against effective lockfile
    pub fn validate_against_config(&self, config_toolset: &Toolset) -> Result<Vec<LockfileValidationError>> {
        let mut errors = Vec::new();
        
        // Check each tool in config toolset against lockfile toolset
        for (tool_name, config_tool_version) in &config_toolset.versions {
            if let Some(lockfile_tool_versions) = self.versions.get(tool_name) {
                let config_version = &config_tool_version.version;
                
                // Check if config version matches any lockfile entry
                if !lockfile_tool_versions.iter().any(|entry| entry.version == *config_version) {
                    errors.push(LockfileValidationError::VersionMismatch {
                        tool: tool_name.clone(),
                        config_version: config_version.clone(),
                        lockfile_versions: lockfile_tool_versions.iter().map(|e| e.version.clone()).collect(),
                    });
                }
            } else {
                errors.push(LockfileValidationError::ToolNotInLockfile {
                    tool: tool_name.clone(),
                    version: config_tool_version.version.clone(),
                });
            }
        }
        
        Ok(errors)
    }

    pub fn validate_against_lockfile(&self, config: &Config) -> Result<Vec<LockfileValidationError>> {
        let effective_lockfile = resolve_effective_lockfile(env::MISE_ENV.as_deref())?;
        let mut errors = vec![];
        
        // Check each tool in current toolset
        for tool_version in &self.versions {
            let tool_name = &tool_version.request.backend.short;
            
            if let Some(lockfile_entries) = effective_lockfile.tools.get(tool_name) {
                let current_version = &tool_version.version;
                
                // Check if current version matches any lockfile entry
                if !lockfile_entries.iter().any(|entry| entry.version == *current_version) {
                    errors.push(LockfileValidationError::VersionMismatch {
                        tool: tool_name.clone(),
                        config_version: current_version.clone(),
                        lockfile_versions: lockfile_entries.iter().map(|e| e.version.clone()).collect(),
                    });
                }
            } else {
                errors.push(LockfileValidationError::ToolNotInLockfile {
                    tool: tool_name.clone(),
                    version: tool_version.version.clone(),
                });
            }
        }
        
        Ok(errors)
    }
}

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockfileValidationError {
    #[error("Version mismatch for {tool}: config specifies {config_version}, lockfile has {}", lockfile_versions.join(", "))]
    VersionMismatch {
        tool: String,
        config_version: String, 
        lockfile_versions: Vec<String>,
    },
    #[error("Tool not in lockfile: {tool}@{version}")]
    ToolNotInLockfile {
        tool: String,
        version: String,
    },
}
```

**Frozen Install Integration:**
```rust
use crate::ui::progress_report::{ProgressIcon, ProgressReport, SingleReport};

// In mise install --frozen implementation
pub async fn install_frozen(config: &Config) -> Result<()> {
    let pr = ProgressReport::new("install frozen".to_string());
    
    // Create toolset from effective lockfile
    let lockfile_toolset = Toolset::from_lockfile(config).await?;
    
    // Validate that lockfile exists and has tools
    if lockfile_toolset.versions.is_empty() {
        return Err(eyre!("No lockfile found or lockfile is empty. Run 'mise lock' first."));
    }
    
    // Validate lockfile against current config - fail fast on any mismatches
    let current_toolset = config.get_toolset().await?;
    let validation_errors = lockfile_toolset.validate_against_config(&current_toolset)?;
    if !validation_errors.is_empty() {
        for error in &validation_errors {
            pr.finish_with_icon(format!("{}", error), ProgressIcon::Error);
        }
        return Err(eyre!("Lockfile validation failed. Run 'mise lock' to update lockfile."));
    }
    
    // Install only tools from lockfile (ignore config tools)
    let mut install_results = vec![];
    for tool_version in lockfile_toolset.versions {
        // Use existing platform info from lockfile for verification
        let result = tool_version.install(InstallContext::new()).await;
        install_results.push((tool_version.request.backend.short.clone(), result));
    }
    
    // Report results
    for (tool_name, result) in install_results {
        match result {
            Ok(_) => pr.finish_with_icon(format!("Installed {tool_name} from lockfile"), ProgressIcon::Success),
            Err(e) => pr.finish_with_icon(format!("Failed to install {tool_name}: {e}"), ProgressIcon::Error),
        }
    }
    
    Ok(())
}
```

**Lockfile Merge Logic:**
```rust
impl Lockfile {
    /// Merge another lockfile into this one (environment overrides base)
    pub fn merge(&mut self, other: Lockfile) {
        for (tool_name, other_entries) in other.tools {
            let tool_entries = self.tools.entry(tool_name).or_insert_with(Vec::new);
            
            for other_entry in other_entries {
                // Replace existing entry with same version, or add new
                if let Some(existing_index) = tool_entries
                    .iter()
                    .position(|e| e.version == other_entry.version) 
                {
                    // Environment overrides base for same version
                    tool_entries[existing_index] = other_entry;
                } else {
                    // Add new version
                    tool_entries.push(other_entry);
                }
            }
            
            // Sort by version for consistent output
            tool_entries.sort_by(|a, b| a.version.cmp(&b.version));
        }
    }
}
```

### Core Components

#### 1. Platform Target Representation

**New Type: `PlatformTarget`**
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlatformTarget {
    pub os: String,        // "macos", "linux", "windows"
    pub arch: String,      // "arm64", "x64", "x86"  
    pub qualifier: Option<String>, // "gnu", "musl", etc.
}

impl PlatformTarget {
    pub fn parse(s: &str) -> Result<Self>;
    pub fn canonical_string(&self) -> String;
    pub fn matches_current() -> bool;
}
```

**Platform Parsing Rules:**
- `macos-arm64` → `PlatformTarget { os: "macos", arch: "arm64", qualifier: None }`
- `linux-x64-gnu` → `PlatformTarget { os: "linux", arch: "x64", qualifier: Some("gnu") }`
- Normalize: `darwin` → `macos`, `amd64` → `x64`

#### 2. Backend Trait Extension

**Extended Backend Trait:**
```rust
#[async_trait]
#[derive(Debug, Clone)]
pub struct InstallContext {
    pub config: Arc<Config>,
    pub pr: Box<dyn SingleReport>,
    pub ts: Toolset,
    pub lockfile_generation: bool,  // Replaces MISE_LOCKFILE_GENERATION
    pub target_platform: Option<PlatformTarget>,  // Platform target for lockfile generation
}

impl InstallContext {
    pub fn new() -> Self {
        Self {
            config: Config::get(),
            pr: ProgressReport::new("install".to_string()),
            ts: Toolset::default(),
            lockfile_generation: false,
            target_platform: None,
        }
    }

    pub fn for_lockfile_generation(target: PlatformTarget) -> Self {
        Self {
            config: Config::get(),
            pr: ProgressReport::new("lockfile generation".to_string()),
            ts: Toolset::default(),
            lockfile_generation: true,
            target_platform: Some(target),
        }
    }
    
    /// Get the platform target for this context (current platform if not specified)
    pub fn platform_target(&self) -> Result<PlatformTarget> {
        match &self.target_platform {
            Some(target) => Ok(target.clone()),
            None => PlatformTarget::current(),
        }
    }
}


#[derive(Debug, Clone)]
pub struct GitHubReleaseInfo {
    pub repo: String,              // e.g., "microsoft/vscode" 
    pub asset_pattern: Option<String>, // e.g., "*linux-x64.tar.gz" or None for auto-detection
    pub api_url: Option<String>,   // Custom API URL for GitLab/Enterprise, None for github.com
    pub release_type: ReleaseType, // GitHub or GitLab
}

#[derive(Debug, Clone)]
pub enum ReleaseType {
    GitHub,
    GitLab,
}

pub trait Backend {
    // Existing methods preserved...
    
    /// Optional: Provide tarball URL for platform-specific tool installation
    /// Backends can implement this for simple tarball-based tools
    async fn get_tarball_url(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<Option<String>> {
        Ok(None) // Default: no tarball URL available
    }
    
    /// Optional: Provide GitHub/GitLab release info for platform-specific tool installation
    /// Backends can implement this for GitHub/GitLab release-based tools
    async fn get_github_release_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<Option<GitHubReleaseInfo>> {
        Ok(None) // Default: no GitHub release info available
    }
    
    /// Resolve platform-specific lock information without installation
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Try simple tarball approach first
        if let Some(tarball_url) = self.get_tarball_url(tv, target).await? {
            return self.resolve_lock_info_from_tarball(&tarball_url, tv, target).await;
        }
        
        // Try GitHub/GitLab release approach second
        if let Some(release_info) = self.get_github_release_info(tv, target).await? {
            return self.resolve_lock_info_from_github_release(release_info, tv, target).await;
        }
        
        // Fall back to temporary installation approach
        self.resolve_lock_info_fallback(tv, target).await
    }

    /// Shared logic for processing tarball-based tools
    /// Downloads tarball, extracts size from actual download, and populates PlatformInfo
    async fn resolve_lock_info_from_tarball(
        &self,
        tarball_url: &str,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        use crate::http::HTTP;
        use std::io::Write;
        use tempfile::NamedTempFile;
        
        // Download tarball to temporary location to get actual size
        let mut temp_file = NamedTempFile::new()?;
        let mut downloaded_size = 0u64;
        
        // Stream download to count bytes without storing entire file in memory
        let response = HTTP.get(tarball_url).send().await?;
        let mut stream = response.bytes_stream();
        
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded_size += chunk.len() as u64;
            temp_file.write_all(&chunk)?;
        }
        
        // Extract checksum from downloaded content
        temp_file.flush()?;
        let checksum = self.calculate_checksum(temp_file.path()).await?;
        
        // Get verification information if available
        let verification_infos = self.get_verification_info(tv, target, tarball_url).await?;
        
        Ok(PlatformInfo {
            url: Some(tarball_url.to_string()),
            size: Some(downloaded_size),
            checksum: Some(checksum),
            verification_infos,
        })
    }
    
    /// Calculate checksum of downloaded file
    async fn calculate_checksum(&self, file_path: &std::path::Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        use tokio::fs::File;
        use tokio::io::AsyncReadExt;
        
        let mut file = File::open(file_path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];
        
        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }
    
    /// Shared logic for processing GitHub/GitLab release-based tools
    /// Leverages existing UnifiedGitBackend logic for asset resolution
    async fn resolve_lock_info_from_github_release(
        &self,
        release_info: GitHubReleaseInfo,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        use crate::backend::github::UnifiedGitBackend;
        use crate::cli::args::BackendArg;
        
        // Create a temporary UnifiedGitBackend to leverage existing asset resolution logic
        let backend_type = match release_info.release_type {
            ReleaseType::GitHub => crate::backend::backend_type::BackendType::Github,
            ReleaseType::GitLab => crate::backend::backend_type::BackendType::Gitlab,
        };
        
        let mut backend_arg = BackendArg::new(
            backend_type.to_string(),
            Some(format!("{}:{}", backend_type.as_ref().to_lowercase(), release_info.repo))
        );
        
        // Add asset pattern and API URL to backend options if provided
        if let Some(pattern) = release_info.asset_pattern {
            backend_arg.with_option("asset_pattern", pattern);
        }
        if let Some(api_url) = release_info.api_url {
            backend_arg.with_option("api_url", api_url);
        }
        
        let unified_backend = UnifiedGitBackend::from_arg(backend_arg);
        
        // Use existing resolve_asset_url logic to get the asset URL
        let opts = tv.request.options();
        let api_url = unified_backend.get_api_url(&opts);
        let repo = &release_info.repo;
        
        let asset_url = match release_info.release_type {
            ReleaseType::GitHub => {
                unified_backend.resolve_github_asset_url(tv, &opts, repo, &api_url, &tv.version).await?
            },
            ReleaseType::GitLab => {
                unified_backend.resolve_gitlab_asset_url(tv, &opts, repo, &api_url, &tv.version).await?
            },
        };
        
        // Use the tarball logic to download and extract metadata
        self.resolve_lock_info_from_tarball(&asset_url, tv, target).await
    }
    
    /// Get verification information for the tool if available
    async fn get_verification_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
        asset_url: &str,
    ) -> Result<Vec<VerificationInfo>> {
        // Default implementation returns no verification
        // Backends can override this to provide GPG, Cosign, etc.
        Ok(Vec::new())
    }
    
    /// Fallback strategy: temporary installation approach
    async fn resolve_lock_info_fallback(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Create temporary directory for installation
        let temp_dir = tempfile::tempdir()?;
        let mut temp_tv = tv.clone();
        temp_tv.install_path = temp_dir.path().to_path_buf();
        
        // Set platform environment variable for backends that need it
        let original_platform = env::var("MISE_PLATFORM").ok();
        env::set_var("MISE_PLATFORM", target.canonical_string());
        
        let result = async {
            // Use existing install_version but in temp location with lockfile generation context
            let config = Config::get().await?;
            let ctx = InstallContext::for_lockfile_generation(config, Toolset::default());
            self.install_version(&ctx, temp_tv).await?;
            
            // Extract platform info from temp installation artifacts
            let platform_key = target.canonical_string();
            if let Some(info) = temp_tv.lock_platforms.get(&platform_key) {
                Ok(info.clone())
            } else {
                Ok(PlatformInfo::default())
            }
        }.await;
        
        // Restore original environment variable
        match original_platform {
            Some(platform) => env::set_var("MISE_PLATFORM", platform),
            None => env::remove_var("MISE_PLATFORM"),
        }
        
        result
    }
    
    /// Optional: Return known supported platforms for discovery
    fn supported_platform_aliases(&self) -> Option<Vec<String>> {
        None
    }
}
```

**PlatformInfo Structure (Existing):**
```rust
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub url: Option<String>,
    pub checksum: Option<String>, 
    pub size: Option<u64>,
}
```

#### 3. Backend Resolution Methods

**Resolution is handled directly by Backend trait methods:**
- `get_tarball_url()` - For simple URL-based tools (Node.js, Python, etc.)
- `get_github_release_info()` - For GitHub/GitLab release-based tools (Bun, etc.)
- `resolve_lock_info_from_tarball()` - Shared download and checksum logic
- `resolve_lock_info_from_github_release()` - Shared GitHub release logic

This eliminates the need for separate resolver abstractions - the backend trait provides all necessary functionality using existing `HTTP` client and helper methods from `static_helpers.rs`.

#### 4. Command Implementation Flow

**Current Implementation (`src/cli/lock.rs`):**
```rust
pub async fn run(self) -> Result<()> {
    let settings = Settings::get();
    let config = Config::get().await?;
    settings.ensure_experimental("lock")?;

    // Phase 1: Analysis and discovery (currently implemented)
    self.analyze_lockfiles(&config).await?;

    if !self.dry_run {
        miseprintln!(
            "{} {}",
            style("mise lock").bold().cyan(),
            style("full implementation coming in next phase").yellow()
        );
    }

    Ok(())
}
```

**Planned Full Implementation:**
```rust
use crate::ui::progress_report::{ProgressIcon, ProgressReport, SingleReport};

pub async fn run(self) -> Result<()> {
    let settings = Settings::get();
    let config = Config::get().await?;
    settings.ensure_experimental("lock")?;

    // 1. Resolve all lockfile paths based on MISE_ENV
    let lockfile_paths = self.resolve_lockfile_paths(env::MISE_ENV.as_deref());
    
    // 2. Generate lockfiles for each environment context
    for lockfile_path in lockfile_paths {
        let pr = ProgressReport::new("lock update".to_string());
        let env_context = self.extract_env_from_path(&lockfile_path);
        let existing_lockfile = if lockfile_path.exists() {
            Some(Lockfile::read(&lockfile_path)?)
        } else {
            None
        };
        
        // 3. Resolve tools for this specific environment context
        let target_tools = self.resolve_target_tools_for_env(&config, env_context).await?;
        let target_platforms = self.get_target_platforms_resolved(&existing_lockfile)?;
        
        // 4. Fetch platform info concurrently
        let platform_infos = self.fetch_platform_infos(&target_tools, &target_platforms).await?;
        
        // 5. Update lockfile for this environment
        let mut lockfile = existing_lockfile.unwrap_or_default();
        self.update_lockfile(&mut lockfile, platform_infos)?;
        
        // 6. Save or dry-run
        if self.dry_run {
            self.print_changes_for_env(&lockfile, env_context)?;
        } else {
            lockfile.save(&lockfile_path)?;
            pr.finish_with_icon(format!("Updated {}", lockfile_path.display()), ProgressIcon::Success);
        }
    }
    
    Ok(())
}

fn resolve_lockfile_paths(&self, mise_env: Option<&str>) -> Vec<PathBuf> {
    let config_root = Config::get().config_root();
    let mut paths = vec![];
    
    if let Some(env_string) = mise_env {
        let environments: Vec<&str> = env_string.split(',').map(str::trim).collect();
        
        if environments.len() == 1 {
            // Single environment: generate env-specific + base
            paths.push(config_root.join(format!("mise.{}.lock", environments[0])));
        } else if environments.len() > 1 {
            // Multiple environments: generate combined + individual + base
            let combined_env = environments.join("+");
            paths.push(config_root.join(format!("mise.{}.lock", combined_env)));
            
            // Individual environment lockfiles
            for env in environments {
                paths.push(config_root.join(format!("mise.{}.lock", env)));
            }
        }
    }
    
    // Always include base lockfile (no environment)
    paths.push(config_root.join("mise.lock"));
    paths
}

async fn resolve_target_tools_for_env(
    &self, 
    config: &Config, 
    env_context: Option<&str>
) -> Result<Vec<(String, ToolVersion)>> {
    // Resolve tools considering environment-specific overrides
    let toolset = if let Some(env) = env_context {
        // Parse environment list (e.g., "dev+test" -> ["dev", "test"])
        let envs: Vec<&str> = env.split('+').collect();
        config.get_toolset_for_environments(&envs).await?
    } else {
        // Base toolset (no environment)
        config.get_toolset().await?
    };
    
    // Convert to tool versions for lockfile generation
    self.toolset_to_tool_versions(toolset)
}

/// Validate lockfile completeness and consistency
async fn check_lockfile(&self) -> Result<()> {
    use crate::ui::progress_report::{ProgressIcon, ProgressReport, SingleReport};
    let lockfile_paths = self.resolve_lockfiles_to_generate(env::MISE_ENV.as_deref());
    let config = Config::get().await?;
    let mut all_valid = true;
    let mut total_issues = 0;

    for lockfile_path in lockfile_paths {
        let env_context = self.extract_env_from_path(&lockfile_path);
        let pr = ProgressReport::new(format!("lock check"));
        pr.set_message(format!("Checking {}...", lockfile_path.display()));
        
        // Check if lockfile exists
        if !lockfile_path.exists() {
            pr.finish_with_icon("Lockfile does not exist".to_string(), ProgressIcon::Error);
            all_valid = false;
            total_issues += 1;
            continue;
        }
        
        // Load and validate lockfile
        let lockfile = match Lockfile::read(&lockfile_path) {
            Ok(lf) => lf,
            Err(e) => {
                pr.finish_with_icon(format!("Failed to parse lockfile: {}", e), ProgressIcon::Error);
                all_valid = false;
                total_issues += 1;
                continue;
            }
        };
        
        // Check 1: Compare with current config tools
        let config_issues = self.check_lockfile_vs_config(&lockfile, &config, env_context).await?;
        total_issues += config_issues;
        if config_issues > 0 {
            all_valid = false;
        }
        
        // Check 2: Validate platform data completeness
        let platform_issues = self.check_platform_completeness(&lockfile).await?;
        total_issues += platform_issues;
        if platform_issues > 0 {
            all_valid = false;
        }
        
        // Check 3: Validate tool metadata integrity
        let metadata_issues = self.check_metadata_integrity(&lockfile).await?;
        total_issues += metadata_issues;
        if metadata_issues > 0 {
            all_valid = false;
        }
        
        if config_issues + platform_issues + metadata_issues == 0 {
            pr.finish_with_icon("Lockfile is valid and complete".to_string(), ProgressIcon::Success);
        }
    }
    
    // Summary
    if all_valid {
        miseprintln!("✓ All lockfiles are valid and complete");
        Ok(())
    } else {
        miseprintln!("✗ Found {} issue(s) across lockfiles", total_issues);
        std::process::exit(1);
    }
}

/// Check if lockfile matches current config for the environment
async fn check_lockfile_vs_config(
    &self,
    lockfile: &Lockfile, 
    config: &Config,
    env_context: Option<&str>
) -> Result<usize> {
    let mut issues = 0;
    
    // Get tools from config for this environment context
    let config_tools = self.resolve_target_tools_for_env(config, env_context).await?;
    let config_tool_map: std::collections::HashMap<String, String> = config_tools
        .iter()
        .map(|(name, tv)| (name.clone(), tv.version.clone()))
        .collect();
    
    // Check for tools in config but missing from lockfile
    for (tool_name, config_version) in &config_tool_map {
        if let Some(lockfile_entries) = lockfile.tools.get(tool_name) {
            // Check if config version exists in lockfile
            if !lockfile_entries.iter().any(|entry| entry.version == *config_version) {
                error!("  ✗ Tool {}: config has version {} but lockfile has {:?}", 
                    tool_name, 
                    config_version,
                    lockfile_entries.iter().map(|e| &e.version).collect::<Vec<_>>()
                );
                issues += 1;
            }
        } else {
            error!("  ✗ Tool {} (version {}) from config is missing from lockfile", tool_name, config_version);
            issues += 1;
        }
    }
    
    // Check for tools in lockfile but not in config (warning only)
    for (tool_name, lockfile_entries) in &lockfile.tools {
        if !config_tool_map.contains_key(tool_name) {
            let versions: Vec<&String> = lockfile_entries.iter().map(|e| &e.version).collect();
            warn!("  ⚠ Tool {} ({:?}) in lockfile but not in current config", tool_name, versions);
        }
    }
    
    Ok(issues)
}

/// Check if all tools have complete platform data
async fn check_platform_completeness(&self, lockfile: &Lockfile) -> Result<usize> {
    let mut issues = 0;
    
    for (tool_name, tool_entries) in &lockfile.tools {
        for entry in tool_entries {
            if entry.platforms.is_empty() {
                error!("  ✗ Tool {}@{} has no platform data", tool_name, entry.version);
                issues += 1;
                continue;
            }
            
            // Check each platform has required data
            for (platform, platform_info) in &entry.platforms {
                let mut missing = Vec::new();
                
                if platform_info.url.is_none() {
                    missing.push("url");
                }
                if platform_info.checksum.is_none() {
                    missing.push("checksum");
                }
                // Size is optional but useful
                if platform_info.size.is_none() {
                    missing.push("size");
                }
                
                if !missing.is_empty() {
                    if missing.contains(&"url") || missing.contains(&"checksum") {
                        // URL/checksum missing is an error
                        error!("  ✗ Tool {}@{} platform {}: missing {}", 
                            tool_name, entry.version, platform, missing.join(", "));
                        issues += 1;
                    } else {
                        // Only size missing is a warning
                        warn!("  ⚠ Tool {}@{} platform {}: missing {}", 
                            tool_name, entry.version, platform, missing.join(", "));
                    }
                }
            }
        }
    }
    
    Ok(issues)
}

/// Check metadata integrity (URLs accessible, checksums valid format, etc.)
async fn check_metadata_integrity(&self, lockfile: &Lockfile) -> Result<usize> {
    let mut issues = 0;
    
    for (tool_name, tool_entries) in &lockfile.tools {
        for entry in tool_entries {
            for (platform, platform_info) in &entry.platforms {
                // Check URL format
                if let Some(url) = &platform_info.url {
                    if !url.starts_with("http://") && !url.starts_with("https://") {
                        error!("  ✗ Tool {}@{} platform {}: invalid URL format: {}", 
                            tool_name, entry.version, platform, url);
                        issues += 1;
                    }
                }
                
                // Check checksum format  
                if let Some(checksum) = &platform_info.checksum {
                    if !checksum.contains(':') {
                        error!("  ✗ Tool {}@{} platform {}: invalid checksum format (should be 'algo:hash'): {}", 
                            tool_name, entry.version, platform, checksum);
                        issues += 1;
                    } else {
                        let parts: Vec<&str> = checksum.splitn(2, ':').collect();
                        let algo = parts[0];
                        if !["md5", "sha1", "sha256", "sha512", "blake3"].contains(&algo) {
                            warn!("  ⚠ Tool {}@{} platform {}: unknown checksum algorithm: {}", 
                                tool_name, entry.version, platform, algo);
                        }
                    }
                }
            }
        }
    }
    
    Ok(issues)
}

/// Core lockfile update logic - merges new platform info with existing lockfile
fn update_lockfile_with_platform_info(
    lockfile: &mut Lockfile,
    target_tools: Vec<(String, ToolVersion)>,
    target_platforms: Vec<PlatformTarget>,
) -> Result<()> {
    for (tool_name, mut tool_version) in target_tools {
        // Find or create tool entry in lockfile
        let tool_entries = lockfile.tools.entry(tool_name.clone()).or_insert_with(Vec::new);
        
        // Find existing entry for this version, or create new one
        let mut tool_entry = tool_entries
            .iter_mut()
            .find(|entry| entry.version == tool_version.version)
            .cloned();
        
        if tool_entry.is_none() {
            // Create new tool entry
            tool_entry = Some(LockfileToolEntry {
                version: tool_version.version.clone(),
                backend: tool_version.backend.to_string(),
                platforms: HashMap::new(),
            });
        }
        
        let mut entry = tool_entry.unwrap();
        
        // Update platform info for each target platform
        for platform_target in &target_platforms {
            let platform_key = platform_target.canonical_string();
            
            // Skip if platform data already exists and --force not specified
            if !self.force && entry.platforms.contains_key(&platform_key) {
                continue;
            }
            
            // Resolve platform info for this tool/platform combination
            let platform_info = tool_version.backend
                .resolve_lock_info(&tool_version, platform_target)
                .await?;
            
            // Only update if we got valid info
            if platform_info.url.is_some() || platform_info.checksum.is_some() || platform_info.size.is_some() {
                entry.platforms.insert(platform_key, platform_info);
            }
        }
        
        // Update or add the tool entry back to lockfile
        if let Some(existing_index) = tool_entries
            .iter()
            .position(|e| e.version == entry.version) 
        {
            // Update existing entry
            tool_entries[existing_index] = entry;
        } else {
            // Add new entry
            tool_entries.push(entry);
        }
        
        // Sort entries by version for consistent output
        tool_entries.sort_by(|a, b| a.version.cmp(&b.version));
    }
    
    Ok(())
}
```

**Update Logic Flow:**

1. **Tool Entry Management**:
   ```rust
   // For each target tool (e.g., "node@20.10.0")
   for (tool_name, tool_version) in target_tools {
       // Find existing [[tools.node]] entries
       let tool_entries = lockfile.tools.entry("node").or_insert_with(Vec::new);
       
       // Find entry with matching version "20.10.0" or create new
       let tool_entry = find_or_create_version_entry(tool_entries, "20.10.0");
   }
   ```

2. **Platform Info Resolution**:
   ```rust
   // For each target platform (e.g., "macos-arm64", "linux-x64")
   for platform_target in target_platforms {
       // Skip if data exists and --force not used
       if !force && entry.platforms.contains_key("macos-arm64") { continue; }
       
       // Call backend to get platform-specific metadata
       let platform_info = backend.resolve_lock_info(&tool_version, &platform_target).await?;
       
       // Update lockfile entry
       entry.platforms.insert("macos-arm64", platform_info);
   }
   ```

3. **Merge Strategy**:
   ```toml
   # Before update: existing lockfile
   [[tools.node]]
   version = "20.10.0"
   backend = "core:node"
   
   [[tools.node.platforms.macos-arm64]]
   url = "https://nodejs.org/dist/v20.10.0/node-v20.10.0-darwin-arm64.tar.gz"
   checksum = "sha256:abc123..."
   
   # After update with --platforms linux-x64: adds new platform
   [[tools.node]]
   version = "20.10.0" 
   backend = "core:node"
   
   [[tools.node.platforms.macos-arm64]]  # Preserved existing
   url = "https://nodejs.org/dist/v20.10.0/node-v20.10.0-darwin-arm64.tar.gz"
   checksum = "sha256:abc123..."
   
   [[tools.node.platforms.linux-x64]]    # Added new platform
   url = "https://nodejs.org/dist/v20.10.0/node-v20.10.0-linux-x64.tar.gz"
   checksum = "sha256:def456..."
   ```

4. **Force Update Behavior**:
   ```rust
   if self.force || !entry.platforms.contains_key(&platform_key) {
       // Always fetch new platform info when --force is used
       // or when platform doesn't exist in lockfile
       let platform_info = backend.resolve_lock_info(&tool_version, platform_target).await?;
       entry.platforms.insert(platform_key, platform_info);
   }
   // Otherwise preserve existing platform data
   ```

5. **Error Handling**:
   ```rust
   match backend.resolve_lock_info(&tool_version, platform_target).await {
       Ok(platform_info) => {
           // Only update if we got useful info
           if platform_info.has_data() {
               entry.platforms.insert(platform_key, platform_info);
           }
       },
       Err(e) => {
           // Log warning but continue with other platforms
           warn!("Failed to resolve platform info for {tool_name}@{version} on {platform}: {e}");
       }
   }
   ```

**Concurrency Model:**
- Use `tokio::task::JoinSet` with `--jobs` limit
- Task granularity: `(tool, version, platform)` tuple
- Fail-fast on critical errors, warn and continue on platform-specific failures

### Backend Implementation Strategy

#### Immediate Targets (Phase 1-2)
**Backends already using `tv.lock_platforms`:**

1. **HTTP Backend** - Already implemented platform-aware lockfile generation
   - Uses `lookup_platform_key()` for platform-specific URLs
   - Generates checksums and size during download
   - Serves as reference implementation

2. **GitHub Backend** - Already using lockfile platforms
   - Asset pattern matching for platform-specific downloads
   - Checksum resolution from release assets

3. **UBI Backend** - Already integrated with lockfile system
   - GitHub releases with platform detection
   - Good candidate for multi-platform resolution

4. **Aqua Backend** - Has lockfile platform integration
   - Registry-based approach with platform mappings

#### Core Backends (Phase 2-3)
**Built-in backends with deterministic patterns:**

1. **Node.js** (`core:node`)
   - URL: `https://nodejs.org/dist/v{version}/node-v{version}-{os}-{arch}.tar.gz`
   - Checksum: `SHASUMS256.txt` manifest
   - Size: HTTP HEAD or manifest parsing

2. **Go** (`core:go`) 
   - URL: `https://go.dev/dl/go{version}.{os}-{arch}.tar.gz`
   - Checksum: JSON download API
   - Size: JSON download API

3. **Zig** (`core:zig`)
   - URL/Checksum/Size: JSON index at `https://ziglang.org/download/index.json`

4. **Deno** (`core:deno`)
   - URL/Checksum/Size: GitHub releases API + asset detection

#### Plugin System Extensions (Phase 3-4)
**ASDF Plugin Lockfile Export:**

ASDF plugins will support a new `bin/lockfile-info` script that returns platform-specific metadata without installation:

```bash
#!/usr/bin/env bash
# bin/lockfile-info
# Usage: bin/lockfile-info <version> <platform> <os> <arch> [extra]
# Example: bin/lockfile-info 20.10.0 macos-arm64 macos arm64
# Example: bin/lockfile-info 3.11.0 linux-x64-gnu linux x64 gnu
# Output: TOML format (no jq dependency required)

set -euo pipefail

version="$1"
platform="$2" 
os="$3"
arch="$4"
extra="${5:-}"

# Map mise platform to tool's platform naming
case "$platform" in
  macos-arm64)
    tool_os="darwin"
    tool_arch="arm64"
    url="https://releases.example.com/v${version}/tool-v${version}-${tool_os}-${tool_arch}.tar.gz"
    ;;
  linux-x64)
    tool_os="linux"
    tool_arch="amd64"
    url="https://releases.example.com/v${version}/tool-v${version}-${tool_os}-${tool_arch}.tar.gz"
    ;;
  linux-x64-gnu)
    tool_os="linux"
    tool_arch="amd64"
    # Use extra parameter for libc variant
    url="https://releases.example.com/v${version}/tool-v${version}-${tool_os}-${tool_arch}-gnu.tar.gz"
    ;;
  *)
    echo "Platform $platform not supported" >&2
    exit 1
    ;;
esac

# Fetch checksum and size
checksum=$(curl -s "https://releases.example.com/v${version}/checksums.txt" | grep "${tool_os}-${tool_arch}" | cut -d' ' -f1)
size=$(curl -sI "$url" | grep -i content-length | cut -d' ' -f2 | tr -d '\r')

# Output TOML format using heredoc (no jq dependency)
cat <<EOF
url = "${url}"
checksum = "sha256:${checksum}"
size = ${size}
EOF
```

**VFox Plugin Lockfile Export:**

VFox plugins will extend their existing hooks with a new `LockfileInfo` function that leverages VFox's existing OS/Arch system:

```lua
-- In plugin's main.lua
function PLUGIN:LockfileInfo(ctx)
    -- ctx.version: requested version (e.g., "20.10.0")  
    -- ctx.os: VFox OS enum (e.g., MACOS, LINUX, WINDOWS)
    -- ctx.arch: VFox Arch enum (e.g., ARM64, AMD64, X86)
    -- ctx.osType: string version (e.g., "darwin", "linux", "windows")
    -- ctx.archType: string version (e.g., "arm64", "amd64", "386")
    
    local version = ctx.version
    
    -- Use existing VFox OS/Arch mappings instead of parsing platform string
    local tool_os = ctx.osType
    local tool_arch = ctx.archType
    
    -- Map VFox arch names to tool naming conventions if needed
    if tool_arch == "amd64" then
        tool_arch = "x64"  -- Many tools use x64 instead of amd64
    end
    
    local url = string.format("https://releases.example.com/v%s/tool-v%s-%s-%s.tar.gz", 
                             version, version, tool_os, tool_arch)
    
    -- Fetch checksum from remote manifest
    local checksum_url = string.format("https://releases.example.com/v%s/checksums.txt", version)
    local checksum_data = http.get(checksum_url)
    local pattern = tool_os .. "%-" .. tool_arch .. "%s+(%w+)"
    local checksum = checksum_data:match(pattern)
    
    -- Get size via HEAD request  
    local size = http.head(url).headers["content-length"]
    
    return {
        url = url,
        checksum = checksum and ("sha256:" .. checksum) or nil,
        size = size and tonumber(size) or nil
    }
end

-- Alternative: Extend existing Available/PreInstall hooks for lockfile context
function PLUGIN:Available(ctx)
    if ctx.lockfileMode then
        -- Return metadata only during lockfile generation
        return self:LockfileInfo(ctx)
    end
    
    -- Normal available versions logic
    return self:getAvailableVersions()
end
```

**Extended VFox Plugin Capabilities:**

Beyond lockfile generation, VFox plugins can leverage checksum/size metadata for improved security and developer experience:

```lua
-- Enhanced security during normal installation
function PLUGIN:PreInstall(ctx)
    -- During normal installation (not lockfile generation)
    if not ctx.lockfileMode and ctx.expectedChecksum then
        -- Use lockfile checksum for verification during install
        local downloaded_file = ctx.downloadPath
        local actual_checksum = self:calculateChecksum(downloaded_file)
        
        if actual_checksum ~= ctx.expectedChecksum then
            error(string.format("Checksum mismatch for %s@%s: expected %s, got %s", 
                ctx.name, ctx.version, ctx.expectedChecksum, actual_checksum))
        end
        
        print("✓ Checksum verified from lockfile")
    end
end

-- Progress reporting using size information
function PLUGIN:Download(ctx) 
    if ctx.expectedSize then
        -- Show progress bar with known total size
        return self:downloadWithProgress(ctx.url, ctx.downloadPath, ctx.expectedSize)
    else {
        -- Fall back to indeterminate progress
        return self:downloadBasic(ctx.url, ctx.downloadPath)
    }
end

-- Integrity checking for development workflows
function PLUGIN:PostInstall(ctx)
    -- Validate installation integrity using lockfile metadata
    if ctx.lockfileMetadata then
        local installed_version = self:getInstalledVersion(ctx.installPath)
        if installed_version ~= ctx.version then
            error(string.format("Version mismatch after install: expected %s, got %s", 
                ctx.version, installed_version))
        end
        
        -- Optional: verify binary checksums match expected
        if ctx.lockfileMetadata.binaryChecksum then
            local binary_checksum = self:calculateBinaryChecksum(ctx.installPath)
            if binary_checksum ~= ctx.lockfileMetadata.binaryChecksum then
                print("Warning: Installed binary checksum doesn't match lockfile")
            end
        end
    end
end

function PLUGIN:Available(ctx)
    -- Standard version fetching logic
    return self:fetchVersionInfo(ctx)
end
```

**VFox Plugin Extensions for Developer Experience:**

1. **Smart Download Progress**: Use `expectedSize` from lockfile for accurate progress bars
2. **Integrity Verification**: Automatically verify checksums during installation when lockfile data available
3. **Network Optimization**: Skip downloads if checksums match during frozen installs
5. **Security Alerts**: Warn users when checksums don't match or are missing
6. **Install Validation**: Verify installed tools match lockfile expectations

**Integration with Mise Core:**
```rust
// Mise passes lockfile metadata to VFox plugins
let vfox_ctx = VFoxContext {
    version: tool_version.version.clone(),
    os: target_platform.os_enum(),
    arch: target_platform.arch_enum(),
    osType: target_platform.os.clone(),
    archType: target_platform.arch.clone(),
    expectedChecksum: tool_version.lock_platforms.get(&platform_key)
        .and_then(|p| p.checksum.clone()),
    expectedSize: tool_version.lock_platforms.get(&platform_key)
        .and_then(|p| p.size),
    lockfileMetadata: Some(lockfile_metadata),
};
```

#### Backend Optimization with InstallContext

**InstallContext.lockfile_generation Boolean:**
During lockfile generation, InstallContext has `lockfile_generation: true` to signal that backends can optimize for metadata-only operations.

**Core Backend Optimization Examples:**
```rust
// Node.js backend can skip compilation steps
impl Backend for NodeBackend {
    async fn install_version(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        if ctx.lockfile_generation {
            // During lockfile generation - minimal install for metadata only
            let url = self.get_download_url(tv)?;
            let checksum = self.get_checksum(tv).await?;
            let size = self.get_download_size(&url).await?;
            
            // Populate lockfile data without full extraction/compilation
            let platform_key = self.get_platform_key();
            tv.lock_platforms.insert(platform_key, PlatformInfo { 
                url: Some(url), 
                checksum, 
                size 
            });
            
            return Ok(()); // Skip normal installation
        }
        
        // Normal installation flow
        self.full_install(ctx, tv).await
    }
}

// Rust backend can skip cargo build during lockfile generation  
impl Backend for RustBackend {
    async fn install_version(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        // Always download source
        let source_path = self.download_source(tv).await?;
        
        if ctx.lockfile_generation {
            // Lockfile generation - extract metadata without building
            let platform_info = self.extract_metadata_only(&source_path).await?;
            let platform_key = self.get_platform_key(); 
            tv.lock_platforms.insert(platform_key, platform_info);
            return Ok(());
        }
        
        // Normal installation - download + build
        self.build_from_source(ctx, tv, &source_path).await
    }
}
```

**Plugin Backend Optimization:**
```bash
# ASDF plugin bin/install can optimize for lockfile generation
#!/usr/bin/env bash
# bin/install

# ASDF plugins use bin/lockfile-info script for lockfile metadata generation
# See plugin system extensions section for implementation details

# Normal installation flow
echo "Installing $ASDF_INSTALL_TYPE $ASDF_INSTALL_VERSION"
# ... full installation logic
```

**VFox Plugin Optimization:**
```lua
function PLUGIN:Install(ctx)
    -- VFox plugins use hooks/lockfile-info.lua hook for lockfile metadata generation
    -- See plugin system extensions section for implementation details
    
    -- Normal installation
    return self:fullInstall(ctx)
end
```

#### Plugin Fallback Strategy
For plugins without lockfile export support:
1. **Temporary Installation**: Use existing `install_version` in temporary directory
2. **Environment Variables**: Set `MISE_PLATFORM` and version info 
3. **Optimized Install**: Backends use `InstallContext.lockfile_generation` to skip expensive steps
4. **Metadata Extraction**: Extract URL/checksum/size from download artifacts
5. **Cleanup**: Remove temporary installation, retain metadata only

#### Package Manager Backend Extensions

**NPM Backend Lockfile Support:**
For `npm:package` tools, resolve metadata without installation:

```rust
impl Backend for NpmBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        let package_name = &tv.request.version(); // e.g., "typescript@5.0.0"
        let (name, version) = package_name.split_once('@').unwrap_or((package_name, "latest"));
        
        // Query npm registry API
        let registry_url = format!("https://registry.npmjs.org/{name}/{version}");
        let package_info: serde_json::Value = self.http_client.get_json(&registry_url).await?;
        
        // npm packages are platform-independent, but we store per-platform for consistency
        let dist = package_info["dist"].as_object().ok_or("Missing dist info")?;
        
        Ok(PlatformInfo {
            url: dist.get("tarball").and_then(|v| v.as_str()).map(String::from),
            checksum: dist.get("shasum").and_then(|v| v.as_str()).map(|s| format!("sha1:{}", s)),
            size: None, // npm doesn't provide size in registry API
        })
    }
}
```

**Pipx Backend Lockfile Support:**
For `pipx:package` tools, resolve metadata from PyPI:

```rust
impl Backend for PipxBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        let package_name = &tv.request.version(); // e.g., "black==23.0.0"
        let (name, version) = if let Some((n, v)) = package_name.split_once("==") {
            (n, v)
        } else {
            (package_name, "latest")
        };
        
        // Query PyPI JSON API  
        let pypi_url = if version == "latest" {
            format!("https://pypi.org/pypi/{name}/json")
        } else {
            format!("https://pypi.org/pypi/{name}/{version}/json")
        };
        
        let package_info: serde_json::Value = self.http_client.get_json(&pypi_url).await?;
        
        // Python packages can be platform-specific (wheels) or universal (sdist)
        let releases = package_info["releases"][version].as_array().ok_or("No releases found")?;
        
        // Prefer wheel for target platform, fallback to source dist
        let preferred_file = releases.iter()
            .find(|f| self.matches_platform(f, target))
            .or_else(|| releases.iter().find(|f| f["packagetype"] == "sdist"))
            .ok_or("No compatible package found")?;
        
        Ok(PlatformInfo {
            url: preferred_file.get("url").and_then(|v| v.as_str()).map(String::from),
            checksum: preferred_file.get("digests").and_then(|d| d.get("sha256"))
                .and_then(|v| v.as_str()).map(|s| format!("sha256:{}", s)),
            size: preferred_file.get("size").and_then(|v| v.as_u64()),
        })
    }
    
    fn matches_platform(&self, file_info: &serde_json::Value, target: &PlatformTarget) -> bool {
        let filename = file_info["filename"].as_str().unwrap_or("");
        
        // Check if wheel filename matches target platform
        // e.g., "black-23.0.0-py3-none-any.whl" or "black-23.0.0-cp39-cp39-macosx_10_9_x86_64.whl"
        if filename.ends_with(".whl") {
            let platform_tags = filename.split('-').collect::<Vec<_>>();
            if platform_tags.len() >= 5 {
                let platform_tag = platform_tags.last().unwrap().trim_end_matches(".whl");
                return self.wheel_platform_matches(platform_tag, target);
            }
        }
        
        false
    }
}
```

**Package Manager Metadata Handling:**
- **npm**: Platform-independent but stored per-platform for lockfile consistency
- **pipx**: Platform-aware with wheel selection based on target platform  
- **Registry APIs**: Use official package registry APIs for reliable metadata
- **Version Resolution**: Support both pinned (`package==1.0.0`) and latest versions

### Error Handling Strategy

#### Error Categories
1. **Fatal Errors** (exit immediately):
   - Invalid platform format
   - Configuration/lockfile parse errors
   - Network connectivity failures
   - **Backend resolution failures** (no warnings/continue - fail fast)

2. **Non-recoverable Errors** (exit immediately):
   - Individual platform resolution failures
   - Missing required metadata (URL/checksum)
   - Unsupported platform for specific backend
   - Backend internal errors

#### Error Handling Strategy
```rust
// Fail fast on any backend error - no warnings/continue
match backend.resolve_lock_info(&tool_version, platform_target).await {
    Ok(platform_info) => {
        // Validate we got essential data
        if platform_info.url.is_none() {
            return Err(eyre!("Backend {} failed to resolve URL for {}@{} on {}", 
                backend.name(), tool_name, version, platform));
        }
        // Checksum optional but log if missing
        if platform_info.checksum.is_none() {
            warn!("No checksum available for {}@{} on {}", 
                tool_name, version, platform);
        }
        Ok(platform_info)
    },
    Err(e) => {
        // Fail immediately - don't continue with partial data
        return Err(e.wrap_err(format!("Failed to resolve lockfile info for {}@{} on {}", 
            tool_name, version, platform)));
    }
}
```

#### Error Messages
```
Error: Failed to resolve lockfile info for node@20.10.0 on linux-x64: checksum manifest not found
Error: Backend 'go' does not support platform windows-arm64
Error: Network request failed for python@3.11.0 platform data
```


## Implementation Plan

### Phase 0: Lockfile Schema Migration (Week 0)

**Tasks:**
1. **Current Lockfile Analysis**:
   - Analyze existing `mise.lock` format and usage patterns
   - Identify migration requirements from single-version to multi-platform schema
   - Document current lockfile structure and dependencies

2. **Migration Strategy Implementation**:
   - Implement lockfile version detection and automatic migration
   - Convert existing single-platform entries to multi-platform format
   - Preserve existing version pins and tool configurations

3. **Backward Compatibility**:
   - Support reading both old and new lockfile formats
   - Automatic migration on first `mise lock` command execution
   - Clear migration messaging and documentation

4. **Schema Versioning**:
   - Add lockfile format version field for future migrations
   - Implement version-aware lockfile parsing
   - Error handling for unsupported future formats

**Migration Logic:**
```rust
// src/lockfile/migration.rs
pub struct LockfileMigrator;

impl LockfileMigrator {
    pub fn migrate_to_v2(&self, old_lockfile: &OldLockfile) -> Result<Lockfile> {
        let mut new_lockfile = Lockfile::new();
        new_lockfile.version = 2;
        
        for (tool_spec, old_entry) in &old_lockfile.tools {
            let (backend_arg, version) = parse_tool_spec(tool_spec)?;
            
            // Convert single entry to multi-platform structure
            let current_platform = PlatformTarget::current()?;
            let mut lock_entry = LockEntry::new();
            
            // Migrate existing data to current platform
            let platform_info = PlatformInfo {
                url: old_entry.url.clone(),
                checksum: old_entry.checksum.clone(),
                size: old_entry.size,
                verification: None, // No verification in old format
            };
            
            lock_entry.platforms.insert(
                current_platform.to_string(), 
                platform_info
            );
            
            new_lockfile.tools.insert(
                backend_arg.full(),
                [(version, lock_entry)].into_iter().collect()
            );
        }
        
        Ok(new_lockfile)
    }
    
    pub fn detect_format_version(content: &str) -> Result<u32> {
        // Try to parse as new format first
        if let Ok(lockfile) = toml::from_str::<Lockfile>(content) {
            return Ok(lockfile.version.unwrap_or(2));
        }
        
        // Try to parse as old format
        if let Ok(_) = toml::from_str::<OldLockfile>(content) {
            return Ok(1);
        }
        
        bail!("Unable to determine lockfile format version")
    }
}

// Legacy format structures
#[derive(Debug, Deserialize)]
struct OldLockfile {
    #[serde(flatten)]
    tools: HashMap<String, OldLockEntry>,
}

#[derive(Debug, Deserialize)]
struct OldLockEntry {
    url: Option<String>,
    checksum: Option<String>,
    size: Option<u64>,
    // Other legacy fields...
}
```

**CLI Integration:**
```rust
// src/cli/lock.rs
impl Lock {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        
        // Check if migration is needed
        if let Ok(existing_content) = fs::read_to_string(&lockfile_path) {
            let format_version = LockfileMigrator::detect_format_version(&existing_content)?;
            
            if format_version == 1 {
                info!("Migrating lockfile from v1 to v2 format...");
                let old_lockfile = toml::from_str::<OldLockfile>(&existing_content)?;
                let migrated = LockfileMigrator::migrate_to_v2(&old_lockfile)?;
                
                // Backup old lockfile
                fs::copy(&lockfile_path, lockfile_path.with_extension("lock.v1.bak"))?;
                
                // Write migrated lockfile
                let content = toml::to_string_pretty(&migrated)?;
                fs::write(&lockfile_path, content)?;
                
                info!("Migration complete. Backup saved as mise.lock.v1.bak");
            }
        }
        
        // Continue with normal lockfile generation...
        self.generate_lockfile(config).await
    }
}
```

**Migration Examples:**

**Before (v1 format):**
```toml
# mise.lock (v1)
"node@22.11.0" = { url = "https://nodejs.org/dist/v22.11.0/node-v22.11.0-darwin-arm64.tar.xz", checksum = "sha256:abc123...", size = 25147968 }
"python@3.11.8" = { url = "https://www.python.org/ftp/python/3.11.8/Python-3.11.8.tar.xz", checksum = "sha256:def456...", size = 18429448 }
```

**After (v2 format):**
```toml
# mise.lock (v2) 
version = 2

[node."22.11.0"]
[node."22.11.0"."macos-arm64"]
url = "https://nodejs.org/dist/v22.11.0/node-v22.11.0-darwin-arm64.tar.xz"
checksum = "sha256:abc123..."
size = 25147968

[python."3.11.8"] 
[python."3.11.8"."macos-arm64"]
url = "https://www.python.org/ftp/python/3.11.8/Python-3.11.8.tar.xz"
checksum = "sha256:def456..."
size = 18429448
```

**Deliverables:**
- Automatic lockfile migration from v1 to v2 format
- Backward compatibility for reading legacy lockfiles
- Schema versioning system for future migrations
- Clear user communication about migration process
- Comprehensive testing of migration edge cases

### Phase 1: GitHub Releases Test Tool Creation (Week 1)

**Tasks:**
1. **Create mise-test-tool Repository**:
   - New GitHub repo: `mise-plugins/mise-test-tool` 
   - Simple Node.js CLI script that prints platform info and version
   - No compilation needed - just package Node.js script with shebang
   - Create platform-specific archives: `linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64`, `windows-x64`
   - Include multiple verification methods: GPG signatures, GitHub attestations, Minisign
   - Add to mise repo via `git subtree` at `test/fixtures/mise-test-tool/`

2. **Release Infrastructure**:
   - GitHub Actions workflow for packaging (no compilation needed)
   - Asset naming: `mise-test-tool-v{version}-{os}-{arch}.{ext}` (tar.gz/zip)
   - Package contents:
     - `bin/mise-test-tool` (Node.js script with shebang)
     - `bin/mise-test-tool.cmd` (Windows batch shim)
     - `package.json` (version metadata)
   - Generate verification artifacts:
     - GPG signatures (`.asc` files)  
     - Minisign signatures (`.minisig` files)
     - GitHub artifact attestations
     - SHA256SUMS file with checksums
   - Publish to GitLab as well for multi-registry testing

3. **Tool Implementation**:
   ```javascript
   #!/usr/bin/env node
   // bin/mise-test-tool
   
   const os = require('os');
   const process = require('process');
   const fs = require('fs');
   const path = require('path');
   
   // Read version from package.json
   const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'));
   
   console.log(`mise-test-tool v${packageJson.version}`);
   console.log(`Platform: ${os.platform()}-${os.arch()}`);
   console.log(`Arguments: ${JSON.stringify(process.argv.slice(2))}`);
   console.log('Environment variables:');
   
   Object.keys(process.env)
     .filter(key => key.startsWith('MISE_'))
     .sort()
     .forEach(key => {
       console.log(`  ${key}=${process.env[key]}`);
     });
   
   // Exit successfully
   process.exit(0);
   ```

   ```cmd
   @echo off
   REM bin/mise-test-tool.cmd - Windows shim
   node "%~dp0\mise-test-tool" %*
   ```

4. **Registry Submissions**:
   - Submit to Aqua registry for `aqua:mise-plugins/mise-test-tool`
   - Create HTTP backend test URLs pointing to GitHub release assets
   - Document usage patterns for all supported backends

**Deliverables:**
- Working `mise-test-tool` with multi-platform releases and verification
- Integration into mise repo via git subtree
- Registry submissions (GitHub, GitLab, Aqua)  
- Complete verification artifact suite
- Foundation for testing all backend types (github, http, ubi, aqua)

### Phase 2: Core Backend Infrastructure (Week 2)
**Tasks:**
1. **Backend Trait Extensions**:
   - Add `get_tarball_url()` and `get_github_release_info()` optional methods
   - Add `resolve_lock_info()` with shared tarball/GitHub release logic
   - Implement `resolve_lock_info_from_tarball()` and `resolve_lock_info_from_github_release()`
   - Update `InstallContext` with `target_platform` field and `lockfile_generation` boolean

2. **Basic Data Structures**:
   - Implement `PlatformTarget` structure for platform representation
   - Basic `PlatformInfo` with url, checksum, size (no verification yet)
   - Implement `GitHubReleaseInfo` structure

3. **Settings Integration**:
   - Add `MISE_LOCK_PLATFORMS` environment variable to settings.toml
   - Update CLI to support `--platforms` flag with setting fallback

4. **Test Tool Integration**:
   - Add `github:mise-plugins/mise-test-tool` as test backend
   - Implement `ubi:mise-plugins/mise-test-tool` for UBI testing  
   - Add HTTP backend URLs pointing to GitHub release assets
   - Test Aqua registry integration with `aqua:mise-plugins/mise-test-tool`

**Deliverables:**
- Extended Backend trait with simplified interface
- Working tarball and GitHub release shared logic
- Basic lockfile generation without verification
- Comprehensive test coverage using mise-test-tool across all backend types

**Architecture:**
```rust
// Simple implementation - Node.js example
impl Backend for NodeBackend {
    async fn get_tarball_url(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<Option<String>> {
        let url = format!("https://nodejs.org/dist/v{}/node-v{}-{}-{}.tar.xz", 
            tv.version, tv.version, target.os_name(), target.arch_name());
        Ok(Some(url))
    }
    
    async fn get_verification_info(&self, tv: &ToolVersion, target: &PlatformTarget, asset_url: &str) -> Result<Vec<VerificationInfo>> {
        Ok(vec![VerificationInfo::GpgSignature(GpgSignatureInfo {
            signature_url: format!("{}.asc", asset_url),
            keyring: "node".to_string(),
        })])
    }
}

// GitHub releases - Bun example  
impl Backend for BunBackend {
    async fn get_github_release_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<Option<GitHubReleaseInfo>> {
        Ok(Some(GitHubReleaseInfo {
            repo: "oven-sh/bun".to_string(),
            asset_pattern: Some(format!("bun-{}-{}.zip", target.os_name(), target.arch_name())),
            api_url: None,
            release_type: ReleaseType::GitHub,
        }))
    }
}
```

**Deliverables:**
- 7+ core tools with lockfile generation support
- Mix of tarball URL, GitHub release, and fallback approaches

### Phase 3: Verification System (Week 3)
**Tasks:**
1. **VerificationInfo Enum Implementation**:
   - Implement `VerificationInfo` enum with variants for GPG, Cosign, SLSA, GitHub attestations, Minisign
   - Create individual structs: `GpgSignatureInfo`, `GitHubAttestationInfo`, `MinisignSignatureInfo`, etc.
   - Update `PlatformInfo` to use `verification_infos: Vec<VerificationInfo>`

2. **Verification Engine**:
   - Implement unified `VerificationEngine` for artifact validation
   - Support multiple verification methods in parallel
   - Integration with external tools (gpg, gh, minisign, cosign)

3. **Backend Verification Integration**:
   - Add `get_verification_info()` method to Backend trait
   - Implement verification metadata collection during lockfile generation
   - Update mise-test-tool to include all verification artifact types

4. **Configuration Settings**:
   - Add verification-related settings to settings.toml
   - Support for enabling/disabling specific verification methods
   - Error handling for missing verification tools

**Architecture:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationInfo {
    GpgSignature(GpgSignatureInfo),
    GitHubAttestation(GitHubAttestationInfo),
    MinisignSignature(MinisignSignatureInfo),
    CosignAttestation(CosignAttestationInfo),
    SlsaProvenance(SlsaProvenanceInfo),
}

pub struct VerificationEngine;
impl VerificationEngine {
    pub async fn verify_all(
        file_path: &Path,
        verification_infos: &[VerificationInfo],
        pr: Option<&ProgressReport>
    ) -> Result<()> { /* ... */ }
}
```

**Deliverables:**
- Complete verification system with enum-based architecture
- Working verification for mise-test-tool releases
- Foundation for secure frozen installs

### Phase 4: Core Tool Implementation (Week 4)  
**Tasks:**
1. **Simple Tarball Tools** (highest ROI):
   - **Node.js**: Implement `get_tarball_url()` with nodejs.org URL pattern + GPG verification
   - **Python**: Implement `get_tarball_url()` with python.org URL pattern + GPG verification
   - **Go**: Implement `get_tarball_url()` with golang.org URL pattern + checksum verification
   - **Ruby**: Implement `get_tarball_url()` with ruby-lang.org URL pattern + GPG verification

2. **GitHub Release Tools**:
   - **Bun**: Implement `get_github_release_info()` with oven-sh/bun repo + GitHub attestations
   - **Deno**: Implement `get_github_release_info()` with denoland/deno repo + GitHub attestations
   - **Zig**: Implement `get_github_release_info()` with ziglang/zig repo + Minisign verification

3. **Complex Tools** (fallback approach):
   - **Java**: Use temporary installation fallback for Adoptium/Eclipse Temurin
   - **Elixir/Erlang**: Use temporary installation fallback

4. **E2E Testing with mise-test-tool**:
   - Test multi-platform lockfile generation with verification
   - Verify verification metadata collection and validation
   - Test incremental updates and platform additions

**Deliverables:**  
- Working lockfile generation for popular core tools
- Comprehensive verification support across different tool sources
- Real-world testing using mise-test-tool infrastructure

**Package Manager Architecture:**
```rust
impl Backend for NpmBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        // Query npm registry for platform-specific binaries
        let registry_data = self.query_npm_registry(tv).await?;
        self.extract_platform_info(registry_data, target)
    }
}

impl Backend for CargoBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        // Most Cargo packages are source-only, limited platform-specific support
        if self.has_platform_specific_binaries(tv).await? {
            self.resolve_cargo_platform_info(tv, target).await
        } else {
            // Source-based install, same for all platforms
            self.resolve_source_info(tv).await
        }
    }
}
```

**Deliverables:**
- Package manager backend lockfile support
- Registry API integrations
- Platform-specific package resolution

### Phase 3b: Distribution Backend Implementation (Week 3-4)
**Tasks:**
1. Implement `resolve_lock_info` for Distribution backends:
   - **GitHub** - GitHub releases API for generic tools
   - **Gitlab** - GitLab releases API integration
   - **HTTP** - Generic HTTP download with checksum support
   - **UBI** - GitHub releases with smart asset detection
   - **Aqua** - Aqua registry integration (existing support)

2. Add generic asset detection and platform mapping
3. Implement release artifact pattern matching

**Distribution Backend Architecture:**
```rust
impl Backend for GitHubBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        let releases = self.fetch_github_releases(tv).await?;
        let asset = self.detect_platform_asset(&releases, target)?;
        
        PlatformInfo {
            url: Some(asset.browser_download_url),
            checksum: self.resolve_asset_checksum(&asset).await?,
            size: Some(asset.size),
            verification: self.collect_asset_verification(&asset).await?,
        }
    }
}

impl Backend for HttpBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        // Template-based URL construction for HTTP backends
        let url = self.template_url(tv, target)?;
        let checksum = self.resolve_checksum_from_sources(&url).await?;
        
        PlatformInfo {
            url: Some(url),
            checksum,
            size: self.get_content_length(&url).await?,
            verification: None, // HTTP backends generally don't have verification
        }
    }
}
```

**Deliverables:**
- Generic distribution backend lockfile support  
- Asset detection and platform mapping
- Release artifact pattern matching

### Phase 5: Advanced Features and Multiple Environments (Week 5)
**Tasks:**
1. **Environment-Specific Lockfiles**:
   - Support MISE_ENV values like `development`, `production`, `ci`
   - Generate individual lockfiles: `mise.development.lock`, `mise.production.lock` 
   - Runtime lockfile merging in `resolve()` phase
   - Environment variable thread-safety via `InstallContext.target_platform`

2. **CLI Enhancements**:
   - Add `mise lock --check` flag for validation-only mode
   - Add `mise lock --dry-run` flag for preview mode  
   - Implement `--force` flag to refresh existing entries
   - Add progress reporting and `--jobs` parallel execution

3. **Incremental Updates**:
   - Update lockfile entries only for specified tools
   - Preserve existing platform data when adding new platforms
   - Smart merge strategies for lockfile updates

4. **Package Manager and Distribution Backends**:
   - **NPM Backend**: Add support for `npm:package` tools with lockfile generation
   - **Pipx Backend**: Add support for `pipx:package` tools with PyPI metadata
   - **Aqua**: Extend existing Aqua backend with lockfile support (registry-based)
   - **UBI**: Extend existing UBI backend with lockfile support using mise-test-tool
   - **HTTP Backend**: Extend to support multiple platform URLs using mise-test-tool GitHub assets

**Architecture:**
```rust
// New trait for plugin backends
#[async_trait]
pub trait PluginBackend: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    
    // Required: List available versions
    async fn list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<String>>;
    
    // Required: Resolve platform-specific download info
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo>;
    
    // Required: Install a specific version  
    async fn install_version(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion>;
    
    // Optional: Custom verification support
    async fn verify_install(&self, tv: &ToolVersion) -> Result<()> {
        Ok(()) // Default no-op
    }
    
    // Optional: Platform support detection
    fn supports_platform(&self, target: &PlatformTarget) -> bool {
        true // Default support all platforms
    }
}

// Plugin backend registry and loading
pub struct PluginBackendRegistry {
    backends: HashMap<String, Box<dyn PluginBackend>>,
}

impl PluginBackendRegistry {
    pub fn discover_backends(&mut self) -> Result<()> {
        // Discover backends in ~/.local/share/mise/backends/
        let backends_dir = dirs::CONFIG_DIR.join("backends");
        for entry in std::fs::read_dir(backends_dir)? {
            if let Ok(backend) = self.load_plugin_backend(&entry.path()) {
                self.backends.insert(backend.name().to_string(), backend);
            }
        }
        Ok(())
    }
    
    fn load_plugin_backend(&self, path: &Path) -> Result<Box<dyn PluginBackend>> {
        // Load plugin backend from directory structure:
        // ~/.local/share/mise/backends/my-backend/
        // ├── backend.toml          # Backend configuration
        // ├── bin/
        // │   ├── list-all         # List remote versions
        // │   ├── lockfile-info    # Platform-specific info
        // │   ├── install          # Install version
        // │   └── verify           # Optional verification
        // └── README.md
        
        let config_file = path.join("backend.toml");
        let backend_config: PluginBackendConfig = toml::from_str(&std::fs::read_to_string(config_file)?)?;
        
        Ok(Box::new(ScriptPluginBackend::new(backend_config, path.to_path_buf())))
    }
}

// Script-based plugin backend implementation
pub struct ScriptPluginBackend {
    config: PluginBackendConfig,
    backend_path: PathBuf,
    script_manager: ScriptManager,
}

impl ScriptPluginBackend {
    async fn resolve_lock_info(&self, tv: &ToolVersion, target: &PlatformTarget) -> Result<PlatformInfo> {
        let lockfile_info_script = self.backend_path.join("bin/lockfile-info");
        if !lockfile_info_script.exists() {
            return Err(eyre!("Plugin backend {} missing bin/lockfile-info", self.config.name));
        }
        
        let mut sm = self.script_manager.clone();
        sm = sm.with_env("MISE_PLATFORM", target.to_string());
        sm = sm.with_env("BACKEND_INSTALL_VERSION", &tv.version);
        
        let output = sm.cmd(&Script::Custom(lockfile_info_script)).read()?;
        self.parse_lockfile_info_output(&output)
    }
    
    fn parse_lockfile_info_output(&self, output: &str) -> Result<PlatformInfo> {
        // Parse key=value output format
        let mut url = None;
        let mut checksum = None;
        let mut size = None;
        
        for line in output.lines() {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 { continue; }
            
            match parts[0] {
                "url" => url = Some(parts[1].to_string()),
                "checksum" => checksum = Some(parts[1].to_string()),
                "size" => size = parts[1].parse().ok(),
                _ => {} // Ignore unknown keys
            }
        }
        
        Ok(PlatformInfo {
            url,
            checksum,
            size,
            verification: None, // Plugin backends can extend this
        })
    }
}

#[derive(Deserialize)]
struct PluginBackendConfig {
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    supported_platforms: Option<Vec<String>>,
    verification_types: Option<Vec<String>>,
}
```

**Plugin Backend Examples:**

**Custom Docker Registry Backend:**
```toml
# ~/.local/share/mise/backends/docker-tools/backend.toml
name = "docker-tools"
version = "1.0.0"
description = "Backend for Docker-distributed tools"
author = "community"
supported_platforms = ["linux-x64", "linux-arm64", "darwin-arm64", "darwin-x64"]
verification_types = ["cosign"]
```

```bash
#!/usr/bin/env bash
# ~/.local/share/mise/backends/docker-tools/bin/lockfile-info

set -eu -o pipefail

TOOL="${BACKEND_TOOL_NAME}"
VERSION="${BACKEND_INSTALL_VERSION}"
PLATFORM="${MISE_PLATFORM}"

# Convert mise platform format to Docker platform
case "$PLATFORM" in
    "linux-x64") DOCKER_PLATFORM="linux/amd64" ;;
    "linux-arm64") DOCKER_PLATFORM="linux/arm64" ;;
    "darwin-x64") DOCKER_PLATFORM="darwin/amd64" ;;
    "darwin-arm64") DOCKER_PLATFORM="darwin/arm64" ;;
    *) 
        echo "Unsupported platform: $PLATFORM" >&2
        exit 1
        ;;
esac

# Query Docker registry for image manifest
REGISTRY_URL="registry.hub.docker.com"
IMAGE_NAME="$TOOL"
TAG="$VERSION"

MANIFEST_URL="https://$REGISTRY_URL/v2/$IMAGE_NAME/manifests/$TAG"
DIGEST=$(curl -s -H "Accept: application/vnd.docker.distribution.manifest.v2+json" "$MANIFEST_URL" | jq -r '.config.digest')

echo "url=docker://$IMAGE_NAME:$TAG"
echo "checksum=sha256:${DIGEST#sha256:}"
echo "platform=$DOCKER_PLATFORM"
```

**Community Package Registry Backend:**
```bash
#!/usr/bin/env bash  
# ~/.local/share/mise/backends/custom-registry/bin/lockfile-info

set -eu -o pipefail

TOOL="${BACKEND_TOOL_NAME}"
VERSION="${BACKEND_INSTALL_VERSION}"
PLATFORM="${MISE_PLATFORM}"

# Query custom package registry API
REGISTRY_API="https://my-package-registry.com/api/v1"
RESPONSE=$(curl -s "$REGISTRY_API/packages/$TOOL/$VERSION")

URL=$(echo "$RESPONSE" | jq -r ".platforms.\"$PLATFORM\".download_url")
CHECKSUM=$(echo "$RESPONSE" | jq -r ".platforms.\"$PLATFORM\".checksum")
SIZE=$(echo "$RESPONSE" | jq -r ".platforms.\"$PLATFORM\".size")

echo "url=$URL"
echo "checksum=$CHECKSUM"
echo "size=$SIZE"
```

**Deliverables:**
- Plugin backend architecture and SDK
- Enhanced ASDF/VFox backend lockfile support
- Plugin backend discovery and loading system
- Community plugin backend templates and examples
- Plugin backend testing framework

### Phase 6: Plugin Integration and Frozen Install (Week 6)

**Plugin System Extensions:**

1. **ASDF/VFox Plugin Extensions**:
   - Extend existing ASDF/VFox backends to call `bin/lockfile-info` and `hooks/lockfile-info.lua`
   - Add environment variable support (`MISE_PLATFORM`)
   - Implement validation in mise core (not in plugins)
   - Test using mise-test-tool as ASDF plugin

**Frozen Install Implementation:**

Complete `mise install --frozen` flag that installs exactly from lockfile data with verification:

```rust
// src/cli/install.rs - Enhanced Install command
#[derive(Debug, clap::Args)]
pub struct Install {
    /// Install tools exactly as specified in lockfile (with verification)
    #[clap(long)]
    frozen: bool,
    
    // ... existing fields
}

impl Install {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        
        // Load lockfile if frozen mode
        let lockfile_data = if self.frozen {
            Some(self.load_lockfile_for_frozen_install(&config).await?)
        } else {
            None
        };
        
        let mut ts = ToolsetBuilder::new().build(&config).await?;
        
        if let Some(lockfile) = &lockfile_data {
            // Validate lockfile against config - error if mismatch
            self.validate_lockfile_against_config(&ts, lockfile).await?;
            
            // Apply lockfile constraints to toolset
            self.apply_lockfile_constraints(&mut ts, lockfile).await?;
        }
        
        ts.install_all(&config, self.jobs, self.force).await?;
        Ok(())
    }
}
```

**Enhanced `mise tool` Integration:**

Add verification methods and platform support to existing detailed output:

```bash
$ mise tool node
Backend:             core
Description:         Node.js JavaScript runtime
Installed Versions:  20.0.0 22.0.0
Active Version:      20.0.0
Requested Version:   20
Config Source:       ~/.config/mise/mise.toml
Tool Options:        [none]
Verification:        GPG signatures
Supported Platforms: linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64
```

**Backend Frozen Install Support:**

```rust
// Enhanced InstallContext with frozen mode
pub struct InstallContext {
    pub target_platform: PlatformTarget,
    pub lockfile_generation: bool,
    pub frozen_mode: bool,          // New field
    pub verification_enabled: bool, // New field
    // ... existing fields
}

// Backends use lockfile data directly in frozen mode
impl Backend for UnifiedGitBackend {
    async fn install_version_(&self, ctx: &InstallContext, mut tv: ToolVersion) -> Result<ToolVersion> {
        if ctx.frozen_mode {
            // Use lockfile platform info instead of GitHub API
            return self.install_from_lockfile(ctx, tv).await;
        }
        // ... existing non-frozen implementation
    }
}
```

**Verification Engine:**

```rust
// src/verification/mod.rs - Unified verification system
pub struct VerificationEngine;

impl VerificationEngine {
    pub async fn verify_all(
        file_path: &Path,
        verification_infos: &[VerificationInfo],
        pr: Option<&ProgressReport>
    ) -> Result<()> {
        for verification in verification_infos {
            match verification {
                VerificationInfo::GpgSignature(info) => {
                    Self::verify_gpg(file_path, info, pr).await?;
                }
                VerificationInfo::GitHubAttestation(info) => {
                    Self::verify_github_attestation(file_path, info, pr).await?;
                }
                VerificationInfo::MinisignSignature(info) => {
                    Self::verify_minisign(file_path, info, pr).await?;
                }
                // ... other verification types
            }
        }
        Ok(())
    }
}
```

**Tasks:**
1. Complete `mise install --frozen` implementation
2. Lockfile validation against configuration files  
3. Enhanced `mise tool` command with verification and platform info
4. Unified verification engine for multiple signature types
5. Integration tests covering frozen install workflows
6. Error handling for lockfile/config mismatches

**Deliverables:**
- Complete frozen install workflow with verification
- Enhanced CLI commands with lockfile integration
- Comprehensive error handling and validation
- Integration tests for secure deployment workflows

## Implementation Summary

This specification defines a comprehensive system for multi-platform lockfile support in mise with the following key components:

**Phase-by-Phase Implementation:**
0. **Schema Migration**: Automatic migration from v1 to v2 lockfile format
1. **Test Tool Creation**: GitHub releases test tool with Node.js CLI
2. **Core Infrastructure**: Basic lockfile generation and platform support
3. **Verification System**: Cryptographic verification with enum-based architecture
4. **Core Tool Implementation**: Popular tools with verification support
5. **Advanced Features**: Multiple environments and incremental updates
6. **Plugin Integration & Frozen Install**: ASDF/VFox plugins and secure deployment

**Key Technical Innovations:**
- Simplified backend interface using tarball URLs and GitHub release patterns
- Thread-safe multi-platform resolution via InstallContext
- Individual environment lockfiles with runtime merging
- Reuse of existing GitHub backend logic for asset resolution
- Enum-based verification system for multiple cryptographic methods

**Production-Ready Features:**
- Complete `mise lock` command with multi-platform support
- `mise install --frozen` for secure, reproducible deployments
- Enhanced `mise tool` command showing verification methods and platforms
- Comprehensive error handling and validation
- Integration with existing plugin ecosystems

## Verification Support Matrix

| Backend/Tool | GPG | Cosign | SLSA | GitHub | Minisign | Notes |
|-------------|-----|--------|------|--------|----------|-------|
| Node.js | ✓ | - | - | - | - | Official GPG signatures |
| Bun | - | - | - | ✓ | ✓ | GitHub attestations + Minisign |
| Go | ✓ | - | - | - | - | Official checksums with GPG |
| Python | ✓ | - | - | - | - | Official releases signed |
| GitHub Backend | Plugin | ✓ | ✓ | ✓ | ✓ | Per-repository configuration |
| ASDF Plugins | Plugin | Plugin | Plugin | Plugin | Plugin | Plugin-dependent |
| VFox Plugins | Plugin | Plugin | Plugin | Plugin | Plugin | Plugin-dependent |

**Legend:**
- **✓**: Supported by backend implementation
- **Plugin**: Verification depends on individual plugin implementation  
- **-**: Not applicable or not supported

## Testing Strategy

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_platform_parsing() {
        assert_eq!(
            PlatformTarget::parse("macos-arm64").unwrap(),
            PlatformTarget { os: "macos".into(), arch: "arm64".into(), qualifier: None }
        );
    }
    
    #[tokio::test] 
    async fn test_node_backend_resolve_lock_info() {
        // Test Node.js platform resolution
    }
}
```

### Integration Tests
```bash
# E2E test scenarios
mise lock --platforms macos-arm64,linux-x64 node     # Multi-platform generation
mise lock --platforms linux-x64 node                # Incremental updates  
mise lock --force --platforms macos-arm64,linux-x64  # Force refresh
mise lock --dry-run --platforms windows-x64 node     # Preview mode
mise install --frozen                                 # Frozen install verification
```

## Security Considerations

### Checksum Verification
- Always verify checksums when available during `mise install`
- Error on checksum mismatches in frozen installs
- Support SHA256 primary, with fallback to other algorithms

### Network Security
- Use HTTPS for all metadata and asset requests
- Validate SSL certificates and implement timeouts
- Rate limiting for API requests

### Supply Chain Security  
- Pin exact versions and checksums in lockfiles
- Support cryptographic verification (GPG, GitHub attestations, Minisign, etc.)
- Enable offline/air-gapped environments via complete lockfile metadata

## Performance Considerations

### Network Optimization
- Concurrent platform resolution with `--jobs` limiting
- HTTP/2 connection reuse where possible
- Efficient API request batching

### Lockfile Optimization
- Reuse existing lockfile data to avoid redundant API calls
- Memory-efficient parsing for large release lists
- Progressive resolution and validation

---

**Spec Version:** 2.0  
**Last Updated:** 2025-01-15  
**Author:** Claude Code Assistant  
**Status:** Implementation Ready
