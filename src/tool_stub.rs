//! Tool stub files: executable TOML files that name a tool, a version and a binary.

use std::path::Path;

use crate::backend::static_helpers::lookup_platform_key;
use crate::config::env_directive::EnvValue;
use crate::file;
use crate::hash;
use crate::toolset::{CoreToolOptions, ToolRequest, ToolSource, ToolVersionOptions};
use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Deserializer};
use toml::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct ToolStubFile {
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
    pub(crate) fn from_file(path: &Path) -> Result<Self> {
        let content = file::read_to_string(path)?;
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

    // Create a ToolRequest directly using ToolVersionOptions
    pub(crate) fn to_tool_request(&self, stub_path: &Path) -> Result<ToolRequest> {
        use crate::args::BackendArg;

        let mut backend_arg = BackendArg::from(&self.tool_name);
        let source = ToolSource::ToolStub(stub_path.to_path_buf());

        // Create ToolVersionOptions from our fields
        let mut opts = self.opts.clone();
        opts.shift_remove("tool"); // Remove tool field since it's handled separately

        // Add bin field if present
        if let Some(bin) = &self.bin {
            opts.insert("bin".to_string(), toml::Value::String(bin.clone()));
        }

        let options = ToolVersionOptions {
            core: CoreToolOptions {
                os: self.os.clone(),
                depends: None,
                install_env: self.install_env.clone(),
                ..Default::default()
            },
            opts: opts.into(),
        };

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
