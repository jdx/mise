# Plan: Enhance `mise lock` to Update All Platforms in Lockfile

## Problem Statement

Currently, `mise` automatically updates lockfile (`mise.lock`) checksums/URLs only for the current platform when installing tools. This means that if a lockfile already contains platform-specific data for multiple platforms (e.g., `linux-x64`, `macos-arm64`, `windows-x64`), only the current platform gets updated with fresh checksums and URLs when tools are installed.

The goal is to implement a `mise lock` command that updates lockfile data for **all platforms already specified** in the lockfile, regardless of the current platform the user is running on.

## Current Architecture Analysis

### Lockfile Structure
- Lockfiles are stored in TOML format with platform-specific sections
- Structure: `[tools.{name}.platforms.{platform-key}]` where platform-key is `{os}-{arch}`
- Each platform section can contain: `checksum`, `size`, `url`

### Current Platform Handling
- Backends have a `get_platform_key()` method that returns current platform
- During installation, only current platform data is populated
- Platform keys are generated in `{os}-{arch}` format (e.g., `linux-x64`)

### Existing Functions
- `update_lockfiles()` in `src/lockfile.rs` handles lockfile updates
- `verify_checksum()` in backends handles checksum verification/generation
- Platform-specific data is stored in `ToolVersion.lock_platforms` BTreeMap

## Implementation Plan

### Phase 1: Create `mise lock` Command Infrastructure

1. **Create CLI Command** (`src/cli/lock.rs`)
   - Add new command `mise lock [TOOL]...`
   - Options:
     - `--all` or no args: Update all tools in lockfile
     - `TOOL`: Update specific tools only
     - `--platform <PLATFORMS>`: Target specific platforms (comma-separated)
     - `--force`: Re-download and update even if data exists

2. **Register Command** (`src/cli/mod.rs`)
   - Add lock module to CLI structure
   - Update help and completions

### Phase 2: Core Implementation

3. **Platform Discovery Logic**
   - Create function to extract all existing platforms from lockfile
   - Parse existing `[tools.name.platforms.*]` sections
   - Return set of platform keys already present

4. **Multi-Platform Backend Enhancement**
   - Create trait method `get_platform_key_for(os: &str, arch: &str)` in Backend trait
   - Allow backends to generate platform keys for arbitrary OS/arch combinations
   - Default implementation uses `format!("{os}-{arch}")`

5. **Multi-Platform Tool Version Resolution**
   - Enhance `ToolVersion` to support multiple target platforms
   - Add method to resolve download URLs for different platforms
   - Cache URLs/metadata per platform during resolution

### Phase 3: Backend Updates

6. **Update Core Backends**
   - **Aqua Backend**: Fetch asset URLs for all target platforms
   - **GitHub/GitLab Backends**: Resolve asset patterns for each platform
   - **HTTP Backend**: Template URLs with different platform variables
   - **Core Plugins** (Node, Java, etc.): Handle platform-specific downloads
   - **UBI Backend**: Generate checksums for each platform's assets

7. **Download and Verification Logic**
   - Create function to download assets for specific platforms
   - Generate checksums for each platform's downloads
   - Update lockfile with new platform data
   - Clean up temporary downloads

### Phase 4: Implementation Details

8. **Lockfile Update Logic** (`src/lockfile.rs`)
   ```rust
   pub async fn update_lockfile_for_platforms(
       config: &Config, 
       tools: &[String], 
       target_platforms: &[String]
   ) -> Result<()>
   ```
   - Read existing lockfiles
   - For each tool, discover existing platforms
   - For each platform, fetch fresh URLs/checksums
   - Update lockfile with new data

9. **Platform Resolution**
   - Parse platform strings like `linux-x64`, `macos-arm64`
   - Map to backend-specific platform identifiers
   - Handle backend-specific platform naming (e.g., Java's detailed platform specs)

10. **Error Handling**
    - Handle cases where assets don't exist for certain platforms
    - Graceful degradation when API calls fail
    - Preserve existing data if update fails

### Phase 5: Safety and Performance

11. **Safety Measures**
    - Validate checksums before updating lockfile
    - Backup existing lockfile before updates
    - Atomic updates to prevent corruption

12. **Performance Optimizations**
    - Parallel downloads for different platforms
    - Cache API responses within single command run
    - Skip platforms that already have recent data (unless --force)

### Phase 6: Documentation and Testing

13. **Documentation**
    - Update `docs/cli/lock.md`
    - Add examples for multi-platform scenarios
    - Document new command options

14. **Testing**
    - E2E tests for `mise lock` command
    - Test with existing multi-platform lockfiles
    - Test platform-specific URL resolution
    - Test error scenarios

## File Changes Required

### New Files
- `src/cli/lock.rs` - Main command implementation
- `docs/cli/lock.md` - Documentation
- `e2e/cli/test_lock` - E2E tests

### Modified Files
- `src/cli/mod.rs` - Register new command
- `src/lockfile.rs` - Add multi-platform update logic
- `src/backend/mod.rs` - Add platform-specific methods
- `src/backend/*.rs` - Update each backend for multi-platform support
- `src/toolset/tool_version.rs` - Add platform resolution methods
- Various backend files for platform-specific logic

## Success Criteria

1. **Functional Requirements**
   - `mise lock` updates checksums/URLs for all existing platforms in lockfile
   - Works with all supported backends (aqua, github, http, core, etc.)
   - Preserves existing platform data when updates fail
   - Supports filtering by specific tools or platforms

2. **Non-Functional Requirements**
   - Performance: Parallel processing for multiple platforms
   - Reliability: Atomic updates and rollback on failure
   - Usability: Clear error messages and progress indication
   - Maintainability: Clean separation of platform-specific logic

3. **Testing**
   - All existing lockfile tests continue to pass
   - New tests cover multi-platform scenarios
   - Edge cases (missing assets, API failures) are handled

## Implementation Order

1. Start with CLI command structure and basic lockfile reading
2. Implement platform discovery and URL resolution for one backend (e.g., GitHub)
3. Add download and checksum generation for discovered platforms
4. Extend to other backends systematically
5. Add error handling and performance optimizations
6. Complete documentation and testing

This approach allows incremental development and testing while building toward the full functionality.
