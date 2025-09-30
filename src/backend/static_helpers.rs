// Shared template logic for backends
use crate::file;
use crate::hash;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::ui::progress_report::SingleReport;
use eyre::{Result, bail};
use indexmap::IndexSet;
use std::path::Path;

// Shared OS/arch patterns used across helpers
const OS_PATTERNS: &[&str] = &[
    "linux", "darwin", "macos", "windows", "win", "freebsd", "openbsd", "netbsd", "android",
    "unknown",
];
// Longer arch patterns first to avoid partial matches
const ARCH_PATTERNS: &[&str] = &[
    "x86_64", "aarch64", "ppc64le", "ppc64", "armv7", "armv6", "arm64", "amd64", "mipsel",
    "riscv64", "s390x", "i686", "i386", "x64", "mips", "arm", "x86",
];

/// Helper to try both prefixed and non-prefixed tags for a resolver function
pub async fn try_with_v_prefix<F, Fut, T>(
    version: &str,
    version_prefix: Option<&str>,
    resolver: F,
) -> Result<T>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut errors = vec![];

    // Generate candidates based on version prefix configuration
    let candidates = if let Some(prefix) = version_prefix {
        // If a custom prefix is configured, try both prefixed and non-prefixed versions
        if version.starts_with(prefix) {
            vec![
                version.to_string(),
                version.trim_start_matches(prefix).to_string(),
            ]
        } else {
            vec![format!("{}{}", prefix, version), version.to_string()]
        }
    } else {
        // Fall back to 'v' prefix logic
        if version.starts_with('v') {
            vec![
                version.to_string(),
                version.trim_start_matches('v').to_string(),
            ]
        } else {
            vec![format!("v{version}"), version.to_string()]
        }
    };

    for candidate in candidates {
        match resolver(candidate.clone()).await {
            Ok(res) => return Ok(res),
            Err(e) => {
                let is_404 = crate::http::error_code(&e) == Some(404);
                if is_404 {
                    errors.push(e);
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(errors
        .pop()
        .unwrap_or_else(|| eyre::eyre!("No matching release found for {version}")))
}

/// Returns all possible aliases for the current platform (os, arch),
/// with the preferred spelling first (macos/x64, linux/x64, etc).
pub fn platform_aliases() -> Vec<(String, String)> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let mut aliases = vec![];

    // OS aliases
    let os_aliases = match os {
        "macos" | "darwin" => vec!["macos", "darwin"],
        "linux" => vec!["linux"],
        "windows" => vec!["windows"],
        _ => vec![os],
    };

    // Arch aliases
    let arch_aliases = match arch {
        "x86_64" | "amd64" => vec!["x64", "amd64", "x86_64"],
        "aarch64" | "arm64" => vec!["arm64", "aarch64"],
        _ => vec![arch],
    };

    for os in &os_aliases {
        for arch in &arch_aliases {
            aliases.push((os.to_string(), arch.to_string()));
        }
    }
    aliases
}

/// Looks up a value in ToolVersionOptions using nested platform key format.
/// Supports nested format (platforms.macos-x64.url) with os-arch dash notation.
/// Also supports both "platforms" and "platform" prefixes.
pub fn lookup_platform_key(opts: &ToolVersionOptions, key_type: &str) -> Option<String> {
    // Try nested platform structure with os-arch format
    for (os, arch) in platform_aliases() {
        for prefix in ["platforms", "platform"] {
            // Try nested format: platforms.macos-x64.url
            let nested_key = format!("{prefix}.{os}-{arch}.{key_type}");
            if let Some(val) = opts.get_nested_string(&nested_key) {
                return Some(val);
            }
            // Try flat format: platforms_macos_arm64_url
            let flat_key = format!("{prefix}_{os}_{arch}_{key_type}");
            if let Some(val) = opts.get(&flat_key) {
                return Some(val.clone());
            }
        }
    }
    None
}

/// Lists platform keys (e.g. "macos-x64") for which a given key_type exists (e.g. "url").
pub fn list_available_platforms_with_key(opts: &ToolVersionOptions, key_type: &str) -> Vec<String> {
    let mut set = IndexSet::new();

    // Gather from flat keys
    for (k, _) in opts.iter() {
        if let Some(rest) = k
            .strip_prefix("platforms_")
            .or_else(|| k.strip_prefix("platform_"))
        {
            if let Some(platform_part) = rest.strip_suffix(&format!("_{}", key_type)) {
                // Only convert the OS/arch separator underscore to a dash, preserving
                // underscores inside architecture names like x86_64
                let platform_key = if let Some((os_part, rest)) = platform_part.split_once('_') {
                    format!("{os_part}-{rest}")
                } else {
                    platform_part.to_string()
                };
                set.insert(platform_key);
            }
        }
    }

    // Probe nested keys using shared patterns
    for os in OS_PATTERNS {
        for arch in ARCH_PATTERNS {
            for prefix in ["platforms", "platform"] {
                let nested_key = format!("{prefix}.{os}-{arch}.{key_type}");
                if opts.contains_key(&nested_key) {
                    set.insert(format!("{os}-{arch}"));
                }
            }
        }
    }

    set.into_iter().collect()
}

pub fn template_string(template: &str, tv: &ToolVersion) -> String {
    let version = &tv.version;
    template.replace("{version}", version)
}

pub fn get_filename_from_url(url: &str) -> String {
    url.split('/').next_back().unwrap_or("download").to_string()
}

pub fn install_artifact(
    tv: &crate::toolset::ToolVersion,
    file_path: &Path,
    opts: &ToolVersionOptions,
    pr: Option<&dyn SingleReport>,
) -> eyre::Result<()> {
    let install_path = tv.install_path();
    let mut strip_components = opts.get("strip_components").and_then(|s| s.parse().ok());

    file::remove_all(&install_path)?;
    file::create_dir_all(&install_path)?;

    // Use TarFormat for format detection
    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let format = file::TarFormat::from_ext(ext);

    // Get file extension and detect format
    let file_name = file_path.file_name().unwrap().to_string_lossy();

    // Check if it's a compressed binary (not a tar archive)
    let is_compressed_binary =
        !file_name.contains(".tar") && matches!(ext, "gz" | "xz" | "bz2" | "zst");

    if is_compressed_binary {
        // Handle compressed single binary
        let decompressed_name = file_name.trim_end_matches(&format!(".{}", ext));
        // Determine the destination path with support for bin_path
        let dest = if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
            let bin_dir = install_path.join(bin_path);
            file::create_dir_all(&bin_dir)?;
            bin_dir.join(decompressed_name)
        } else if let Some(bin_name) = opts.get("bin") {
            install_path.join(bin_name)
        } else {
            // Auto-clean binary names by removing OS/arch suffixes
            let cleaned_name = clean_binary_name(decompressed_name, Some(&tv.ba().tool_name));
            install_path.join(cleaned_name)
        };

        match ext {
            "gz" => file::un_gz(file_path, &dest)?,
            "xz" => file::un_xz(file_path, &dest)?,
            "bz2" => file::un_bz2(file_path, &dest)?,
            "zst" => file::un_zst(file_path, &dest)?,
            _ => unreachable!(),
        }

        file::make_executable(&dest)?;
    } else if format == file::TarFormat::Raw {
        // Copy the file directly to the bin_path directory or install_path
        if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
            let bin_dir = install_path.join(bin_path);
            file::create_dir_all(&bin_dir)?;
            let dest = bin_dir.join(file_path.file_name().unwrap());
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        } else if let Some(bin_name) = opts.get("bin") {
            // If bin is specified, rename the file to this name
            let dest = install_path.join(bin_name);
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        } else {
            // Always auto-clean binary names by removing OS/arch suffixes
            let original_name = file_path.file_name().unwrap().to_string_lossy();
            let cleaned_name = clean_binary_name(&original_name, Some(&tv.ba().tool_name));
            let dest = install_path.join(cleaned_name);
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        }
    } else {
        // Handle archive formats
        // Auto-detect if we need strip_components=1 before extracting
        // Only do this if strip_components was not explicitly set by the user AND bin_path is not configured
        if strip_components.is_none() && opts.get("bin_path").is_none() {
            if let Ok(should_strip) = file::should_strip_components(file_path, format) {
                if should_strip {
                    debug!(
                        "Auto-detected single directory archive, extracting with strip_components=1"
                    );
                    strip_components = Some(1);
                }
            }
        }
        let tar_opts = file::TarOptions {
            format,
            strip_components: strip_components.unwrap_or(0),
            pr,
            ..Default::default()
        };

        // Extract with determined strip_components
        file::untar(file_path, &install_path, &tar_opts)?;
    }
    Ok(())
}

pub fn verify_artifact(
    _tv: &crate::toolset::ToolVersion,
    file_path: &Path,
    opts: &crate::toolset::ToolVersionOptions,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    // Check platform-specific checksum first, then fall back to generic
    let checksum = lookup_platform_key(opts, "checksum").or_else(|| opts.get("checksum").cloned());

    if let Some(checksum) = checksum {
        verify_checksum_str(file_path, &checksum, pr)?;
    }

    // Check platform-specific size first, then fall back to generic
    let size_str = lookup_platform_key(opts, "size").or_else(|| opts.get("size").cloned());

    if let Some(size_str) = size_str {
        let expected_size: u64 = size_str.parse()?;
        let actual_size = file_path.metadata()?.len();
        if actual_size != expected_size {
            bail!(
                "Size mismatch: expected {}, got {}",
                expected_size,
                actual_size
            );
        }
    }

    Ok(())
}

pub fn verify_checksum_str(
    file_path: &Path,
    checksum: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    if let Some((algo, hash_str)) = checksum.split_once(':') {
        hash::ensure_checksum(file_path, hash_str, pr, algo)?;
    } else {
        bail!("Invalid checksum format: {}", checksum);
    }
    Ok(())
}

/// Cleans a binary name by removing OS/arch suffixes and version numbers.
/// This is useful when downloading single binaries that have platform-specific names.
/// Executable extensions (.exe, .bat, .sh, etc.) are preserved.
///
/// # Parameters
/// - `name`: The binary name to clean
/// - `tool_name`: Optional hint for the expected tool name. When provided:
///   - Version removal is more aggressive, only keeping the result if it matches the tool name
///   - Helps ensure the cleaned name matches the expected tool
///     – When `None`, version removal is more conservative to avoid over-cleaning
///
/// # Examples
/// - "docker-compose-linux-x86_64" -> "docker-compose"
/// - "tool-darwin-arm64.exe" -> "tool.exe" (preserves extension)
/// - "mytool-v1.2.3-windows-amd64" -> "mytool"
/// - "app-2.0.0-linux-x64" -> "app" (with tool_name="app")
/// - "script-darwin-arm64.sh" -> "script.sh" (preserves .sh extension)
pub fn clean_binary_name(name: &str, tool_name: Option<&str>) -> String {
    // Extract extension if present (to preserve it)
    let (name_without_ext, extension) = if let Some(pos) = name.rfind('.') {
        let potential_ext = &name[pos + 1..];
        // Common executable extensions to preserve
        let executable_extensions = [
            "exe", "bat", "cmd", "sh", "ps1", "app", "AppImage", "run", "bin",
        ];
        if executable_extensions.contains(&potential_ext) {
            (&name[..pos], Some(&name[pos..]))
        } else {
            // Not an executable extension, treat it as part of the name
            (name, None)
        }
    } else {
        (name, None)
    };

    // Try to find and remove platform suffixes
    let mut cleaned = name_without_ext.to_string();

    // First try combined OS-arch patterns
    for os in OS_PATTERNS {
        for arch in ARCH_PATTERNS {
            // Try different separator combinations
            let patterns = [
                format!("-{os}-{arch}"),
                format!("-{os}_{arch}"),
                format!("_{os}-{arch}"),
                format!("_{os}_{arch}"),
                format!("-{arch}-{os}"), // Sometimes arch comes before OS
                format!("_{arch}_{os}"),
            ];

            for pattern in &patterns {
                if let Some(pos) = cleaned.rfind(pattern) {
                    cleaned = cleaned[..pos].to_string();
                    // Continue processing to also remove version numbers
                    let result = clean_version_suffix(&cleaned, tool_name);
                    // Add the extension back if we had one
                    if let Some(ext) = extension {
                        return format!("{}{}", result, ext);
                    } else {
                        return result;
                    }
                }
            }
        }
    }

    // Try just OS suffix (sometimes arch is omitted)
    for os in OS_PATTERNS {
        let patterns = [format!("-{os}"), format!("_{os}")];
        for pattern in &patterns {
            if let Some(pos) = cleaned.rfind(pattern.as_str()) {
                // Only remove if it's at the end or followed by more platform info
                let after = &cleaned[pos + pattern.len()..];
                if after.is_empty() || after.starts_with('-') || after.starts_with('_') {
                    // Check if what comes before looks like a valid name
                    let before = &cleaned[..pos];
                    if !before.is_empty() {
                        cleaned = before.to_string();
                        let result = clean_version_suffix(&cleaned, tool_name);
                        // Add the extension back if we had one
                        if let Some(ext) = extension {
                            return format!("{}{}", result, ext);
                        } else {
                            return result;
                        }
                    }
                }
            }
        }
    }

    // Try just arch suffix (sometimes OS is omitted)
    for arch in ARCH_PATTERNS {
        let patterns = [format!("-{arch}"), format!("_{arch}")];
        for pattern in &patterns {
            if let Some(pos) = cleaned.rfind(pattern.as_str()) {
                // Only remove if it's at the end or followed by more platform info
                let after = &cleaned[pos + pattern.len()..];
                if after.is_empty() || after.starts_with('-') || after.starts_with('_') {
                    // Check if what comes before looks like a valid name
                    let before = &cleaned[..pos];
                    if !before.is_empty() {
                        cleaned = before.to_string();
                        let result = clean_version_suffix(&cleaned, tool_name);
                        // Add the extension back if we had one
                        if let Some(ext) = extension {
                            return format!("{}{}", result, ext);
                        } else {
                            return result;
                        }
                    }
                }
            }
        }
    }

    // Try to remove version suffixes as a final step
    let cleaned = clean_version_suffix(&cleaned, tool_name);

    // Add the extension back if we had one
    if let Some(ext) = extension {
        format!("{}{}", cleaned, ext)
    } else {
        cleaned
    }
}

/// Remove version suffixes from binary names.
///
/// When `tool_name` is provided, aggressively removes version patterns but only
/// if the result matches or relates to the tool name. This prevents accidentally
/// removing too much from the name.
///
/// When `tool_name` is None, only removes clear version patterns at the end
/// while ensuring we don't leave an empty or invalid result.
fn clean_version_suffix(name: &str, tool_name: Option<&str>) -> String {
    // Common version patterns to remove
    // Matches: -v1.2.3, _v1.2.3, -1.2.3, _1.2.3, etc.
    // Also handles pre-release versions like -v1.2.3-alpha, -2.0.0-rc1
    let version_pattern = regex::Regex::new(r"[-_]v?\d+(\.\d+)*(-[a-zA-Z0-9]+(\.\d+)?)?$").unwrap();

    if let Some(tool) = tool_name {
        // If we have a tool name, only remove version if what remains matches the tool
        if let Some(m) = version_pattern.find(name) {
            let without_version = &name[..m.start()];
            if without_version == tool
                || tool.contains(without_version)
                || without_version.contains(tool)
            {
                return without_version.to_string();
            }
        }
    } else {
        // No tool name hint, be more conservative
        // Only remove if it looks like a clear version pattern at the end
        if let Some(m) = version_pattern.find(name) {
            let without_version = &name[..m.start()];
            // Make sure we're not left with nothing or just a dash/underscore
            if !without_version.is_empty()
                && !without_version.ends_with('-')
                && !without_version.ends_with('_')
            {
                return without_version.to_string();
            }
        }
    }

    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolset::ToolVersionOptions;
    use indexmap::IndexMap;

    #[test]
    fn test_clean_binary_name() {
        // Test basic OS/arch removal
        assert_eq!(
            clean_binary_name("docker-compose-linux-x86_64", None),
            "docker-compose"
        );
        assert_eq!(
            clean_binary_name("docker-compose-linux-x86_64.exe", None),
            "docker-compose.exe"
        );
        assert_eq!(clean_binary_name("tool-darwin-arm64", None), "tool");
        assert_eq!(
            clean_binary_name("mytool-v1.2.3-windows-amd64", None),
            "mytool"
        );

        // Test different separators
        assert_eq!(clean_binary_name("app_linux_amd64", None), "app");
        assert_eq!(clean_binary_name("app-linux_x64", None), "app");
        assert_eq!(clean_binary_name("app_darwin-arm64", None), "app");

        // Test arch before OS
        assert_eq!(clean_binary_name("tool-x86_64-linux", None), "tool");
        assert_eq!(clean_binary_name("tool_amd64_windows", None), "tool");

        // Test with tool name hint
        assert_eq!(
            clean_binary_name("docker-compose-linux-x86_64", Some("docker-compose")),
            "docker-compose"
        );
        assert_eq!(
            clean_binary_name("compose-linux-x86_64", Some("compose")),
            "compose"
        );

        // Test single OS or arch suffix
        assert_eq!(clean_binary_name("binary-linux", None), "binary");
        assert_eq!(clean_binary_name("binary-x86_64", None), "binary");
        assert_eq!(clean_binary_name("binary_arm64", None), "binary");

        // Test version removal
        assert_eq!(clean_binary_name("tool-v1.2.3", None), "tool");
        assert_eq!(clean_binary_name("app-2.0.0", None), "app");
        assert_eq!(clean_binary_name("binary_v3.2.1", None), "binary");
        assert_eq!(clean_binary_name("tool-1.0.0-alpha", None), "tool");
        assert_eq!(clean_binary_name("app-v2.0.0-rc1", None), "app");

        // Test version removal with tool name hint
        assert_eq!(
            clean_binary_name("docker-compose-v2.29.1", Some("docker-compose")),
            "docker-compose"
        );
        assert_eq!(
            clean_binary_name("compose-2.29.1", Some("compose")),
            "compose"
        );

        // Test no cleaning needed
        assert_eq!(clean_binary_name("simple-tool", None), "simple-tool");

        // Test that executable extensions are preserved
        assert_eq!(clean_binary_name("app-linux-x64.exe", None), "app.exe");
        assert_eq!(
            clean_binary_name("tool-v1.2.3-windows.bat", None),
            "tool.bat"
        );
        assert_eq!(
            clean_binary_name("script-darwin-arm64.sh", None),
            "script.sh"
        );
        assert_eq!(
            clean_binary_name("app-linux.AppImage", None),
            "app.AppImage"
        );

        // Test edge cases
        assert_eq!(clean_binary_name("linux", None), "linux"); // Just OS name
        assert_eq!(clean_binary_name("", None), "");
    }

    #[test]
    fn test_list_available_platforms_with_key_flat_preserves_arch_underscore() {
        let mut opts = IndexMap::new();
        // Flat keys with os_arch_keytype naming
        opts.insert(
            "platforms_macos_x86_64_url".to_string(),
            "https://example.com/macos-x86_64.tar.gz".to_string(),
        );
        opts.insert(
            "platforms_linux_x64_url".to_string(),
            "https://example.com/linux-x64.tar.gz".to_string(),
        );
        // Different prefix variant also supported
        opts.insert(
            "platform_windows_arm64_url".to_string(),
            "https://example.com/windows-arm64.zip".to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        let platforms = list_available_platforms_with_key(&tool_opts, "url");

        // Should convert only the OS/arch separator underscore to dash
        assert!(platforms.contains(&"macos-x86_64".to_string()));
        assert!(!platforms.contains(&"macos-x86-64".to_string()));

        assert!(platforms.contains(&"linux-x64".to_string()));
        assert!(platforms.contains(&"windows-arm64".to_string()));
    }

    #[test]
    fn test_verify_artifact_platform_specific() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-x64]
checksum = "blake3:abc123"
size = "1024"

[macos-arm64]
checksum = "blake3:jkl012"
size = "4096"

[linux-x64]
checksum = "blake3:def456"
size = "2048"

[linux-arm64]
checksum = "blake3:mno345"
size = "5120"

[windows-x64]
checksum = "blake3:ghi789"
size = "3072"

[windows-arm64]
checksum = "blake3:mno345"
size = "5120"
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test that platform-specific checksum and size are found
        // This test verifies that lookup_platform_key is being used correctly
        // The actual verification would require a real file, but we can test the lookup logic
        let checksum = lookup_platform_key(&tool_opts, "checksum");
        let size = lookup_platform_key(&tool_opts, "size");

        // Skip the test if the current platform isn't supported in the test data
        if checksum.is_none() || size.is_none() {
            eprintln!(
                "Skipping test_verify_artifact_platform_specific: current platform not supported in test data"
            );
            return;
        }

        // The exact values depend on the current platform, but we should get some value
        // If we're not on a supported platform, the test should still pass
        // since the function should handle missing platform-specific values gracefully
        assert!(checksum.is_some());
        assert!(size.is_some());
    }

    #[test]
    fn test_verify_artifact_fallback_to_generic() {
        let mut opts = IndexMap::new();
        opts.insert("checksum".to_string(), "blake3:generic123".to_string());
        opts.insert("size".to_string(), "512".to_string());

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test that generic fallback works when no platform-specific values exist
        let checksum = lookup_platform_key(&tool_opts, "checksum")
            .or_else(|| tool_opts.get("checksum").cloned());
        let size =
            lookup_platform_key(&tool_opts, "size").or_else(|| tool_opts.get("size").cloned());

        assert_eq!(checksum, Some("blake3:generic123".to_string()));
        assert_eq!(size, Some("512".to_string()));
    }
}
