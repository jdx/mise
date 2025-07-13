use std::collections::BTreeMap;

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

/// Looks up a value in a BTreeMap using all possible platform key aliases.
/// Example: for key_type = "url", will check platform_macos_x64_url, platform_darwin_amd64_url, etc.
/// Also supports both "platforms_" and "platform_" prefixes.
pub fn lookup_platform_key<'a>(
    opts: &'a BTreeMap<String, String>,
    key_type: &str,
) -> Option<&'a String> {
    for (os, arch) in platform_aliases() {
        for prefix in ["platforms", "platform"] {
            if let Some(val) = opts.get(&format!("{prefix}_{os}_{arch}_{key_type}")) {
                return Some(val);
            }
        }
    }
    None
}
