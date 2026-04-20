//! OCI image layout writer — produces a directory conforming to the
//! [OCI image-layout spec](https://github.com/opencontainers/image-spec/blob/main/image-layout.md).
//!
//! Layout:
//! ```text
//! <root>/
//!   oci-layout
//!   index.json
//!   blobs/
//!     sha256/
//!       <digest>     — manifest / config / layer blobs
//! ```

use std::path::{Path, PathBuf};

use eyre::{Context, Result};
use sha2::{Digest, Sha256};

use crate::file;
use crate::oci::manifest::{
    Descriptor, ImageIndex, ImageManifest, MEDIA_TYPE_OCI_INDEX, OciLayout, Platform,
};

pub struct ImageLayout {
    pub root: PathBuf,
}

impl ImageLayout {
    pub fn init(root: &Path) -> Result<Self> {
        file::create_dir_all(root)?;
        file::create_dir_all(root.join("blobs/sha256"))?;
        let layout_path = root.join("oci-layout");
        let layout = OciLayout::default();
        file::write(&layout_path, serde_json::to_vec(&layout)?)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Write raw bytes as a blob. Returns the blob digest (as `sha256:...`).
    /// Idempotent — if the blob already exists it is left untouched.
    pub fn write_blob(&self, bytes: &[u8]) -> Result<(String, u64)> {
        let mut h = Sha256::new();
        h.update(bytes);
        let hex = crate::oci::layer::hex_encode(&h.finalize());
        let digest = format!("sha256:{hex}");
        let path = self.blob_path(&digest);
        if !path.exists() {
            file::write(&path, bytes)?;
        }
        Ok((digest, bytes.len() as u64))
    }

    /// Copy a blob into the layout by its known digest.
    ///
    /// Two guards:
    ///  1. The digest must be `sha256:` followed by 64 lowercase hex chars.
    ///     This prevents path traversal (e.g. a malicious registry returning
    ///     `sha256:../../etc/passwd` would otherwise let us write attacker-
    ///     controlled bytes to an arbitrary filesystem path).
    ///  2. We verify `sha256(bytes) == digest` before writing so corrupted
    ///     or tampered content surfaces with a clear "got X, wanted Y"
    ///     message instead of later as a confusing mismatch from skopeo /
    ///     podman.
    pub fn write_blob_with_digest(&self, digest: &str, bytes: &[u8]) -> Result<()> {
        validate_sha256_digest(digest)?;
        let mut h = Sha256::new();
        h.update(bytes);
        let actual = format!("sha256:{}", crate::oci::layer::hex_encode(&h.finalize()));
        if actual != digest {
            eyre::bail!("blob digest mismatch: got {actual}, expected {digest}");
        }
        let path = self.blob_path(digest);
        if !path.exists() {
            file::write(&path, bytes)?;
        }
        Ok(())
    }

    pub fn blob_path(&self, digest: &str) -> PathBuf {
        let hex = digest.trim_start_matches("sha256:");
        self.root.join("blobs/sha256").join(hex)
    }

    #[allow(dead_code)]
    pub fn read_blob(&self, digest: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(digest);
        std::fs::read(&path).wrap_err_with(|| format!("reading blob {}", path.display()))
    }

    /// Write the top-level `index.json` pointing at a single manifest descriptor.
    pub fn write_index(
        &self,
        manifest_digest: &str,
        manifest_size: u64,
        platform: Option<Platform>,
        tag: Option<&str>,
    ) -> Result<()> {
        use indexmap::IndexMap;

        let mut annotations = IndexMap::new();
        if let Some(tag) = tag {
            annotations.insert(
                "org.opencontainers.image.ref.name".to_string(),
                tag.to_string(),
            );
        }
        let desc = Descriptor {
            media_type: crate::oci::manifest::MEDIA_TYPE_OCI_MANIFEST.to_string(),
            size: manifest_size,
            digest: manifest_digest.to_string(),
            annotations,
            platform,
        };
        let index = ImageIndex {
            schema_version: 2,
            media_type: MEDIA_TYPE_OCI_INDEX.to_string(),
            manifests: vec![desc],
        };
        let path = self.root.join("index.json");
        file::write(&path, serde_json::to_vec_pretty(&index)?)?;
        Ok(())
    }

    pub fn write_manifest(&self, manifest: &ImageManifest) -> Result<(String, u64)> {
        let bytes = serde_json::to_vec(manifest)?;
        self.write_blob(&bytes)
    }
}

/// Validate that `digest` is a well-formed `sha256:<64 lowercase hex>` string.
/// Guards against path traversal from a malicious registry returning something
/// like `sha256:../../etc/passwd` as a layer digest — without this check, that
/// would be used directly as a filesystem path component.
fn validate_sha256_digest(digest: &str) -> Result<()> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        eyre::bail!("invalid blob digest (expected sha256: prefix): {digest}");
    };
    if hex.len() != 64
        || !hex
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        eyre::bail!("invalid blob digest (expected 64 lowercase hex chars): {digest}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_sha256_digest("sha256:../../etc/passwd").is_err());
        assert!(validate_sha256_digest("sha256:../foo").is_err());
        assert!(validate_sha256_digest("../bad").is_err());
        assert!(validate_sha256_digest("sha256:DEADBEEF").is_err());
    }

    #[test]
    fn accepts_valid_digest() {
        let d = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(validate_sha256_digest(d).is_ok());
    }
}
