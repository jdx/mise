//! Tool stub files: executable TOML files that name a tool, a version and a binary.

use std::path::Path;

use crate::backend::static_helpers::{
    list_available_platforms_with_key, lookup_platform_key, platform_download,
    platform_entry_string,
};
use crate::config::env_directive::EnvValue;
use crate::file;
use crate::hash;
use crate::toolset::{CoreToolOptions, ToolRequest, ToolSource, ToolVersionOptions};
use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Deserializer};
use toml::Value;

#[derive(Debug, Deserialize)]
pub struct ToolStubFile {
    #[serde(default = "default_version")]
    pub version: String,
    pub bin: Option<String>,  // defaults to filename if not specified
    pub tool: Option<String>, // explicit tool name override
    #[serde(default)]
    pub install_env: indexmap::IndexMap<String, EnvValue>,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(flatten, deserialize_with = "deserialize_tool_stub_options")]
    pub opts: indexmap::IndexMap<String, toml::Value>,
    #[serde(skip)]
    pub tool_name: String,
}

// Custom deserializer that keeps TOML values native, converting scalars to strings
fn deserialize_tool_stub_options<'de, D>(
    deserializer: D,
) -> Result<indexmap::IndexMap<String, toml::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let mut opts = indexmap::IndexMap::new();

    if let Value::Table(table) = value {
        for (key, val) in table {
            // Skip known special fields that are handled separately
            if matches!(
                key.as_str(),
                "version" | "bin" | "tool" | "install_env" | "os" | "lock"
            ) {
                continue;
            }

            let stored_value = match val {
                Value::String(_) | Value::Table(_) | Value::Array(_) => val,
                // Convert scalar values (ints, bools, floats) to strings
                _ => Value::String(val.to_string().trim_matches('"').to_string()),
            };

            opts.insert(key, stored_value);
        }
    }

    Ok(opts)
}

fn default_version() -> String {
    "latest".to_string()
}

fn has_http_backend_config(opts: &indexmap::IndexMap<String, toml::Value>) -> bool {
    // Check for top-level url
    if opts.contains_key("url") {
        return true;
    }

    // Check for platform-specific configs with urls
    for (key, value) in opts {
        if key.starts_with("platforms") {
            // Check if the value is a table containing url keys
            if let toml::Value::Table(table) = value {
                for (_, v) in table {
                    if let toml::Value::Table(inner) = v
                        && inner.contains_key("url")
                    {
                        return true;
                    }
                }
            } else if let toml::Value::String(s) = value
                && s.contains("url")
            {
                return true;
            }
        }
    }

    false
}

/// Whether `platform` is a plain `os-arch` key, the only kind a download's `url` and a run's
/// `bin` are looked up under (see [`ToolStubFile::bin_for_this_platform`]). A qualified key
/// such as `linux-x64-musl` is not one.
fn is_os_arch_key(platform: &str) -> bool {
    platform
        .split_once('-')
        .is_some_and(|(os, arch)| !os.is_empty() && !arch.is_empty() && !arch.contains('-'))
}

/// Extract TOML content from a bootstrap script's comment block
/// Looks for content between `# MISE_TOOL_STUB:` and `# :MISE_TOOL_STUB` markers
fn extract_toml_from_bootstrap(content: &str) -> Option<String> {
    let start_marker = "# MISE_TOOL_STUB:";
    let end_marker = "# :MISE_TOOL_STUB";

    let start_pos = content.find(start_marker)?;
    let end_pos = content.find(end_marker)?;

    if start_pos >= end_pos {
        return None;
    }

    // Extract content between markers (skip the start marker line)
    let between = &content[start_pos + start_marker.len()..end_pos];

    // Remove leading `# ` from each line to get the original TOML
    let toml = between
        .lines()
        .map(|line| line.strip_prefix("# ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");

    Some(toml.trim().to_string())
}

impl ToolStubFile {
    pub fn from_file(path: &Path) -> Result<Self> {
        Self::from_content(path, file::read_to_string(path)?)
    }

    /// The stub at `path` whose text is `content`.
    pub fn from_content(path: &Path, content: String) -> Result<Self> {
        let stub_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("Invalid stub file name"))?
            .to_string();

        // Check if this is a bootstrap script with embedded TOML
        let toml_content = if let Some(toml) = extract_toml_from_bootstrap(&content) {
            toml
        } else {
            content
        };

        let mut stub: ToolStubFile = toml::from_str(&toml_content)?;

        // Determine tool name from tool field or derive from stub name
        // If no tool is specified, default to HTTP backend if HTTP config is present
        let tool_name = stub
            .tool
            .clone()
            .or_else(|| {
                stub.opts
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| {
                if has_http_backend_config(&stub.opts) {
                    format!("http:{stub_name}")
                } else {
                    stub_name.to_string()
                }
            });

        // Set bin to filename if not specified
        if stub.bin.is_none() {
            stub.bin = Some(stub_name.to_string());
        }

        stub.tool_name = tool_name;

        Ok(stub)
    }

    /// The `bin` a run of the stub on this machine looks for: the one the install names the
    /// downloaded binary after, read with it by [`platform_download`] from the platform whose
    /// `url` is downloaded, so the two agree even when `MISE_OS` or `MISE_ARCH` names another
    /// platform, or a platform's `bin` is a flat `platforms_<os>_<arch>_bin` key.
    pub fn bin_for_this_platform(&self) -> Option<String> {
        platform_download(&self.tool_options()).bin
    }

    /// Whether every platform the stub downloads for has its own `bin`, so a run never uses the
    /// top-level `bin`, which defaults to the stub's file name. Only an `http:` stub without a
    /// top-level `url` lists the platforms it supports: any other runs on platforms it does not
    /// name. A run looks a platform's `bin` up as its download looks up the `url`, under an
    /// `os-arch` key or an alias of one, so a qualified key such as `linux-x64-musl` counts as a
    /// platform without its own `bin`.
    pub fn sets_bin_for_every_platform(&self) -> bool {
        if !self.tool_name.starts_with("http:") || self.opts.contains_key("url") {
            return false;
        }
        let opts = ToolVersionOptions {
            opts: self.opts.clone().into(),
            ..Default::default()
        };
        let platforms = list_available_platforms_with_key(&opts, "url");
        !platforms.is_empty()
            && platforms.iter().all(|platform| {
                is_os_arch_key(platform) && platform_entry_string(&opts, platform, "bin").is_some()
            })
    }

    // Create a ToolRequest directly using ToolVersionOptions
    /// Whether the stub sets tool options beyond the tool, version, binary and
    /// platforms it runs on.
    pub fn has_tool_options(&self) -> bool {
        self.opts.keys().any(|key| key != "tool") || !self.install_env.is_empty()
    }

    /// The options the stub installs its tool with.
    fn tool_options(&self) -> ToolVersionOptions {
        let mut opts = self.opts.clone();
        opts.shift_remove("tool"); // Remove tool field since it's handled separately

        // Add bin field if present
        if let Some(bin) = &self.bin {
            opts.insert("bin".to_string(), toml::Value::String(bin.clone()));
        }

        ToolVersionOptions {
            core: CoreToolOptions {
                os: self.os.clone(),
                depends: None,
                install_env: self.install_env.clone(),
                ..Default::default()
            },
            opts: opts.into(),
        }
    }

    pub fn to_tool_request(&self, stub_path: &Path) -> Result<ToolRequest> {
        use crate::args::BackendArg;

        let mut backend_arg = BackendArg::from(&self.tool_name);
        let source = ToolSource::ToolStub(stub_path.to_path_buf());
        let options = self.tool_options();

        // Set options on the BackendArg so they're available to the backend
        backend_arg.set_opts(Some(options.clone()));

        // For HTTP backend with "latest" version, use URL+checksum hash as version for stability
        let version = if self.tool_name.starts_with("http:") && self.version == "latest" {
            if let Some(url) = lookup_platform_key(&options, "url")
                .or_else(|| options.get("url").map(|s| s.to_string()))
            {
                // Include checksum in hash calculation for better version stability
                let checksum = lookup_platform_key(&options, "checksum")
                    .or_else(|| options.get("checksum").map(|s| s.to_string()))
                    .unwrap_or_default();
                let hash_input = format!("{url}:{checksum}");
                // Use first 8 chars of URL+checksum hash as version
                format!("url-{}", &hash::hash_to_str(&hash_input)[..8])
            } else {
                self.version.clone()
            }
        } else {
            self.version.clone()
        };

        ToolRequest::new_with_options(backend_arg.into(), &version, options, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub(content: &str) -> ToolStubFile {
        ToolStubFile::from_content(Path::new("/bin/stub"), content.to_string()).unwrap()
    }

    #[test]
    fn test_sets_bin_for_every_platform() {
        let every = r#"
tool = "http:t"
[platforms.linux-x64]
url = "https://example.com/l.tar.gz"
bin = "t"
[platforms.macos-arm64]
url = "https://example.com/m.tar.gz"
bin = "t"
"#;
        assert!(stub(every).sets_bin_for_every_platform());

        let missing = every.replacen("bin = \"t\"\n", "", 1);
        assert!(!stub(&missing).sets_bin_for_every_platform());

        // a top-level url serves platforms the stub does not name
        let top_level_url = format!("url = \"https://example.com/a.tar.gz\"\n{every}");
        assert!(!stub(&top_level_url).sets_bin_for_every_platform());

        // a run looks the bin up under an alias, as the download does the url
        let alias = every.replace("macos-arm64", "darwin-arm64");
        assert!(stub(&alias).sets_bin_for_every_platform());

        // but under no qualified key
        let qualified = every.replace("linux-x64", "linux-x64-musl");
        assert!(!stub(&qualified).sets_bin_for_every_platform());

        let not_http = every.replace("http:t", "github:o/t");
        assert!(!stub(&not_http).sets_bin_for_every_platform());

        assert!(!stub("tool = \"http:t\"\n").sets_bin_for_every_platform());
    }

    #[test]
    fn test_bin_for_this_platform_follows_the_download() {
        // whichever spelling of this machine's platform the url is under, the bin under the
        // same key is the one a run uses
        for (os, arch) in crate::backend::static_helpers::platform_aliases() {
            let platform = format!("{os}-{arch}");
            let stub = stub(&format!(
                "tool = \"http:t\"\n[platforms.{platform}]\nurl = \"https://example.com/t.tar.gz\"\nbin = \"{platform}-bin\"\n"
            ));
            let opts = ToolVersionOptions {
                opts: stub.opts.clone().into(),
                ..Default::default()
            };
            assert!(lookup_platform_key(&opts, "url").is_some(), "{platform}");
            assert_eq!(
                stub.bin_for_this_platform(),
                Some(format!("{platform}-bin"))
            );
        }

        // a platform that is not this machine's is never used: the top-level bin is, which
        // defaults to the stub's file name
        let other = if cfg!(target_os = "linux") {
            "windows-x64"
        } else {
            "linux-x64"
        };
        let stub = stub(&format!("[platforms.{other}]\nbin = \"other\"\n"));
        assert_eq!(stub.bin_for_this_platform().as_deref(), Some("stub"));
    }

    #[test]
    fn test_bin_for_this_platform_is_the_bin_of_the_downloaded_url() {
        let aliases = crate::backend::static_helpers::platform_aliases();
        let [(os, arch), (later_os, later_arch), ..] = aliases.as_slice() else {
            return; // a platform with one spelling has no later alias to confuse
        };
        let later = format!("{later_os}-{later_arch}");
        let installed_and_run = |content: &str| {
            let stub = stub(content);
            // the install names the downloaded binary after this bin, and a run looks for it
            let installed = platform_download(&stub.tool_options());
            assert_eq!(installed.bin, stub.bin_for_this_platform(), "{content}");
            (installed.url, stub.bin_for_this_platform())
        };

        // a flat key for the preferred spelling is the entry downloaded, so its bin is the one
        // run, not the bin of a later spelling's table
        let flat = format!(
            "tool = \"http:t\"\n\
            platforms_{os}_{arch}_url = \"https://example.com/flat.tar.gz\"\n\
            platforms_{os}_{arch}_bin = \"flat-bin\"\n\
            [platforms.{later}]\nurl = \"https://example.com/nested.tar.gz\"\nbin = \"nested-bin\"\n"
        );
        assert_eq!(
            installed_and_run(&flat),
            (
                Some("https://example.com/flat.tar.gz".to_string()),
                Some("flat-bin".to_string())
            )
        );

        // a preferred spelling that sets only a bin downloads nothing, so the bin run is the
        // one of the later spelling whose url is downloaded
        let bin_only = format!(
            "tool = \"http:t\"\n\
            [platforms.{os}-{arch}]\nbin = \"unused-bin\"\n\
            [platforms.{later}]\nurl = \"https://example.com/later.tar.gz\"\nbin = \"later-bin\"\n"
        );
        assert_eq!(
            installed_and_run(&bin_only),
            (
                Some("https://example.com/later.tar.gz".to_string()),
                Some("later-bin".to_string())
            )
        );

        // and a later spelling with a url and no bin of its own takes the top-level bin
        let no_bin = bin_only.replace("bin = \"later-bin\"\n", "");
        assert_eq!(installed_and_run(&no_bin).1.as_deref(), Some("stub"));
    }
}
