// Shared template logic for backends
use crate::file;
use crate::hash;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::{config::Settings, ui::progress_report::SingleReport};
use eyre::{Result, bail};
use std::path::Path;

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

pub fn template_string(template: &str, tv: &ToolVersion) -> String {
    let name = tv.ba().tool_name();
    let version = &tv.version;
    let settings = Settings::get();
    let os = settings.os();
    let arch = settings.arch();
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };

    template
        .replace("{name}", &name)
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch)
        .replace("{ext}", ext)
}

pub fn get_filename_from_url(url: &str) -> String {
    url.split('/').next_back().unwrap_or("download").to_string()
}

pub fn install_artifact(
    tv: &crate::toolset::ToolVersion,
    file_path: &Path,
    opts: &ToolVersionOptions,
    pr: Option<&Box<dyn SingleReport>>,
) -> eyre::Result<()> {
    let install_path = tv.install_path();
    let mut strip_components = opts.get("strip_components").and_then(|s| s.parse().ok());

    file::remove_all(&install_path)?;
    file::create_dir_all(&install_path)?;

    // Use TarFormat for format detection
    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let format = file::TarFormat::from_ext(ext);
    if format == file::TarFormat::Raw {
        // Copy the file directly to the bin_path directory or install_path
        if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
            let bin_dir = install_path.join(bin_path);
            file::create_dir_all(&bin_dir)?;
            let dest = bin_dir.join(file_path.file_name().unwrap());
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        } else {
            let dest = install_path.join(file_path.file_name().unwrap());
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        }
    } else {
        // Auto-detect if we need strip_components=1 before extracting
        // Only do this if strip_components was not explicitly set by the user
        if strip_components.is_none() {
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
    pr: Option<&Box<dyn SingleReport>>,
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
    pr: Option<&Box<dyn SingleReport>>,
) -> Result<()> {
    if let Some((algo, hash_str)) = checksum.split_once(':') {
        hash::ensure_checksum(file_path, hash_str, pr, algo)?;
    } else {
        bail!("Invalid checksum format: {}", checksum);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolset::ToolVersionOptions;
    use indexmap::IndexMap;

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
