//! Bottle downloads from ghcr.io with sha256 verification.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::unix::fs::OpenOptionsExt;
#[cfg(not(target_os = "linux"))]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::{error::Error, fmt};

use eyre::{WrapErr, bail};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::api::BottleFile;
use crate::http::HTTP;
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

#[derive(Debug, Clone)]
pub struct OciBottleMetadata {
    pub tab: Value,
    pub sbom_supplement: Option<Value>,
}

#[derive(Debug)]
pub(super) struct DescriptorIdentityMiss {
    name: String,
    tag: String,
}

impl fmt::Display for DescriptorIdentityMiss {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "brew:{}: OCI index has no descriptor for {}",
            self.name, self.tag
        )
    }
}

impl Error for DescriptorIdentityMiss {}

/// Checksum-verified bottle bytes retained on an anonymous file descriptor.
///
/// Inspection and extraction clone this descriptor instead of reopening the
/// mutable cache pathname, so another same-UID process cannot swap the bottle
/// between lifecycle authorization and pour.
#[derive(Debug)]
pub struct VerifiedArtifact {
    file: File,
    label: PathBuf,
}

impl VerifiedArtifact {
    pub(super) fn from_path(
        path: &Path,
        expected_sha256: &str,
        pr: Option<&dyn SingleReport>,
    ) -> Result<Option<Self>> {
        let mut source = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(path)
            .wrap_err_with(|| format!("failed to retain verified bottle: {}", path.display()))?;
        let metadata = source.metadata()?;
        if !metadata.is_file() {
            bail!(
                "bottle cache entry is not a regular file: {}",
                path.display()
            );
        }
        if let Some(pr) = pr {
            pr.set_length(metadata.len());
        }
        let mut retained = new_retained_file()?;
        let copied = std::io::copy(&mut source, &mut retained)?;
        if let Some(pr) = pr {
            pr.inc(copied);
        }
        let (retained, actual) = seal_and_hash_retained(retained)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Ok(None);
        }
        retained.sync_all()?;
        Ok(Some(Self {
            file: retained,
            label: path.to_path_buf(),
        }))
    }

    pub fn reader(&self) -> Result<File> {
        let mut file = self.file.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }

    pub fn label(&self) -> &Path {
        &self.label
    }

    pub(super) fn publish_cache(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| eyre::eyre!("bottle cache path has no parent"))?;
        crate::file::create_dir_all(parent)?;
        let mut source = self.reader()?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        std::io::copy(&mut source, &mut staged)?;
        staged.as_file_mut().sync_all()?;
        staged.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    pub(super) async fn from_response(
        mut response: reqwest::Response,
        label: &Path,
        expected_sha256: &str,
        pr: Option<&dyn SingleReport>,
    ) -> Result<Self> {
        if let Some(pr) = pr
            && let Some(length) = response.content_length()
        {
            pr.set_length(length);
        }
        let retained = new_retained_file()?;
        let mut output = tokio::fs::File::from_std(retained);
        while let Some(chunk) = response.chunk().await? {
            if crate::ui::ctrlc::is_cancelled() {
                bail!("download cancelled by user");
            }
            output.write_all(&chunk).await?;
            if let Some(pr) = pr {
                pr.inc(chunk.len() as u64);
            }
        }
        output.flush().await?;
        output.sync_all().await?;
        let (file, actual) = seal_and_hash_retained(output.into_std().await)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            bail!(
                "checksum mismatch for {}: expected {expected_sha256}, got {actual}",
                label.display()
            );
        }
        Ok(Self {
            file,
            label: label.to_path_buf(),
        })
    }
}

#[cfg(target_os = "linux")]
fn new_retained_file() -> Result<File> {
    let fd = unsafe {
        nix::libc::syscall(
            nix::libc::SYS_memfd_create,
            c"mise-brew-verified".as_ptr(),
            nix::libc::MFD_CLOEXEC | nix::libc::MFD_ALLOW_SEALING,
        )
    };
    if fd == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}

#[cfg(not(target_os = "linux"))]
fn new_retained_file() -> Result<File> {
    Ok(tempfile::tempfile()?)
}

fn seal_and_hash_retained(mut file: File) -> Result<(File, String)> {
    file.sync_all()?;
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let seals = nix::libc::F_SEAL_WRITE
            | nix::libc::F_SEAL_GROW
            | nix::libc::F_SEAL_SHRINK
            | nix::libc::F_SEAL_SEAL;
        if unsafe { nix::libc::fcntl(file.as_raw_fd(), nix::libc::F_ADD_SEALS, seals) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        let actual = unsafe { nix::libc::fcntl(file.as_raw_fd(), nix::libc::F_GET_SEALS) };
        if actual == -1 || actual & seals != seals {
            bail!("verified download could not be made immutable")
        }
    }
    #[cfg(not(target_os = "linux"))]
    file.set_permissions(std::fs::Permissions::from_mode(0o400))?;

    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok((file, hex::encode(hasher.finalize())))
}

async fn download_verified_bottle(
    url: &str,
    label: &Path,
    expected_sha256: &str,
    headers: &HeaderMap,
    pr: Option<&dyn SingleReport>,
) -> Result<VerifiedArtifact> {
    let response = HTTP
        .get_async_with_headers_allow_error_status(url, headers)
        .await?
        .error_for_status()?;
    VerifiedArtifact::from_response(response, label, expected_sha256, pr).await
}

/// Download a bottle to the mise cache (or reuse a verified cached copy).
pub async fn fetch_bottle(
    name: &str,
    pkg_version: &str,
    bottle: &BottleFile,
    pr: Option<&dyn SingleReport>,
) -> Result<VerifiedArtifact> {
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("bottles");
    let path = cache_dir.join(format!("{name}-{pkg_version}.tar.gz"));
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if let Some(verified) = VerifiedArtifact::from_path(&path, &bottle.sha256, pr)? {
                debug!("bottle cache hit: {}", path.display());
                return Ok(verified);
            }
        }
        Ok(_) => bail!(
            "bottle cache entry is not a regular file: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(pr) = pr {
        pr.set_message(format!("download {name}-{pkg_version}.tar.gz"));
    }
    // ghcr.io allows anonymous pulls with this static bearer token
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
    let verified =
        download_verified_bottle(&bottle.url, &path, &bottle.sha256, &headers, pr).await?;
    if let Some(pr) = pr {
        pr.set_message("checksum".to_string());
    }
    verified.publish_cache(&path)?;
    Ok(verified)
}

/// Fetch digest-bound GHCR metadata. A URL without an OCI blob identity is an
/// archive bottle and must derive facts from its checksum-verified archive.
pub async fn fetch_oci_bottle_metadata(
    name: &str,
    pkg_version: &str,
    rebuild: u32,
    tag: &str,
    bottle: &BottleFile,
) -> Result<Option<OciBottleMetadata>> {
    let Some((registry, _)) = bottle.url.split_once("/blobs/") else {
        return Ok(None);
    };
    let manifest_version = manifest_version_rebuild(pkg_version, rebuild);
    let url = format!("{registry}/manifests/{manifest_version}");
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json",
        ),
    );
    let response = HTTP
        .get_async_with_headers_allow_error_status(url, &headers)
        .await?
        .error_for_status()?;
    let index: Value = response.json().await?;
    oci_metadata_from_index(name, pkg_version, rebuild, tag, &bottle.sha256, &index).map(Some)
}

fn manifest_version_rebuild(pkg_version: &str, rebuild: u32) -> String {
    if rebuild == 0 {
        pkg_version.to_string()
    } else {
        format!("{pkg_version}-{rebuild}")
    }
}

fn oci_metadata_from_index(
    name: &str,
    pkg_version: &str,
    rebuild: u32,
    tag: &str,
    bottle_sha256: &str,
    index: &Value,
) -> Result<OciBottleMetadata> {
    let expected_ref = if rebuild == 0 {
        format!("{pkg_version}.{tag}")
    } else {
        format!("{pkg_version}.{tag}.{rebuild}")
    };
    let descriptor = index
        .get("manifests")
        .and_then(Value::as_array)
        .and_then(|manifests| {
            manifests.iter().find(|manifest| {
                let annotations = manifest.get("annotations");
                annotations
                    .and_then(|value| value.get("sh.brew.bottle.digest"))
                    .and_then(Value::as_str)
                    == Some(bottle_sha256)
                    && annotations
                        .and_then(|value| value.get("org.opencontainers.image.ref.name"))
                        .and_then(Value::as_str)
                        == Some(expected_ref.as_str())
            })
        })
        .ok_or_else(|| DescriptorIdentityMiss {
            name: name.to_string(),
            tag: tag.to_string(),
        })?;
    let annotations = descriptor
        .get("annotations")
        .and_then(Value::as_object)
        .ok_or_else(|| eyre::eyre!("brew:{name}: OCI descriptor has no annotations"))?;
    let tab = annotations
        .get("sh.brew.tab")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre::eyre!("brew:{name}: OCI descriptor has no sh.brew.tab"))?;
    let sbom_supplement = annotations
        .get("sh.brew.sbom.supplement")
        .and_then(Value::as_str)
        .map(|sbom| {
            serde_json::from_str(sbom)
                .wrap_err_with(|| format!("brew:{name}: invalid SBOM supplement annotation"))
        })
        .transpose()?
        .and_then(|supplement| select_sbom_supplement(supplement, &super::tag::host_tag()));
    Ok(OciBottleMetadata {
        tab: serde_json::from_str(tab)
            .wrap_err_with(|| format!("brew:{name}: invalid sh.brew.tab annotation"))?,
        sbom_supplement,
    })
}

/// Homebrew's BottleManifest resource resolves a tagged supplement against
/// the host tag, not the selected bottle tag. This matters for `all` bottles:
/// their single OCI descriptor contains a supplement for every supported host.
fn select_sbom_supplement(supplement: Value, host_tag: &str) -> Option<Value> {
    let Some(tags) = supplement.get("tags").and_then(Value::as_object) else {
        return Some(supplement);
    };
    tags.get(host_tag)
        .filter(|value| value.is_object())
        .cloned()
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::io::Write;

    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn retained_download_is_sealed_before_use() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("artifact");
        crate::file::write(&path, "verified")?;
        let sha256 = crate::hash::file_hash_sha256(&path, None)?;
        let artifact = VerifiedArtifact::from_path(&path, &sha256, None)?
            .ok_or_else(|| eyre::eyre!("verified artifact checksum mismatched"))?;

        let mut writer = artifact.file.try_clone()?;
        let error = writer.write_all(b"mutate").unwrap_err();
        assert_eq!(error.raw_os_error(), Some(nix::libc::EPERM));
        Ok(())
    }

    #[tokio::test]
    async fn non_oci_bottle_is_explicitly_archive_backed() {
        let bottle = BottleFile {
            cellar: ":any".to_string(),
            url: "https://github.com/example/tap/releases/download/v1/tool.tar.gz".to_string(),
            sha256: "abc123".to_string(),
        };
        assert!(
            fetch_oci_bottle_metadata("tool", "1.0.0", 0, "arm64_sonoma", &bottle)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn oci_sbom_supplement_is_optional_like_homebrew() {
        let index = serde_json::json!({
            "manifests": [{
                "annotations": {
                    "org.opencontainers.image.ref.name": "1.0.arm64_tahoe.1",
                    "sh.brew.bottle.digest": "abc123",
                    "sh.brew.tab": "{\"compiler\":\"clang\"}"
                }
            }]
        });
        let metadata =
            oci_metadata_from_index("foo", "1.0", 1, "arm64_tahoe", "abc123", &index).unwrap();
        assert_eq!(metadata.tab["compiler"], "clang");
        assert!(metadata.sbom_supplement.is_none());
    }

    #[test]
    fn oci_metadata_rejects_matching_tag_with_wrong_bottle_digest() {
        let index = serde_json::json!({
            "manifests": [{
                "annotations": {
                    "org.opencontainers.image.ref.name": "1.0.arm64_tahoe",
                    "sh.brew.bottle.digest": "different-bottle",
                    "sh.brew.tab": "{}"
                }
            }]
        });

        let error = oci_metadata_from_index(
            "foo",
            "1.0",
            0,
            "arm64_tahoe",
            "expected-bottle",
            &index,
        )
            .unwrap_err();
        assert!(error.downcast_ref::<DescriptorIdentityMiss>().is_some());
    }

    #[test]
    fn all_bottle_sbom_supplement_selects_the_host_tag() {
        let current = serde_json::json!({
            "packages": [{"SPDXID": "SPDXRef-current"}]
        });
        let supplement = serde_json::json!({
            "tags": {
                "arm64_tahoe": current,
                "x86_64_linux": {
                    "packages": [{"SPDXID": "SPDXRef-other"}]
                }
            }
        });

        assert_eq!(
            select_sbom_supplement(supplement.clone(), "arm64_tahoe"),
            Some(current)
        );
        assert_eq!(select_sbom_supplement(supplement, "arm64_sequoia"), None);
    }

    #[test]
    fn oci_all_bottle_metadata_exposes_only_the_current_host_supplement() {
        let host_tag = super::super::tag::host_tag();
        let expected = serde_json::json!({
            "packages": [{"SPDXID": "SPDXRef-current"}]
        });
        let supplement = serde_json::json!({
            "tags": {
                host_tag: expected,
                "foreign": {"packages": [{"SPDXID": "SPDXRef-foreign"}]}
            }
        });
        let index = serde_json::json!({
            "manifests": [{
                "annotations": {
                    "org.opencontainers.image.ref.name": "1.0.all",
                    "sh.brew.bottle.digest": "abc123",
                    "sh.brew.tab": "{\"compiler\":\"clang\"}",
                    "sh.brew.sbom.supplement": supplement.to_string()
                }
            }]
        });

        let metadata =
            oci_metadata_from_index("foo", "1.0", 0, "all", "abc123", &index).unwrap();
        assert_eq!(metadata.sbom_supplement, Some(expected));
    }
}
