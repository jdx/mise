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
    /// We verify `sha256(bytes) == digest` before writing so that a corrupted
    /// or tampered registry response surfaces here — with a clear "got X,
    /// wanted Y" message — instead of much later as a confusing digest
    /// mismatch from `skopeo inspect` or `podman load`.
    pub fn write_blob_with_digest(&self, digest: &str, bytes: &[u8]) -> Result<()> {
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
