use crate::toolset::ToolVersionOptions;

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
        }
    }

    None
}
