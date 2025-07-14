# Documentation Updates Summary

## Overview

This document summarizes the documentation updates made to reflect the new consolidated assets format in mise.lock files, where assets are nested under individual tools.

## Updated Files

### 1. `/docs/dev-tools/mise-lock.md` - **Major Updates**

**Changes:**
- Updated file format examples to show the new consolidated `[tools.name.assets]` sections
- Added explanation of asset information fields (checksum, size, url)
- Updated backend support matrix to reflect new capabilities
- Added section about legacy format migration
- Added benefits section explaining advantages of the new format
- Updated all code examples to use the new nested format

**Key Updates:**
- **File Format**: Changed from separate `[tools.name.checksums]` and `[tools.name.sizes]` to consolidated `[tools.name.assets]` sections
- **Asset Information**: Documented checksum, size, and URL fields
- **Migration**: Explained automatic migration from legacy format
- **Benefits**: Added section highlighting advantages of consolidation under each tool

### 2. `/docs/tips-and-tricks.md` - **Minor Updates**

**Changes:**
- Added explanation of consolidated format with nested assets sections
- Mentioned automatic migration from legacy format
- Added information about new metadata fields (size, URL)

### 3. `/SECURITY.md` - **Minor Updates**

**Changes:**
- Added explanation that lockfile uses consolidated format with nested assets
- Mentioned that tool assets sections store checksums, sizes, and URLs
- Emphasized improved maintainability and organization

### 4. E2E Test Updates

**Created New Tests:**
- `/e2e/lockfile/test_lockfile_migration` - Tests legacy format migration to nested assets
- `/e2e/lockfile/test_lockfile_assets` - Tests new consolidated nested format

**Updated Existing Tests:**
- `/e2e/lockfile/test_lockfile_install` - Updated to expect new format with `[tools.gh.assets]` section instead of `[tools.gh.checksums]`

## Key Documentation Changes

### File Format Examples

**Before:**
```toml
[tools.node]
version = "20.11.0"
backend = "core:node"

[tools.node.checksums]
"node-v20.11.0-linux-x64.tar.xz" = "sha256:abc123..."

[tools.node.sizes]
"node-v20.11.0-linux-x64.tar.xz" = 23456789
```

**After:**
```toml
[tools.node]
version = "20.11.0"
backend = "core:node"

[tools.node.assets]
"node-v20.11.0-linux-x64.tar.xz" = { 
  checksum = "sha256:a6c213b7a2c3b8b9c0aaf8d7f5b3a5c8d4e2f4a5b6c7d8e9f0a1b2c3d4e5f6a7", 
  size = 23456789,
  url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz"
}
```

### Backend Support Matrix

**Updated classifications:**
- ✅ **Full support** (version + checksum + size): `http`, `github`, `gitlab`, `ubi`
- ⚠️ **Partial support** (version + checksum): `aqua`, `core` (some tools)
- 📝 **Version only**: `asdf`, `npm`, `cargo`, `pipx`

### New Sections Added

1. **Asset Information** - Explains checksum, size, and URL fields in tool assets sections
2. **Legacy Format Migration** - Explains automatic migration process to nested format
3. **Benefits of the New Format** - Lists advantages of nested consolidation

## Files Not Changed

These files mentioned mise.lock but didn't need updates:
- `/docs/walkthrough.md` - Generic mention of lockfile updates
- `/docs/continuous-integration.md` - Cache configuration (format-agnostic)
- `/docs/cli/use.md` - Generic recommendation to use lockfiles
- `/docs/cli/upgrade.md` - Generic mention of lockfile updates
- `/docs/plugin-usage.md` - Generic mention of version pinning

## Test Coverage

**New Tests:**
1. **Migration Test** - Verifies legacy format is automatically migrated to nested assets
2. **Nested Assets Test** - Verifies new consolidated nested format works correctly

**Updated Tests:**
1. **Install Test** - Updated to expect new format in checksum validation with tool-specific assets

## Benefits Highlighted

The documentation now emphasizes these key benefits:

1. **Organized Structure** - Asset information is logically grouped under each tool
2. **Reduced Duplication** - Checksums and sizes consolidated per tool
3. **Extended Metadata** - Support for sizes and URLs
4. **Better Navigation** - Tool-specific assets are easier to locate
5. **Easier Maintenance** - Cleaner separation of tool info and assets

## Migration Story

The documentation clearly explains that:
- Legacy lockfiles are automatically migrated to nested assets format
- Migration is seamless and maintains functionality
- No manual intervention required
- Both formats work during transition period
- Tool-specific assets are easier to manage

This comprehensive documentation update ensures users understand the new nested format while maintaining confidence in the migration process. The nested approach provides better organization compared to the previous separate checksums and sizes sections.