//! Attestation fetching and signature verification for mise.

#![deny(unreachable_pub)]

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sigstore_verify::VerificationPolicy;
pub use sigstore_verify::trust_root::DEFAULT_TUF_URL;
use sigstore_verify::trust_root::{PRODUCTION_TUF_ROOT, SigstoreInstance, TrustedRoot, TufConfig};
use sigstore_verify::types::bundle::VerificationMaterialContent;
use sigstore_verify::types::{
    Artifact, Bundle, DerCertificate, DerPublicKey, HashAlgorithm, Sha256Hash, SignatureBytes,
    SignatureContent,
};
use thiserror::Error;
use tokio::io::AsyncReadExt;

mod bundle;
mod certificate;
mod client;
mod cosign;
mod cosign_identity;
mod digest;
mod github;
mod model;
mod retry;
mod slsa;
mod trust;

pub use client::{
    Attestation, AttestationClient, AttestationClientBuilder, FetchParams, GitHubSource, sources,
};
pub use cosign::{verify_cosign_signature, verify_cosign_signature_with_key};
pub use cosign_identity::CosignIdentity;
pub use digest::calculate_file_digest;
pub use github::{
    GithubAttestationRequest, verify_github_attestation, verify_github_attestation_sources,
    verify_github_attestation_with_attestations, verify_github_attestation_with_base_url,
    verify_github_attestation_with_base_url_and_digest,
};
pub use model::{
    ArtifactRef, AttestationError, AttestationSource, Result, SlsaArtifact, SlsaSignerIdentity,
};
pub use retry::RetryConfig;
pub use slsa::{
    is_slsa_subject_mismatch, verify_slsa_provenance, verify_slsa_provenance_artifacts,
};
pub use trust::set_tuf_url;

use bundle::*;
use certificate::*;
use cosign_identity::*;
use retry::*;
use slsa::*;
use trust::*;

#[cfg(test)]
mod tests;
