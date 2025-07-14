# Lockfile Consolidation: Summary of Changes

## Overview

This document summarizes the changes made to consolidate checksums and sizes in the `mise.lock` file to reduce duplication by introducing consolidated `assets` sections under each tool.

## Problem

The original `mise.lock` format had separate sections for checksums and sizes:

```toml
[tools.actionlint]
version = "1.7.7"
backend = "aqua:rhysd/actionlint"

[tools.actionlint.checksums]
"actionlint_1.7.7_linux_amd64.tar.gz" = "sha256:023070a287cd8cccd71515fedc843f1985bf96c436b7effaecce67290e7e0757"

[tools.actionlint.sizes]
"actionlint_1.7.7_linux_amd64.tar.gz" = 4567890

[tools.bun]
version = "1.2.18"
backend = "core:bun"

[tools.bun.checksums]
"bun-linux-x64-baseline.zip" = "sha256:c7504c216d8729105d1781385665e3ac2c63debdbb5de4efdc12e2d6c4a6cb4a"

[tools.bun.sizes]
"bun-linux-x64-baseline.zip" = 38942100
```

This caused duplication of filenames and made the lockfile verbose.

## Solution

### New Format

The new format consolidates all asset information into `[tools.name.assets]` sections:

```toml
[tools.actionlint]
version = "1.7.7"
backend = "aqua:rhysd/actionlint"

[tools.actionlint.assets]
"actionlint_1.7.7_linux_amd64.tar.gz" = { checksum = "sha256:023070a287cd8cccd71515fedc843f1985bf96c436b7effaecce67290e7e0757", size = 4567890 }

[tools.bun]
version = "1.2.18"
backend = "core:bun"

[tools.bun.assets]
"bun-linux-x64-baseline.zip" = { checksum = "sha256:c7504c216d8729105d1781385665e3ac2c63debdbb5de4efdc12e2d6c4a6cb4a", size = 38942100 }
```

### Asset Structure

Each asset can now include:
- `checksum`: Optional SHA256/Blake3 checksum
- `size`: Optional file size in bytes
- `url`: Optional download URL

Example with all fields:
```toml
[tools.node.assets]
"node-v20.11.0-linux-x64.tar.xz" = { 
  checksum = "sha256:abc123", 
  size = 23456789, 
  url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz" 
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

### 2. Updated Lockfile Tool Structure

Modified the `LockfileTool` struct to include an `assets` field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfileTool {
    pub version: String,
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<String, AssetInfo>,
    // Legacy fields for migration compatibility
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub checksums: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sizes: BTreeMap<String, u64>,
}
```

### 3. Migration Logic

Implemented automatic migration from the legacy format:

```rust
fn migrate_legacy_format(&mut self) {
    for (_tool_name, versions) in &mut self.tools {
        for version in versions {
            // Migrate checksums and sizes to assets section
            let checksums_to_migrate: Vec<(String, String)> = version.checksums.clone().into_iter().collect();
            let sizes_to_migrate: Vec<(String, u64)> = version.sizes.clone().into_iter().collect();
            
            version.checksums.clear();
            version.sizes.clear();
            
            // Combine checksums and sizes into assets
            for (filename, checksum) in checksums_to_migrate {
                let asset = version.assets.entry(filename).or_insert_with(|| AssetInfo {
                    checksum: None,
                    size: None,
                    url: None,
                });
                asset.checksum = Some(checksum);
            }
            
            for (filename, size) in sizes_to_migrate {
                let asset = version.assets.entry(filename).or_insert_with(|| AssetInfo {
                    checksum: None,
                    size: None,
                    url: None,
                });
                asset.size = Some(size);
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
pub fn get_checksum(&self, tool_name: &str, filename: &str) -> Option<&String>
pub fn get_size(&self, tool_name: &str, filename: &str) -> Option<u64>
pub fn get_url(&self, tool_name: &str, filename: &str) -> Option<&String>
```

## Key Changes by File

### `src/lockfile.rs`

1. **Added new structures**: `AssetInfo` struct for asset metadata
2. **Updated `LockfileTool` struct**: Added `assets` field
3. **Added migration logic**: `migrate_legacy_format()` method
4. **Updated serialization**: Assets sections are included in TOML output
5. **Updated reading logic**: Automatic migration when reading lockfiles
6. **Updated tool conversion**: Asset info is consolidated during lockfile creation
7. **Backward compatibility**: Legacy fields are populated from assets when returning locked versions
8. **Added helper methods**: Tool-specific access to asset information

### Migration Process

1. **Reading**: When a lockfile is read, any legacy checksums/sizes are automatically migrated to the tool's assets section
2. **Writing**: New lockfiles are written with the assets sections under each tool, legacy fields are not serialized
3. **Updating**: When updating lockfiles, asset information is consolidated into the tool's assets section
4. **Retrieval**: When getting locked versions, asset information is populated from the tool's assets section for backward compatibility

## Benefits

1. **Organized Structure**: Asset information is logically grouped under each tool
2. **Reduced Duplication**: Checksums and sizes are consolidated in a single section per tool
3. **Extended Information**: URLs can now be stored alongside checksums and sizes
4. **Backward Compatibility**: Existing lockfiles continue to work seamlessly
5. **Better Navigation**: Tool-specific assets are easier to locate and manage
6. **Cleaner Format**: More organized and maintainable structure

## Testing

The implementation includes comprehensive migration testing that verifies:
- Legacy format is properly migrated to nested assets
- Asset information is correctly consolidated under each tool
- Helper methods work as expected with tool-specific access
- Legacy fields are cleared after migration
- New asset information (including URLs) can be added per tool

## Future Improvements

1. **Asset deduplication**: Could detect and merge duplicate assets with same checksums across tools
2. **Asset validation**: Could validate that assets match their checksums
3. **Asset cleanup**: Could remove unused assets from tool assets sections
4. **Asset compression**: Could optimize storage of asset information