//! Signed, portable vfox plugin archives. Tool installation continues to use vfox.
use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::sync::Arc;

use eyre::{Result, WrapErr, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::backend::packslip::{PackslipBackend, project_name};
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::file;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::{ResolveOptions, ToolRequest, ToolSource, Toolset};
use crate::ui::progress_report::{QuietReport, SingleReport};

const STATE_FILE: &str = ".mise-packslip-plugin.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Source {
    project: String,
    request: String,
}

impl Source {
    pub(crate) fn parse(value: &str) -> Result<Option<Self>> {
        let Some(value) = value.strip_prefix("packslip:") else {
            return Ok(None);
        };
        let (project, request) = value.split_once('#').unwrap_or((value, "latest"));
        let project = project_name(project)?;
        ensure!(
            project.starts_with("github.com/") && packslip::model::repository(&project).is_some(),
            "packslip plugin sources currently require a GitHub repository"
        );
        ensure!(
            !request.is_empty(),
            "packslip plugin version must not be empty"
        );
        Ok(Some(Self {
            project,
            request: request.to_owned(),
        }))
    }

    pub(crate) fn url(&self) -> String {
        format!("packslip:{}#{}", self.project, self.request)
    }

    pub(crate) fn with_version(mut self, version: Option<String>) -> Self {
        if let Some(version) = version {
            self.request = version;
        }
        self
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Installed {
    pub source: Source,
    pub version: String,
    platforms: BTreeMap<String, PlatformInfo>,
}

pub(crate) fn installed(path: &Path) -> Result<Option<Installed>> {
    let state = path.join(STATE_FILE);
    if !state.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&file::read_to_string(state)?)?))
}

pub(crate) async fn install(
    config: &Arc<Config>,
    source: Source,
    path: &Path,
    pr: &dyn SingleReport,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre::eyre!("plugin path has no parent"))?;
    file::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".packslip-")
        .tempdir_in(parent)?;
    let payload = staging.path().join("plugin");
    let previous = installed(path)?;
    let mut ba = BackendArg::from(format!("packslip:{}", source.project).as_str());
    // Each attempt owns its downloads, including concurrent aliases of a plugin.
    ba.downloads_path = staging.path().join("downloads");
    let backend = PackslipBackend::from_arg(ba.clone());
    let request = ToolRequest::new(Arc::new(ba), &source.request, ToolSource::Argument)?;
    pr.set_message(format!("resolve {}", source.url()));
    let mut tv = request
        .resolve(
            config,
            &ResolveOptions {
                latest_versions: true,
                use_locked_version: false,
                refresh_remote_versions: true,
                ..Default::default()
            },
        )
        .await?;
    if let Some(previous) = previous
        && previous.source.project == source.project
        && previous.version == tv.version
    {
        // Reinstalling the same release must retain its digest and signer pins.
        tv.lock_platforms = previous.platforms;
    }
    tv.install_path = Some(payload.clone());
    let ctx = InstallContext {
        config: config.clone(),
        ts: Arc::new(Toolset::new(ToolSource::Argument)),
        pr: Arc::new(QuietReport::new()),
        force: false,
        dry_run: false,
        locked: false,
        before_date: None,
        dependency_context: Default::default(),
    };
    pr.set_message(format!(
        "verify and install {}@{}",
        source.project, tv.version
    ));
    // Reuse the backend's signed release lists, stamper policy, age policy,
    // signature verification, digest checks, and remembered signer pins.
    let (tv, pin) = backend.install_payload(&ctx, tv, true).await?;
    let state = Installed {
        source,
        version: tv.version,
        platforms: tv.lock_platforms,
    };
    file::write(payload.join(STATE_FILE), serde_json::to_vec_pretty(&state)?)?;
    if let Err(error) = replace(&payload, path, &staging.path().join("previous"), || {
        pin.record()
    }) {
        let recovery = staging.keep();
        return Err(error.wrap_err(format!(
            "plugin replacement failed; recovery files retained at {}",
            recovery.display()
        )));
    }
    pr.finish_with_message(format!("{}@{}", state.source.project, state.version));
    Ok(())
}

fn replace(
    payload: &Path,
    destination: &Path,
    backup: &Path,
    commit: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let existed = file::entry_exists(destination);
    if existed {
        file::rename(destination, backup)?;
    }
    if let Err(error) = file::rename(payload, destination) {
        if existed {
            file::rename(backup, destination).wrap_err("restoring previous plugin")?;
        }
        return Err(error);
    }
    if let Err(error) = commit() {
        file::rename(destination, payload)?;
        if existed {
            file::rename(backup, destination).wrap_err("restoring previous plugin")?;
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn validate_artifact(artifact: &packslip::model::Artifact) -> Result<()> {
    ensure!(
        artifact
            .extensions
            .get("mise")
            .and_then(|v| v.get("plugin"))
            .and_then(|v| v.as_str())
            == Some("vfox"),
        "packslip artifact must declare extensions.mise.plugin = vfox"
    );
    ensure!(
        artifact.bin.is_empty(),
        "vfox plugin artifacts must not declare executables"
    );
    ensure!(
        artifact.os.is_none() && artifact.arch.is_none() && artifact.libc.is_none(),
        "vfox plugin artifact must be portable"
    );
    ensure!(
        artifact.requires.is_none(),
        "vfox plugin artifacts must not declare host requirements"
    );
    ensure!(
        matches!(artifact.format.as_deref(), Some("tar.gz" | "tgz")),
        "vfox plugin artifacts currently require tar.gz format"
    );
    Ok(())
}

/// Reject links and special files before extraction, including links whose
/// targets might otherwise be resolved while unpacking a later archive entry.
pub(crate) fn validate_archive(path: &Path) -> Result<()> {
    let reader = flate2::read::GzDecoder::new(std::fs::File::open(path)?);
    let mut archive = jdx_tar::Archive::new(reader);
    for entry in archive.entries()? {
        let entry = entry?;
        ensure!(
            matches!(
                entry.entry_type(),
                jdx_tar::EntryType::File | jdx_tar::EntryType::Directory
            ),
            "vfox plugin archive contains a link or special file"
        );
        let path = entry.path()?;
        for component in path.components() {
            if component == Component::CurDir {
                continue;
            }
            let Component::Normal(name) = component else {
                bail!("vfox plugin archive contains an unsafe path");
            };
            let name = name.to_string_lossy();
            ensure!(
                !name.contains(['\\', ':'])
                    && !name.eq_ignore_ascii_case(".git")
                    && !name.eq_ignore_ascii_case(STATE_FILE),
                "vfox plugin archive contains a reserved or unsafe path"
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_layout(path: &Path) -> Result<()> {
    ensure!(
        path.join("metadata.lua").is_file(),
        "vfox plugin archive must contain metadata.lua at its root"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_artifact_contract() {
        let payload = packslip::sigstore::peek_statement(include_str!(
            "../../test/fixtures/packslip-vfox/packslip.sigstore.json"
        ))
        .unwrap();
        let mut statement: packslip::model::Statement = serde_json::from_slice(&payload).unwrap();
        let artifact = statement.predicate.artifacts.remove(0);
        validate_artifact(&artifact).unwrap();
        let mut invalid = artifact.clone();
        invalid.extensions.clear();
        assert!(validate_artifact(&invalid).is_err());
        let mut invalid = artifact.clone();
        invalid.os = Some("linux".into());
        assert!(validate_artifact(&invalid).is_err());
        let mut invalid = artifact;
        invalid.bin.push(packslip::model::Bin {
            name: "bfs".into(),
            path: "bfs".into(),
        });
        assert!(validate_artifact(&invalid).is_err());
    }

    #[test]
    fn archive_rejects_links_and_reserved_paths() {
        let temp = tempfile::tempdir().unwrap();
        for (name, link) in [
            ("metadata.lua", false),
            ("./metadata.lua", false),
            (".git/config", false),
            (STATE_FILE, false),
            ("hooks/link", true),
        ] {
            let path = temp.path().join("plugin.tar.gz");
            let output = flate2::write::GzEncoder::new(
                std::fs::File::create(&path).unwrap(),
                flate2::Compression::default(),
            );
            let mut builder = jdx_tar::Builder::new(output);
            if link {
                let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::Symlink);
                header.set_mode(0o777);
                builder
                    .append_link(&mut header, name, "../../outside")
                    .unwrap();
            } else {
                let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::File);
                header.set_size(0);
                header.set_mode(0o644);
                builder.append_data(&mut header, name, &b""[..]).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
            assert_eq!(
                validate_archive(&path).is_ok(),
                matches!(name, "metadata.lua" | "./metadata.lua"),
                "{name}"
            );
        }
    }

    #[test]
    fn failed_pin_restores_previous_plugin() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = temp.path().join("plugin");
        let payload = temp.path().join("payload");
        std::fs::create_dir(&plugin).unwrap();
        std::fs::create_dir(&payload).unwrap();
        std::fs::write(plugin.join("metadata.lua"), "previous").unwrap();
        std::fs::write(payload.join("metadata.lua"), "new").unwrap();
        let result = replace(&payload, &plugin, &temp.path().join("backup"), || {
            assert_eq!(
                std::fs::read_to_string(plugin.join("metadata.lua")).unwrap(),
                "new"
            );
            eyre::bail!("pin write failed")
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(plugin.join("metadata.lua")).unwrap(),
            "previous"
        );
    }

    #[test]
    fn failed_replacement_restores_previous_plugin() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = temp.path().join("plugin");
        std::fs::create_dir(&plugin).unwrap();
        std::fs::write(plugin.join("metadata.lua"), "previous").unwrap();
        assert!(
            replace(
                &temp.path().join("missing"),
                &plugin,
                &temp.path().join("backup"),
                || panic!("must not record a pin before replacement succeeds")
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(plugin.join("metadata.lua")).unwrap(),
            "previous"
        );
    }
}
