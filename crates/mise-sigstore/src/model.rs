use super::*;

#[derive(Debug, Error)]
pub enum AttestationError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Verification failed: {0}")]
    Verification(String),
    #[error("Workflow verification failed: {0}")]
    WorkflowMismatch(String),
    #[error("SLSA subject mismatch: {0}")]
    SubjectMismatch(String),
    #[error("Unsupported attestation format: {0}")]
    UnsupportedFormat(String),
    #[error("No attestations found")]
    NoAttestations,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Sigstore error: {0}")]
    Sigstore(String),
}

impl From<sigstore_verify::Error> for AttestationError {
    fn from(err: sigstore_verify::Error) -> Self {
        AttestationError::Sigstore(err.to_string())
    }
}

impl From<sigstore_verify::types::Error> for AttestationError {
    fn from(err: sigstore_verify::types::Error) -> Self {
        AttestationError::Sigstore(err.to_string())
    }
}

impl From<sigstore_verify::trust_root::Error> for AttestationError {
    fn from(err: sigstore_verify::trust_root::Error) -> Self {
        AttestationError::Sigstore(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AttestationError>;

#[derive(Debug, Clone)]
pub struct SlsaArtifact {
    pub name: String,
    pub sha256: String,
}

/// Expected signer of an SLSA provenance statement.
///
/// Both values must match the Fulcio certificate exactly. The identity is the
/// certificate's URI subject alternative name, including its workflow ref.
#[derive(Debug, Clone, Copy)]
pub struct SlsaSignerIdentity<'a> {
    pub identity: &'a str,
    pub issuer: &'a str,
}

impl SlsaArtifact {
    pub fn from_bytes(name: String, bytes: &[u8]) -> Self {
        Self {
            name,
            sha256: hex::encode(Sha256::digest(bytes)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactRef {
    pub(crate) digest: String,
}

impl ArtifactRef {
    pub fn from_digest(digest: &str) -> Self {
        if digest.contains(':') {
            Self {
                digest: digest.to_string(),
            }
        } else {
            Self {
                digest: format!("sha256:{digest}"),
            }
        }
    }
}

#[async_trait]
pub trait AttestationSource {
    async fn fetch_attestations(&self, artifact: &ArtifactRef) -> Result<Vec<Attestation>>;
}
