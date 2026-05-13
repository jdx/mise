//! Current installed-tool inventory reporting for mise-wings.

use std::{collections::BTreeMap, sync::Arc};

use eyre::{Context, Result, bail};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Serialize;

use crate::{
    backend::platform_target::PlatformTarget,
    config::{Config, Settings},
    file,
    toolset::{ToolVersion, ToolsetBuilder},
};

const DEVICE_ID_FILENAME: &str = "inventory-device-id";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InventorySnapshot {
    device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_label: Option<String>,
    os: String,
    arch: String,
    tools: Vec<InventoryTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InventoryTool {
    backend: String,
    tool: String,
    version: String,
    platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InventorySummary {
    pub(crate) device_id: String,
    pub(crate) tools_count: usize,
}

pub(crate) async fn submit_current_snapshot(config: &Arc<Config>) -> Result<InventorySummary> {
    if !Settings::get().wings.enabled {
        bail!("wings inventory requires wings.enabled = true");
    }
    let Some(token) = crate::wings::auth::session_token_for_cli().await? else {
        bail!("wings authentication is not available; run `mise wings login`");
    };

    let snapshot = current_snapshot(config).await?;
    let url = format!("https://api.{}/v1/wings/inventory", crate::wings::host());
    let response = crate::wings::client::http_client()?
        .post(&url)
        .headers(bearer_headers(&token)?)
        .json(&snapshot)
        .send()
        .await
        .wrap_err_with(|| format!("POST {url}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("wings inventory upload returned {status}: {body}");
    }

    Ok(InventorySummary {
        device_id: snapshot.device_id,
        tools_count: snapshot.tools.len(),
    })
}

async fn current_snapshot(config: &Arc<Config>) -> Result<InventorySnapshot> {
    let target = PlatformTarget::from_current();
    let toolset = ToolsetBuilder::new().build(config).await?;
    let mut tools = BTreeMap::new();
    for (backend, tv) in toolset.list_installed_versions(config).await? {
        let tool = inventory_tool(
            backend.get_type().to_string(),
            backend.tool_name(),
            &tv,
            &target,
        );
        tools.insert(
            (
                tool.backend.clone(),
                tool.tool.clone(),
                tool.version.clone(),
                tool.platform.clone(),
            ),
            tool,
        );
    }

    Ok(InventorySnapshot {
        device_id: device_id()?,
        device_label: None,
        os: target.os_name().to_string(),
        arch: target.arch_name().to_string(),
        tools: tools.into_values().collect(),
    })
}

fn inventory_tool(
    backend: String,
    tool: String,
    tv: &ToolVersion,
    target: &PlatformTarget,
) -> InventoryTool {
    let platform = target.to_key();
    let platform_info = tv.lock_platforms.get(&platform);
    let installed_artifact = crate::wings::artifact::installed_artifact(tv)
        .map_err(|e| {
            warn!(
                "Error loading wings install marker for {}: {e:#}",
                tv.style()
            )
        })
        .ok()
        .flatten();

    InventoryTool {
        backend,
        tool,
        version: tv.version.clone(),
        platform,
        artifact_digest: installed_artifact.map(|artifact| artifact.digest),
        source_url: platform_info
            .and_then(|info| info.url.clone().or(info.url_api.clone()))
            .filter(|url| is_http_url(url)),
        source_digest: platform_info
            .and_then(|info| info.checksum.clone())
            .filter(|digest| is_lower_sha256_digest(digest)),
    }
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn is_lower_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn device_id() -> Result<String> {
    let host = crate::wings::host();
    if let Some(creds) = crate::wings::credentials::cached()
        && creds.host == host
        && let Some(device_id) = creds.device_id
        && !device_id.trim().is_empty()
    {
        return Ok(device_id);
    }

    let path = generated_device_id_path();
    match file::read_to_string(&path) {
        Ok(device_id) if !device_id.trim().is_empty() => Ok(device_id.trim().to_string()),
        Ok(_) | Err(_) => {
            let device_id = format!("mise-{}", crate::rand::random_string(32));
            if let Some(parent) = path.parent() {
                file::create_dir_all(parent)?;
            }
            file::write(&path, &device_id)?;
            Ok(device_id)
        }
    }
}

fn generated_device_id_path() -> std::path::PathBuf {
    crate::env::MISE_STATE_DIR
        .join("wings")
        .join(DEVICE_ID_FILENAME)
}

fn bearer_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .wrap_err("wings token contains invalid header characters")?,
    );
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::toolset::{ToolRequest, ToolSource, ToolVersion};

    use super::*;

    #[test]
    fn inventory_tool_uses_wings_marker_for_artifact_digest() {
        let dir = tempfile::tempdir().unwrap();
        let install_path = dir.path().join("node").join("20.0.0");
        file::create_dir_all(&install_path).unwrap();
        file::write(
            install_path.join(".mise-wings-install.json"),
            r#"{
  "schema": 1,
  "artifact_ref": "registry.mise-wings.en.dev/org/acme/core/node:20.0.0",
  "artifact_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}"#,
        )
        .unwrap();
        let mut tv = tool_version("node", "20.0.0");
        tv.install_path = Some(install_path);

        let tool = inventory_tool(
            "core".into(),
            "node".into(),
            &tv,
            &PlatformTarget::from_current(),
        );

        assert_eq!(
            tool.artifact_digest.as_deref(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(tool.tool, "node");
    }

    #[test]
    fn inventory_tool_includes_lockfile_source_evidence() {
        let mut tv = tool_version("node", "20.0.0");
        let platform = PlatformTarget::from_current().to_key();
        tv.lock_platforms.insert(
            platform.clone(),
            crate::lockfile::PlatformInfo {
                url: Some("https://nodejs.org/dist/v20.0.0/node.tar.gz".into()),
                checksum: Some(
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .into(),
                ),
                ..Default::default()
            },
        );

        let tool = inventory_tool(
            "core".into(),
            "node".into(),
            &tv,
            &PlatformTarget::from_current(),
        );

        assert_eq!(tool.platform, platform);
        assert_eq!(
            tool.source_url.as_deref(),
            Some("https://nodejs.org/dist/v20.0.0/node.tar.gz")
        );
        assert_eq!(
            tool.source_digest.as_deref(),
            Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
    }

    #[test]
    fn inventory_tool_omits_invalid_optional_evidence() {
        let mut tv = tool_version("node", "20.0.0");
        tv.lock_platforms.insert(
            PlatformTarget::from_current().to_key(),
            crate::lockfile::PlatformInfo {
                url: Some("file:///tmp/node.tar.gz".into()),
                checksum: Some("blake3:abc123".into()),
                ..Default::default()
            },
        );

        let tool = inventory_tool(
            "core".into(),
            "node".into(),
            &tv,
            &PlatformTarget::from_current(),
        );

        assert_eq!(tool.source_url, None);
        assert_eq!(tool.source_digest, None);
    }

    fn tool_version(short: &str, version: &str) -> ToolVersion {
        let request = ToolRequest::Version {
            backend: Arc::new(crate::cli::args::BackendArg::new(short.to_string(), None)),
            version: version.into(),
            options: Default::default(),
            source: ToolSource::Unknown,
        };
        ToolVersion::new(request, version.into())
    }
}
