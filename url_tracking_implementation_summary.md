# URL Tracking Implementation Summary

## Overview

This document summarizes the implementation of URL tracking in mise lockfiles, allowing backends to store the original download URLs of assets alongside checksums and sizes.

## Changes Made

### 1. ToolVersion Structure Updates

**File**: `src/toolset/tool_version.rs`

Added a new `urls` field to the `ToolVersion` struct:

```rust
pub struct ToolVersion {
    pub request: ToolRequest,
    pub version: String,
    pub checksums: BTreeMap<String, String>,
    pub sizes: BTreeMap<String, u64>,
    pub urls: BTreeMap<String, String>,  // New field
    pub install_path: Option<PathBuf>,
}
```

Updated the constructor to initialize the new field:

```rust
impl ToolVersion {
    pub fn new(request: ToolRequest, version: String) -> Self {
        ToolVersion {
            request,
            version,
            checksums: Default::default(),
            sizes: Default::default(),
            urls: Default::default(),  // New initialization
            install_path: None,
        }
    }
}
```

### 2. Lockfile Integration Updates

**File**: `src/lockfile.rs`

Updated the `From<ToolVersionList>` implementation to include URLs in the assets conversion:

```rust
// Convert checksums, sizes, and URLs to assets format
for (filename, url) in &tv.urls {
    let asset = assets.entry(filename.clone()).or_insert_with(|| AssetInfo {
        checksum: None,
        size: None,
        url: None,
    });
    asset.url = Some(url.clone());
}
```

### 3. Backend URL Storage Implementation

#### Aqua Backend

**File**: `src/backend/aqua.rs`

Added URL storage in the `install_version_` method:

```rust
let filename = url.split('/').next_back().unwrap();

// Store the asset URL in the tool version
tv.urls.insert(filename.to_string(), url.clone());

self.download(ctx, &tv, &url, filename).await?;
```

#### HTTP Backend

**File**: `src/backend/http.rs`

Added URL storage in the `install_version_` method:

```rust
// Download
let filename = get_filename_from_url(&url);
let file_path = tv.download_path().join(&filename);

// Store the asset URL in the tool version
tv.urls.insert(filename.clone(), url.clone());

ctx.pr.set_message(format!("download {filename}"));
HTTP.download_file(&url, &file_path, Some(&ctx.pr)).await?;
```

#### GitHub/GitLab Backend

**File**: `src/backend/github.rs`

Added URL storage in the `download_and_install` method:

```rust
let filename = get_filename_from_url(asset_url);
let file_path = tv.download_path().join(&filename);

// Store the asset URL in the tool version
tv.urls.insert(filename.clone(), asset_url.to_string());

ctx.pr.set_message(format!("download {filename}"));
HTTP.download_file(asset_url, &file_path, Some(&ctx.pr)).await?;
```

### 4. Backend Support Matrix Updates

**File**: `docs/dev-tools/mise-lock.md`

Updated backend support documentation:

- ✅ **Full support** (version + checksum + size + URL): `aqua`, `http`, `github`, `gitlab`
- ⚠️ **Partial support** (version + checksum + size): `ubi`
- 📝 **Basic support** (version + checksum): `core` (some tools)
- 📝 **Version only**: `asdf`, `npm`, `cargo`, `pipx`

### 5. Test Coverage

**File**: `e2e/lockfile/test_lockfile_urls`

Created a new test to verify URL storage:

```bash
#!/usr/bin/env bash

export MISE_LOCKFILE=1
export MISE_EXPERIMENTAL=1

# Test that aqua backend stores URLs in lockfile
rm -rf mise.lock

# Install a tool via aqua backend that should have URLs
mise use ripgrep@14.1.1

# Check that the lockfile was created and contains URLs
assert_contains "cat mise.lock" "[tools.ripgrep.assets]"
assert_contains "cat mise.lock" "url ="
```

## Example Lockfile Format

With URL tracking enabled, lockfiles now include complete asset information:

```toml
[tools.ripgrep]
version = "14.1.1"
backend = "aqua:BurntSushi/ripgrep"

[tools.ripgrep.assets]
"ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz" = { 
  checksum = "sha256:4cf9f2741e6c465ffdb7c26f38056a59e2a2544b51f7cc128ef28337eeae4d8e",
  size = 1234567,
  url = "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"
}
```

## Benefits of URL Tracking

1. **Full Traceability**: Complete audit trail of where assets originated
2. **Enhanced Security**: Better compliance and security auditing capabilities
3. **Debugging Support**: Easier to diagnose download issues and verify sources
4. **Offline Workflows**: Knowledge of exact sources enables better offline support
5. **Mirror Detection**: Foundation for automatic mirror detection and failover
6. **Compliance**: Meets enterprise requirements for asset source tracking

## Implementation Details

### URL Storage Pattern

Each backend follows a consistent pattern:

1. **Generate/Fetch URL**: Determine the download URL for the asset
2. **Extract Filename**: Get the filename from the URL (usually the last path segment)
3. **Store URL**: Add the URL to `tv.urls` using the filename as the key
4. **Download**: Proceed with normal download and verification

### Filename Key Strategy

- All backends use the filename (last URL path segment) as the key
- This ensures consistency across backends and enables easy lookup
- Filenames are unique within a tool version context

### Backend-Specific Considerations

- **Aqua**: URLs come from the aqua registry configuration
- **HTTP**: URLs are directly specified in tool configuration
- **GitHub/GitLab**: URLs come from release API responses
- **UBI**: Not implemented (uses external tool for downloads)

## Future Enhancements

1. **URL Validation**: Verify URLs are accessible and match stored checksums
2. **Mirror Support**: Use URLs to automatically detect and configure mirrors
3. **Offline Mode**: Download and cache assets locally using URL information
4. **Security Scanning**: Integrate with security tools using URL metadata
5. **Dependency Tracking**: Build dependency graphs using URL relationships

## Testing Strategy

- **Unit Tests**: Verify URL storage in individual backends
- **Integration Tests**: Test complete lockfile generation with URLs
- **E2E Tests**: Verify URL functionality in real-world scenarios
- **Migration Tests**: Ensure backwards compatibility with existing lockfiles

This implementation provides a solid foundation for enhanced asset tracking and security in mise tool management.