# PR Title

feat: consolidate lockfile assets and add URL tracking

# PR Description

## Summary

This PR consolidates checksums and sizes in `mise.lock` files into unified `assets` sections and adds URL tracking capabilities for enhanced security and traceability.

## Changes Made

### 📁 Lockfile Format Consolidation

**Before:**
```toml
[tools.node.checksums]
"node-v20.11.0-linux-x64.tar.xz" = "sha256:abc123..."

[tools.node.sizes]  
"node-v20.11.0-linux-x64.tar.xz" = 23456789
```

**After:**
```toml
[tools.node.assets]
"node-v20.11.0-linux-x64.tar.xz" = { 
  checksum = "sha256:abc123...", 
  size = 23456789,
  url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz"
}
```

### 🔗 URL Tracking Implementation

- **Aqua Backend**: Captures URLs from aqua registry configuration
- **HTTP Backend**: Stores URLs from tool configuration templates  
- **GitHub/GitLab Backend**: Records URLs from release API responses
- **UBI Backend**: Partial support (size + checksum, no URL)

### 🔄 Automatic Migration

- Legacy lockfiles are automatically migrated to the new format when read
- Seamless backward compatibility with existing workflows
- No manual intervention required from users

### 📚 Documentation & Testing

- Updated all documentation to reflect new format
- Comprehensive test coverage including migration tests
- E2E tests for URL tracking functionality
- Updated backend support matrix

## Benefits

✅ **Organized Structure**: Asset info logically grouped under each tool  
✅ **Reduced Duplication**: Single consolidated section per tool  
✅ **Full Traceability**: Complete audit trail with URLs  
✅ **Enhanced Security**: Better compliance and auditing capabilities  
✅ **Better Navigation**: Tool-specific assets easier to locate  
✅ **Migration Safe**: Automatic migration preserves existing functionality  

## Backend Support Matrix

- ✅ **Full support** (version + checksum + size + URL): `aqua`, `http`, `github`, `gitlab`
- ⚠️ **Partial support** (version + checksum + size): `ubi`  
- 📝 **Basic support** (version + checksum): `core` (some tools)
- 📝 **Version only**: `asdf`, `npm`, `cargo`, `pipx`

## Testing

- [x] Legacy format migration tests
- [x] New consolidated format tests  
- [x] URL tracking verification tests
- [x] Backend-specific asset storage tests
- [x] Documentation examples validation

## Breaking Changes

None - this is a backward-compatible enhancement with automatic migration.

## Related Issues

Addresses the need for:
- Reduced lockfile duplication
- Better asset source tracking  
- Enhanced security and compliance
- Improved lockfile maintainability