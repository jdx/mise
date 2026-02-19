// Shared template logic for backends
use crate::backend::platform_target::PlatformTarget;
use crate::file;
use crate::hash;
use crate::http::HTTP;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::ui::progress_report::SingleReport;
use eyre::{Result, bail};
use indexmap::IndexSet;
use std::path::Path;
use std::sync::LazyLock;

/// Regex pattern for matching version suffixes like -v1.2.3, _1.2.3, etc.
static VERSION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[-_]v?\d+(\.\d+)*(-[a-zA-Z0-9]+(\.\d+)?)?$").unwrap());

// ========== Checksum Fetching Helpers ==========

/// Fetches a checksum for a specific file from a SHASUMS256.txt-style file.
/// Uses cached HTTP requests since the same SHASUMS file is fetched for all platforms.
///
/// # Arguments
/// * `shasums_url` - URL to the SHASUMS256.txt file
/// * `filename` - The filename to look up in the SHASUMS file
///
/// # Returns
/// * `Some("sha256:<hash>")` if found
/// * `None` if the SHASUMS file couldn't be fetched or filename not found
pub async fn fetch_checksum_from_shasums(shasums_url: &str, filename: &str) -> Option<String> {
    match HTTP.get_text_cached(shasums_url).await {
        Ok(shasums_content) => {
            let shasums = hash::parse_shasums(&shasums_content);
            shasums.get(filename).map(|h| format!("sha256:{h}"))
        }
        Err(e) => {
            debug!("Failed to fetch SHASUMS from {}: {e}", shasums_url);
            None
        }
    }
}

/// Fetches a checksum from an individual checksum file (e.g., file.tar.gz.sha256).
/// The checksum file should contain just the hash, optionally followed by filename.
///
/// # Arguments
/// * `checksum_url` - URL to the checksum file (e.g., `https://example.com/file.tar.gz.sha256`)
/// * `algo` - The algorithm name to prefix (e.g., "sha256")
///
/// # Returns
/// * `Some("<algo>:<hash>")` if found
/// * `None` if the checksum file couldn't be fetched
pub async fn fetch_checksum_from_file(checksum_url: &str, algo: &str) -> Option<String> {
    match HTTP.get_text(checksum_url).await {
        Ok(content) => {
            // Format is typically "<hash>  <filename>" or just "<hash>"
            content
                .split_whitespace()
                .next()
                .map(|h| format!("{algo}:{}", h.trim()))
        }
        Err(e) => {
            debug!("Failed to fetch checksum from {}: {e}", checksum_url);
            None
        }
    }
}

// ========== Platform Patterns ==========

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

pub trait VerifiableError: Sized + Send + Sync + 'static {
    fn is_not_found(&self) -> bool;
    fn into_eyre(self) -> eyre::Report;
}

impl VerifiableError for eyre::Report {
    fn is_not_found(&self) -> bool {
        self.chain().any(|cause| {
            if let Some(err) = cause.downcast_ref::<reqwest::Error>() {
                err.status() == Some(reqwest::StatusCode::NOT_FOUND)
            } else {
                false
            }
        })
    }

    fn into_eyre(self) -> eyre::Report {
        self
    }
}

impl VerifiableError for anyhow::Error {
    fn is_not_found(&self) -> bool {
        if self.to_string().contains("404") {
            return true;
        }
        self.chain().any(|cause| {
            if let Some(err) = cause.downcast_ref::<reqwest::Error>() {
                err.status() == Some(reqwest::StatusCode::NOT_FOUND)
            } else {
                false
            }
        })
    }

    fn into_eyre(self) -> eyre::Report {
        eyre::eyre!(self)
    }
}

/// Helper to try both prefixed and non-prefixed tags for a resolver function
pub async fn try_with_v_prefix<F, Fut, T, E>(
    version: &str,
    version_prefix: Option<&str>,
    resolver: F,
) -> Result<T>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = std::result::Result<T, E>>,
    E: VerifiableError,
{
    try_with_v_prefix_and_repo(version, version_prefix, None, resolver).await
}

/// Helper to try various tag formats for a resolver function
/// Tries version_prefix (if set), v prefix, and optionally repo@version formats
pub async fn try_with_v_prefix_and_repo<F, Fut, T, E>(
    version: &str,
    version_prefix: Option<&str>,
    repo: Option<&str>,
    resolver: F,
) -> Result<T>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = std::result::Result<T, E>>,
    E: VerifiableError,
{
    let mut errors = vec![];

    // Generate candidates based on version prefix configuration
    let mut candidates = if let Some(prefix) = version_prefix {
        // If a custom prefix is configured, try both prefixed and non-prefixed versions
        if version.starts_with(prefix) {
            vec![
                version.to_string(),
                version.trim_start_matches(prefix).to_string(),
            ]
        } else {
            vec![format!("{}{}", prefix, version), version.to_string()]
        }
    } else if version == "latest" {
        vec![version.to_string()]
    } else if version.starts_with('v') {
        vec![
            version.to_string(),
            version.trim_start_matches('v').to_string(),
        ]
    } else {
        vec![format!("v{version}"), version.to_string()]
    };

    // Also try repo@version formats (e.g., tectonic@0.15.0) when no prefix is configured
    // Try both the repo short name and full repo name
    // Skip this for "latest" since it's a special keyword, not an actual tag
    if version_prefix.is_none()
        && version != "latest"
        && let Some(full_repo) = repo
    {
        // Try short name first (more common), e.g., "tectonic@0.15.0"
        if let Some(short_name) = full_repo.split('/').next_back() {
            candidates.push(format!("{}@{}", short_name, version));
        }
        // Also try full repo name, e.g., "tectonic-typesetting/tectonic@0.15.0"
        candidates.push(format!("{}@{}", full_repo, version));
    }

    for candidate in candidates {
        match resolver(candidate).await {
            Ok(res) => return Ok(res),
            Err(e) => {
                if e.is_not_found() {
                    errors.push(e.into_eyre());
                } else {
                    return Err(e.into_eyre());
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

/// Looks up an option value with platform-specific fallback.
/// First tries platform-specific lookup, then falls back to the base key.
///
/// # Arguments
/// * `opts` - The tool version options to search
/// * `key` - The option key to look up (e.g., "bin_path", "checksum")
///
/// # Returns
/// * `Some(value)` if found in platform-specific or base options
/// * `None` if not found
pub fn lookup_with_fallback(opts: &ToolVersionOptions, key: &str) -> Option<String> {
    lookup_platform_key(opts, key).or_else(|| opts.get(key).cloned())
}

/// Returns all possible aliases for a given platform target (os, arch).
fn target_platform_aliases(target: &PlatformTarget) -> Vec<(String, String)> {
    let os = target.os_name();
    let arch = target.arch_name();
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
        "x64" | "amd64" | "x86_64" => vec!["x64", "amd64", "x86_64"],
        "arm64" | "aarch64" => vec!["arm64", "aarch64"],
        _ => vec![arch],
    };

    for os in &os_aliases {
        for arch in &arch_aliases {
            aliases.push((os.to_string(), arch.to_string()));
        }
    }
    aliases
}

/// Looks up a value in ToolVersionOptions for a specific target platform.
/// Used for cross-platform lockfile generation.
pub fn lookup_platform_key_for_target(
    opts: &ToolVersionOptions,
    key_type: &str,
    target: &PlatformTarget,
) -> Option<String> {
    // Try nested platform structure with os-arch format
    for (os, arch) in target_platform_aliases(target) {
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
            && let Some(platform_part) = rest.strip_suffix(&format!("_{}", key_type))
        {
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
    // Check for legacy {version} syntax and emit deprecation warning
    if template.contains("{version}") && !template.contains("{{version}}") {
        deprecated_at!(
            "2026.3.0",
            "2027.3.0",
            "legacy-version-template",
            "Use {{{{ version }}}} instead of {{version}} in URL templates"
        );
        // Legacy support: replace {version} placeholder
        return template.replace("{version}", &tv.version);
    }

    // Use Tera rendering for templates
    // Supports {{ version }}, {{ os() }}, {{ arch() }}, etc.
    let mut ctx = crate::tera::BASE_CONTEXT.clone();
    ctx.insert("version", &tv.version);

    match crate::tera::get_tera(None).render_str(template, &ctx) {
        Ok(rendered) => rendered,
        Err(e) => {
            warn!("Failed to render template '{}': {}", template, e);
            template.to_string()
        }
    }
}

pub fn get_filename_from_url(url_str: &str) -> String {
    let filename = if let Ok(url) = url::Url::parse(url_str) {
        // Use proper URL parsing to get the path and extract filename
        url.path_segments()
            .and_then(|mut segments| segments.next_back())
            .map(|s| s.to_string())
            .unwrap_or_else(|| url_str.to_string())
    } else {
        // Fallback to simple parsing for non-URL strings or malformed URLs
        url_str
            .split('/')
            .next_back()
            .unwrap_or(url_str)
            .to_string()
    };
    urlencoding::decode(&filename)
        .map(|s| s.to_string())
        .unwrap_or(filename)
}

pub fn install_artifact(
    tv: &crate::toolset::ToolVersion,
    file_path: &Path,
    opts: &ToolVersionOptions,
    pr: Option<&dyn SingleReport>,
) -> eyre::Result<()> {
    let install_path = tv.install_path();
    let mut strip_components = lookup_platform_key(opts, "strip_components")
        .or_else(|| opts.get("strip_components").cloned())
        .and_then(|s| s.parse().ok());

    file::remove_all(&install_path)?;
    file::create_dir_all(&install_path)?;

    // Use TarFormat for format detection
    // Check for explicit format option first, then fall back to file extension
    let ext = if let Some(format_opt) = lookup_with_fallback(opts, "format") {
        format_opt
    } else {
        file_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    };
    let format = file::TarFormat::from_ext(&ext);

    // Get file extension and detect format
    let file_name = file_path.file_name().unwrap().to_string_lossy();

    // Check if it's a compressed binary (not a tar archive)
    let is_compressed_binary =
        !file_name.contains(".tar") && matches!(ext.as_str(), "gz" | "xz" | "bz2" | "zst");

    if is_compressed_binary {
        // Handle compressed single binary
        let decompressed_name = file_name.trim_end_matches(&format!(".{}", ext));
        // Determine the destination path with support for bin_path
        let dest = if let Some(bin_path_template) = lookup_with_fallback(opts, "bin_path") {
            let bin_path = template_string(&bin_path_template, tv);
            let bin_dir = install_path.join(&bin_path);
            file::create_dir_all(&bin_dir)?;
            bin_dir.join(decompressed_name)
        } else if let Some(bin_name) = lookup_with_fallback(opts, "bin") {
            install_path.join(&bin_name)
        } else {
            // Auto-clean binary names by removing OS/arch suffixes
            let cleaned_name = clean_binary_name(decompressed_name, Some(&tv.ba().tool_name));
            install_path.join(cleaned_name)
        };

        match ext.as_str() {
            "gz" => file::un_gz(file_path, &dest)?,
            "xz" => file::un_xz(file_path, &dest)?,
            "bz2" => file::un_bz2(file_path, &dest)?,
            "zst" => file::un_zst(file_path, &dest)?,
            _ => unreachable!(),
        }

        file::make_executable(&dest)?;
    } else if format == file::TarFormat::Raw {
        // Copy the file directly to the bin_path directory or install_path
        if let Some(bin_path_template) = lookup_with_fallback(opts, "bin_path") {
            let bin_path = template_string(&bin_path_template, tv);
            let bin_dir = install_path.join(&bin_path);
            file::create_dir_all(&bin_dir)?;
            let dest = bin_dir.join(file_path.file_name().unwrap());
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        } else if let Some(bin_name) = lookup_with_fallback(opts, "bin") {
            // If bin is specified, rename the file to this name
            let dest = install_path.join(&bin_name);
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
        if strip_components.is_none()
            && lookup_platform_key(opts, "bin_path")
                .or_else(|| opts.get("bin_path").cloned())
                .is_none()
            && let Ok(should_strip) = file::should_strip_components(file_path, format)
            && should_strip
        {
            debug!("Auto-detected single directory archive, extracting with strip_components=1");
            strip_components = Some(1);
        }
        let tar_opts = file::TarOptions {
            format,
            strip_components: strip_components.unwrap_or(0),
            pr,
            ..Default::default()
        };

        // Extract with determined strip_components
        file::untar(file_path, &install_path, &tar_opts)?;

        // Extract just the repo name from tool_name (e.g., "opsgenie/opsgenie-lamp" -> "opsgenie-lamp")
        // This is needed for matching binary names in ZIP archives where exec bits are lost
        let full_tool_name = tv.ba().tool_name.as_str();
        let tool_name = full_tool_name.rsplit('/').next().unwrap_or(full_tool_name);

        // Determine search directory based on bin_path option (used by both bin= and rename_exe=)
        let search_dir = if let Some(bin_path_template) = lookup_with_fallback(opts, "bin_path") {
            let bin_path = template_string(&bin_path_template, tv);
            install_path.join(&bin_path)
        } else {
            install_path.clone()
        };

        // Handle bin= option for archives (renames executable to specified name)
        if let Some(bin_name) = lookup_with_fallback(opts, "bin") {
            rename_executable_in_dir(&search_dir, &bin_name, Some(tool_name))?;
        }

        // Handle rename_exe option for archives
        if let Some(rename_to) = lookup_with_fallback(opts, "rename_exe") {
            rename_executable_in_dir(&search_dir, &rename_to, Some(tool_name))?;
        }
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
    let checksum = lookup_with_fallback(opts, "checksum");

    if let Some(checksum) = checksum {
        verify_checksum_str(file_path, &checksum, pr)?;
    }

    // Check platform-specific size first, then fall back to generic
    let size_str = lookup_with_fallback(opts, "size");

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

/// File extensions that indicate non-binary files.
const SKIP_EXTENSIONS: &[&str] = &[".txt", ".md", ".json", ".yml", ".yaml"];

/// File names (case-insensitive) that should be skipped when looking for executables.
const SKIP_FILE_NAMES: &[&str] = &["LICENSE", "README"];

/// Checks if a file should be skipped when searching for executables.
///
/// # Arguments
/// * `file_name` - The file name to check
/// * `strict` - If true, also checks against SKIP_FILE_NAMES and README.* patterns
///
/// # Returns
/// * `true` if the file should be skipped (not a binary)
/// * `false` if the file might be a binary
fn should_skip_file(file_name: &str, strict: bool) -> bool {
    // Skip hidden files
    if file_name.starts_with('.') {
        return true;
    }

    // Skip known non-binary extensions
    if SKIP_EXTENSIONS.iter().any(|ext| file_name.ends_with(ext)) {
        return true;
    }

    // In strict mode, also skip LICENSE/README files
    if strict {
        let upper = file_name.to_uppercase();
        if SKIP_FILE_NAMES.iter().any(|name| upper == *name) || upper.starts_with("README.") {
            return true;
        }
    }

    false
}

/// Renames the first executable file found in a directory to a new name.
/// Used by the `rename_exe` and `bin` options to rename binaries after archive extraction.
///
/// # Parameters
/// - `dir`: The directory to search for executables
/// - `new_name`: The new name for the executable
/// - `tool_name`: Optional hint for finding non-executable files by name matching.
///   When provided, if no executable is found, will search for files matching the tool name
///   and make them executable before renaming.
pub fn rename_executable_in_dir(
    dir: &Path,
    new_name: &str,
    tool_name: Option<&str>,
) -> eyre::Result<()> {
    let target_path = dir.join(new_name);

    // Check if target already exists before iterating
    // (read_dir order is non-deterministic, so we must check first)
    if target_path.is_file() && file::is_executable(&target_path) {
        return Ok(());
    }

    // Check for stripped macOS .app bundle (Contents/MacOS at root level)
    // This happens when auto-strip removes the .app wrapper directory
    let contents_macos = dir.join("Contents").join("MacOS");
    if contents_macos.is_dir() {
        // Rename within Contents/MacOS instead of moving to root
        let target_in_macos = contents_macos.join(new_name);
        if rename_executable_in_app_bundle(&contents_macos, &target_in_macos, tool_name)? {
            return Ok(());
        }
    }

    // Check for macOS .app bundles and look inside Contents/MacOS/
    for entry in file::ls(dir)? {
        if entry.is_dir() {
            let dir_name = entry.file_name().unwrap().to_string_lossy();
            if dir_name.ends_with(".app") {
                let macos_dir = entry.join("Contents").join("MacOS");
                if macos_dir.is_dir() {
                    // Try to rename executable inside the .app bundle
                    if rename_executable_in_app_bundle(&macos_dir, &target_path, tool_name)? {
                        return Ok(());
                    }
                }
            }
        }
    }

    // First pass: Find executables in the directory (non-recursive for top level)
    for path in file::ls(dir)? {
        if path.is_file() && file::is_executable(&path) {
            let file_name = path.file_name().unwrap().to_string_lossy();
            if should_skip_file(&file_name, false) {
                continue;
            }
            file::rename(&path, &target_path)?;
            debug!("Renamed {} to {}", path.display(), target_path.display());
            return Ok(());
        }
    }

    // Second pass: Find non-executable files by name matching (for ZIP archives without exec bit)
    if let Some(tool_name) = tool_name {
        for path in file::ls(dir)? {
            if path.is_file() {
                let file_name = path.file_name().unwrap().to_string_lossy();
                if should_skip_file(&file_name, true) {
                    continue;
                }

                // Check if filename matches tool name pattern or the target name
                if file_name.contains(tool_name) || *file_name == *new_name {
                    file::make_executable(&path)?;
                    file::rename(&path, &target_path)?;
                    debug!(
                        "Found and renamed {} to {} (added exec permissions)",
                        path.display(),
                        target_path.display()
                    );
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}

/// Helper function to rename executable inside a macOS .app bundle
fn rename_executable_in_app_bundle(
    macos_dir: &Path,
    target_path: &Path,
    tool_name: Option<&str>,
) -> eyre::Result<bool> {
    // Find the first executable in the Contents/MacOS directory
    for path in file::ls(macos_dir)? {
        if path.is_file() && file::is_executable(&path) {
            let file_name = path.file_name().unwrap().to_string_lossy();
            if should_skip_file(&file_name, false) {
                continue;
            }
            file::rename(&path, target_path)?;
            debug!(
                "Renamed .app bundle executable {} to {}",
                path.display(),
                target_path.display()
            );
            return Ok(true);
        }
    }

    // If no executable found, try matching by tool name
    if let Some(tool_name) = tool_name {
        for path in file::ls(macos_dir)? {
            if path.is_file() {
                let file_name = path.file_name().unwrap().to_string_lossy();
                if should_skip_file(&file_name, true) {
                    continue;
                }
                if file_name.to_lowercase().contains(&tool_name.to_lowercase()) {
                    file::make_executable(&path)?;
                    file::rename(&path, target_path)?;
                    debug!(
                        "Found and renamed .app bundle file {} to {} (added exec permissions)",
                        path.display(),
                        target_path.display()
                    );
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
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

    // Helper to add extension back to a cleaned name
    let with_ext = |s: String| -> String {
        match extension {
            Some(ext) => format!("{}{}", s, ext),
            None => s,
        }
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
                    return with_ext(result);
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
                        return with_ext(result);
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
                        return with_ext(result);
                    }
                }
            }
        }
    }

    // Try to remove version suffixes as a final step
    let cleaned = clean_version_suffix(&cleaned, tool_name);

    // Add the extension back if we had one
    with_ext(cleaned)
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
    if let Some(tool) = tool_name {
        // If we have a tool name, only remove version if what remains matches the tool
        if let Some(m) = VERSION_PATTERN.find(name) {
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
        if let Some(m) = VERSION_PATTERN.find(name) {
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
        let size = lookup_with_fallback(&tool_opts, "size");

        assert_eq!(checksum, Some("blake3:generic123".to_string()));
        assert_eq!(size, Some("512".to_string()));
    }

    #[test]
    fn test_lookup_platform_key_bin_path() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platform".to_string(),
            r#"
[macos-arm64]
bin_path = "CMake.app/Contents/bin"

[linux-x64]
bin_path = "bin"

[windows-x64]
bin_path = "."
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test that platform-specific bin_path is found
        let bin_path = lookup_platform_key(&tool_opts, "bin_path");

        // The exact value depends on the current platform
        if let Some(bp) = bin_path {
            // Should be one of the platform-specific values
            assert!(
                bp == "CMake.app/Contents/bin" || bp == "bin" || bp == ".",
                "Expected platform-specific bin_path, got: {}",
                bp
            );
        }
    }

    #[test]
    fn test_lookup_platform_key_bin() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-arm64]
bin = "xmake"

[linux-x64]
bin = "xmake"

[windows-x64]
bin = "xmake.exe"
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test that platform-specific bin is found
        let bin = lookup_platform_key(&tool_opts, "bin");

        // The exact value depends on the current platform
        if let Some(b) = bin {
            // Should be one of the platform-specific values
            assert!(
                b == "xmake" || b == "xmake.exe",
                "Expected platform-specific bin, got: {}",
                b
            );
        }
    }

    #[test]
    fn test_lookup_platform_key_bin_with_fallback() {
        let mut opts = IndexMap::new();
        opts.insert("bin".to_string(), "generic-tool".to_string());
        opts.insert(
            "platforms".to_string(),
            r#"
[windows-x64]
bin = "tool.exe"
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test that platform-specific bin takes precedence, or falls back to generic
        let bin = lookup_with_fallback(&tool_opts, "bin");

        assert!(bin.is_some());
        let bin_value = bin.unwrap();
        // On Windows x64, should get "tool.exe", otherwise "generic-tool"
        assert!(
            bin_value == "tool.exe" || bin_value == "generic-tool",
            "Expected platform-specific or generic bin, got: {}",
            bin_value
        );
    }

    #[test]
    fn test_lookup_platform_key_inline_format() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms_windows_x64_bin".to_string(),
            "xmake.exe".to_string(),
        );
        opts.insert("platforms_linux_x64_bin".to_string(), "xmake".to_string());
        opts.insert("platforms_macos_arm64_bin".to_string(), "xmake".to_string());

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test that flat platform format works
        let bin = lookup_platform_key(&tool_opts, "bin");

        if let Some(b) = bin {
            assert!(
                b == "xmake" || b == "xmake.exe",
                "Expected platform-specific bin from flat format, got: {}",
                b
            );
        }
    }
}
