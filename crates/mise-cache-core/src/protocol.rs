use super::*;

pub const PROTOCOL_VERSION: u8 = 1;
pub(crate) const PROTOCOL_HEADER: &str = "mise-cache-protocol";
pub(crate) const NAMESPACE_HEADER: &str = "mise-cache-namespace";
pub const ACTION_RESULT_MEDIA_TYPE: &str = "application/vnd.mise.cache-action-result.v1+json";
pub const DIRECTORY_MEDIA_TYPE: &str = "application/vnd.mise.cache-directory.v1+json";
pub const CLIENT_METADATA_MEDIA_TYPE: &str = "application/vnd.mise.cache-client-metadata.v1+json";
pub const BLOB_MEDIA_TYPE: &str = "application/octet-stream";
pub const BLOB_PACK_MEDIA_TYPE: &str = "application/vnd.mise.cache-blob-pack.v1";
pub(crate) const DIGEST_LIST_MEDIA_TYPE: &str = "application/vnd.mise.cache-digests.v1+json";
pub(crate) const BLOB_PACK_BLOBS_HEADER: &str = "mise-cache-pack-blobs";
pub(crate) const BLOB_PACK_BYTES_HEADER: &str = "mise-cache-pack-bytes";
pub(crate) const BLOB_PACK_MAGIC: &[u8; 8] = b"MISEPK01";
pub(crate) const BLOB_PACK_HEADER_BYTES: u64 = 1 + 32 + 8;
pub(crate) const MAX_STAGED_BLOB_PACK_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_STAGED_BLOB_PACK_ITEMS: usize = 2 * 1024;
pub(crate) const BLOB_PACK_TIMEOUT_BYTES_PER_UNIT: u64 = MAX_STAGED_BLOB_PACK_BYTES / 4;
pub(crate) const BLOB_PACK_TIMEOUT_ITEMS_PER_UNIT: usize = MAX_STAGED_BLOB_PACK_ITEMS / 4;

/// Serialize a protocol object using the JSON Canonicalization Scheme.
///
/// Action digests are computed from these bytes, so callers must not use
/// serde's struct field order as part of the wire contract.
pub fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(serde_json_canonicalizer::to_vec(value)?)
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RemoteCacheMode {
    #[default]
    ReadWrite,
    ReadOnly,
    WriteOnly,
}

impl RemoteCacheMode {
    pub fn reads(self) -> bool {
        matches!(self, Self::ReadWrite | Self::ReadOnly)
    }

    pub fn writes(self) -> bool {
        matches!(self, Self::ReadWrite | Self::WriteOnly)
    }
}

pub struct RemoteCacheConfig {
    pub base_url: Url,
    pub namespace: String,
    pub token: Option<String>,
    pub token_file: Option<PathBuf>,
    pub oidc_audience: Option<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub download_timeout: Duration,
    pub retries: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheDigest {
    pub algorithm: String,
    pub hash: String,
    pub size: u64,
}

impl CacheDigest {
    pub fn blake3(bytes: &[u8]) -> Self {
        Self {
            algorithm: "blake3".into(),
            hash: blake3::hash(bytes).to_hex().to_string(),
            size: bytes.len() as u64,
        }
    }

    /// Hash a file while counting the bytes read in the same streaming pass.
    pub fn blake3_file(path: &Path) -> Result<Self> {
        let (hash, size) = hash_file_blake3(path)?;
        Ok(Self {
            algorithm: "blake3".into(),
            hash,
            size,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.algorithm != "blake3" && self.algorithm != "sha256" {
            bail!("unsupported remote cache digest algorithm");
        }
        if self.hash.len() != 64
            || !self
                .hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("invalid remote cache digest");
        }
        Ok(())
    }

    pub fn matches_bytes(&self, bytes: &[u8]) -> Result<bool> {
        self.validate()?;
        if self.size != bytes.len() as u64 {
            return Ok(false);
        }
        let hash = match self.algorithm.as_str() {
            "blake3" => blake3::hash(bytes).to_hex().to_string(),
            "sha256" => hex::encode(sha2::Sha256::digest(bytes)),
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.hash == hash)
    }

    pub fn matches_file(&self, path: &Path) -> Result<bool> {
        self.validate()?;
        let (hash, size) = match self.algorithm.as_str() {
            "blake3" => hash_file_blake3(path)?,
            "sha256" => hash_file_sha256(path)?,
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.size == size && self.hash == hash)
    }
}

pub(crate) fn hash_file_blake3(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((hasher.finalize().to_hex().to_string(), size))
}

pub(crate) fn hash_file_sha256(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

/// A canonical action-result record referencing objects in the CAS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteActionResult {
    pub action: CacheDigest,
    #[serde(default)]
    pub metadata: Option<CacheDigest>,
    #[serde(default)]
    pub output_root: Option<CacheDigest>,
    pub version: u8,
}

/// A canonical directory object stored in the CAS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDirectory {
    pub directories: Vec<CacheDirectoryNode>,
    pub files: Vec<CacheFileNode>,
    pub symlinks: Vec<CacheSymlinkNode>,
    pub version: u8,
}

/// A child directory entry in a canonical cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDirectoryNode {
    pub digest: CacheDigest,
    pub mode: u32,
    pub name: String,
}

/// A file entry in a canonical cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheFileNode {
    pub digest: CacheDigest,
    pub executable: bool,
    pub mode: u32,
    pub name: String,
}

/// A symbolic-link entry in a canonical cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheSymlinkNode {
    pub mode: u32,
    pub name: String,
    pub target: String,
}

pub enum BlobSource {
    Bytes(Vec<u8>),
    File(tempfile::NamedTempFile),
    Path(PathBuf),
}

pub struct BlobUpload {
    pub digest: CacheDigest,
    pub source: BlobSource,
}

/// A verified set of remote CAS objects downloaded through blob-pack streams.
pub struct RemoteBlobPack {
    pub(crate) _directory: tempfile::TempDir,
    pub blobs: Vec<(CacheDigest, PathBuf)>,
    pub requests: u64,
    pub requested: Vec<CacheDigest>,
    pub blob_count: u64,
    pub payload_bytes: u64,
    pub framed_bytes: u64,
}

pub(crate) struct DownloadedBlobPack {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) blobs: Vec<(CacheDigest, PathBuf)>,
    pub(crate) metadata: BlobPackResponseStats,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BlobPackResponseMetadata {
    content_length: Option<u64>,
    blob_count: Option<u64>,
    payload_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BlobPackResponseStats {
    pub(crate) blob_count: u64,
    pub(crate) payload_bytes: u64,
    pub(crate) framed_bytes: u64,
}

impl BlobPackResponseMetadata {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Result<Self> {
        Ok(Self {
            content_length: optional_u64_header(headers, CONTENT_LENGTH.as_str())?,
            blob_count: optional_u64_header(headers, BLOB_PACK_BLOBS_HEADER)?,
            payload_bytes: optional_u64_header(headers, BLOB_PACK_BYTES_HEADER)?,
        })
    }

    pub(crate) fn validate(self, decoded: BlobPackResponseStats) -> Result<BlobPackResponseStats> {
        if let Some(content_length) = self.content_length
            && content_length != decoded.framed_bytes
        {
            bail!(
                "remote cache blob pack content length metadata mismatch: expected {}, decoded {}",
                content_length,
                decoded.framed_bytes
            );
        }
        if let Some(blob_count) = self.blob_count
            && blob_count != decoded.blob_count
        {
            bail!(
                "remote cache blob pack blob count metadata mismatch: expected {}, decoded {}",
                blob_count,
                decoded.blob_count
            );
        }
        if let Some(payload_bytes) = self.payload_bytes
            && payload_bytes != decoded.payload_bytes
        {
            bail!(
                "remote cache blob pack payload byte metadata mismatch: expected {}, decoded {}",
                payload_bytes,
                decoded.payload_bytes
            );
        }
        Ok(BlobPackResponseStats {
            blob_count: self.blob_count.unwrap_or(decoded.blob_count),
            payload_bytes: self.payload_bytes.unwrap_or(decoded.payload_bytes),
            framed_bytes: self.content_length.unwrap_or(decoded.framed_bytes),
        })
    }
}

pub(crate) fn optional_u64_header(headers: &HeaderMap, name: &str) -> Result<Option<u64>> {
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| eyre!("remote cache blob pack {name} header is not valid UTF-8"))?;
    let value = value
        .parse::<u64>()
        .map_err(|_| eyre!("remote cache blob pack {name} header is not an unsigned integer"))?;
    Ok(Some(value))
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteCacheCapabilities {
    pub(crate) protocol: CapabilityProtocol,
    #[serde(default)]
    pub(crate) features: CapabilityFeatures,
    #[serde(default)]
    pub(crate) limits: CapabilityLimits,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CapabilityProtocol {
    pub(crate) major: u8,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CapabilityFeatures {
    #[serde(default)]
    pub(crate) blob_packs: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CapabilityLimits {
    #[serde(default)]
    pub(crate) max_batch_items: u64,
    #[serde(default)]
    pub(crate) max_pack_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BlobPackLimits {
    pub(crate) max_items: usize,
    pub(crate) max_bytes: u64,
}

#[derive(Serialize)]
pub(crate) struct DigestList<'a> {
    pub(crate) digests: &'a [CacheDigest],
}
