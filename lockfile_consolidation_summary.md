# Lockfile Consolidation: Summary of Changes

## Overview

This document summarizes the changes made to consolidate checksums and sizes in the `mise.lock` file to reduce duplication by introducing a centralized `assets` section.

## Problem

The original `mise.lock` format duplicated filenames across multiple tools:

```toml
[tools.actionlint]
version = "1.7.7"
backend = "aqua:rhysd/actionlint"

[tools.actionlint.checksums]
"actionlint_1.7.7_linux_amd64.tar.gz" = "sha256:023070a287cd8cccd71515fedc843f1985bf96c436b7effaecce67290e7e0757"

[tools.bun]
version = "1.2.18"
backend = "core:bun"

[tools.bun.checksums]
"bun-linux-x64-baseline.zip" = "sha256:c7504c216d8729105d1781385665e3ac2c63debdbb5de4efdc12e2d6c4a6cb4a"
```

This caused significant duplication when the same assets were used by multiple tools.

## Solution

### New Format

The new format consolidates all asset information into a single `assets` section:

```toml
[tools.actionlint]
version = "1.7.7"
backend = "aqua:rhysd/actionlint"

[tools.bun]
version = "1.2.18"
backend = "core:bun"

[assets]
"actionlint_1.7.7_linux_amd64.tar.gz" = { checksum = "sha256:023070a287cd8cccd71515fedc843f1985bf96c436b7effaecce67290e7e0757" }
"bun-linux-x64-baseline.zip" = { checksum = "sha256:c7504c216d8729105d1781385665e3ac2c63debdbb5de4efdc12e2d6c4a6cb4a" }
```

### Asset Structure

Each asset can now include:
- `checksum`: Optional SHA256/Blake3 checksum
- `size`: Optional file size in bytes
- `url`: Optional download URL

Example with all fields:
```toml
[assets]
"example.zip" = { 
  checksum = "sha256:abc123", 
  size = 1024, 
  url = "https://example.com/example.zip" 
}
```

## Implementation Details

### 1. New Data Structures

Added `AssetInfo` struct to represent asset metadata:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}
```

### 2. Updated Lockfile Structure

Modified the `Lockfile` struct to include an `assets` field:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(skip)]
    tools: BTreeMap<String, Vec<LockfileTool>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    assets: BTreeMap<String, AssetInfo>,
}
```

### 3. Migration Logic

Implemented automatic migration from the legacy format:

```rust
fn migrate_legacy_format(&mut self) {
    // Move checksums and sizes from individual tools to the assets section
    for (_tool_name, versions) in &mut self.tools {
        for version in versions {
            // Migrate checksums
            for (filename, checksum) in version.checksums.drain() {
                let asset = self.assets.entry(filename).or_insert_with(|| AssetInfo {
                    checksum: None,
                    size: None,
                    url: None,
                });
                if asset.checksum.is_none() {
                    asset.checksum = Some(checksum);
                }
            }
            
            // Migrate sizes
            for (filename, size) in version.sizes.drain() {
                let asset = self.assets.entry(filename).or_insert_with(|| AssetInfo {
                    checksum: None,
                    size: None,
                    url: None,
                });
                if asset.size.is_none() {
                    asset.size = Some(size);
                }
            }
        }
    }
}
```

### 4. Backward Compatibility

- Legacy fields (`checksums` and `sizes`) are preserved in `LockfileTool` for migration
- When reading locked versions, checksums and sizes are populated from the assets section
- Legacy lockfiles are automatically migrated when read

### 5. Helper Methods

Added convenience methods for asset management:

```rust
pub fn get_checksum(&self, filename: &str) -> Option<&String>
pub fn get_size(&self, filename: &str) -> Option<u64>
pub fn get_url(&self, filename: &str) -> Option<&String>
pub fn set_asset_info(&mut self, filename: String, checksum: Option<String>, size: Option<u64>, url: Option<String>)
```

## Key Changes by File

### `src/lockfile.rs`

1. **Added new structures**: `AssetInfo` struct for asset metadata
2. **Updated `Lockfile` struct**: Added `assets` field
3. **Added migration logic**: `migrate_legacy_format()` method
4. **Updated serialization**: Assets section is included in TOML output
5. **Updated reading logic**: Automatic migration when reading lockfiles
6. **Updated tool extraction**: Asset info is moved to assets section during lockfile updates
7. **Backward compatibility**: Legacy fields are populated from assets when returning locked versions
8. **Added helper methods**: Convenient access to asset information

### Migration Process

1. **Reading**: When a lockfile is read, any legacy checksums/sizes are automatically migrated to the assets section
2. **Writing**: New lockfiles are written with the assets section, legacy fields are not serialized
3. **Updating**: When updating lockfiles, asset information is extracted and stored in the assets section
4. **Retrieval**: When getting locked versions, asset information is populated from the assets section for backward compatibility

## Benefits

1. **Reduced duplication**: Filenames are no longer repeated across tools
2. **Centralized metadata**: All asset information is in one place
3. **Extended information**: URLs can now be stored alongside checksums and sizes
4. **Backward compatibility**: Existing lockfiles continue to work seamlessly
5. **Cleaner format**: More organized and maintainable structure

## Testing

The implementation includes comprehensive migration testing that verifies:
- Legacy format is properly migrated
- Asset information is correctly consolidated
- Helper methods work as expected
- Legacy fields are cleared after migration
- New asset information (including URLs) can be added

## Future Improvements

1. **Asset deduplication**: Could detect and merge duplicate assets with same checksums
2. **Asset validation**: Could validate that assets match their checksums
3. **Asset cleanup**: Could remove unused assets from the assets section
4. **Asset compression**: Could compress or reference assets more efficiently