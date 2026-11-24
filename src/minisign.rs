use crate::*;
use minisign_verify::*;
use std::iter::Iterator;
use std::sync::LazyLock;

pub static MISE_PUB_KEY: LazyLock<String> = LazyLock::new(|| {
    include_str!("../minisign.pub")
        .to_string()
        .lines()
        .last()
        .unwrap()
        .to_string()
});

pub fn verify(pub_key: &str, data: &[u8], sig: &str) -> Result<()> {
    let public_key = PublicKey::from_base64(pub_key)?;
    let signature = Signature::decode(sig)?;
    public_key.verify(data, &signature, false)?;
    Ok(())
}

/// Like [`verify`], and also requires the signature to have been made for `filename`.
///
/// A vendor's key signs every release, so a valid signature alone does not tell one release's
/// archive from another's. minisign records `file:<name>` in the trusted comment, which the
/// global signature covers, so a download host cannot change it. A trusted comment without a
/// `file:` field (a custom `minisign -t` comment) names no file and is accepted.
pub fn verify_file(pub_key: &str, data: &[u8], sig: &str, filename: &str) -> Result<()> {
    let public_key = PublicKey::from_base64(pub_key)?;
    let signature = Signature::decode(sig)?;
    public_key.verify(data, &signature, false)?;
    if let Some(signed) = signed_filename(signature.trusted_comment())
        && signed != filename
    {
        eyre::bail!(
            "minisign signature was made for {signed}, not for the downloaded {filename}; \
             the download host served a file signed for another release"
        );
    }
    Ok(())
}

/// The file name in a `timestamp:<ts>\tfile:<name>\thashed` trusted comment. A signer may record
/// the path it was given, so only the last component is returned.
fn signed_filename(trusted_comment: &str) -> Option<&str> {
    let name = trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("file:"))?;
    Some(name.rsplit(['/', '\\']).next().unwrap_or(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real release signature whose trusted comment is
    // `timestamp:1785776133\tfile:SHASUMS256.txt\thashed`
    const SHASUMS: &str =
        include_str!("../crates/mise-system/src/remote_testdata/mise-v2026.8.1-SHASUMS256.txt");
    const SHASUMS_SIG: &str = include_str!(
        "../crates/mise-system/src/remote_testdata/mise-v2026.8.1-SHASUMS256.txt.minisig"
    );

    #[test]
    fn verify_file_accepts_the_signed_file_name() {
        verify_file(
            &MISE_PUB_KEY,
            SHASUMS.as_bytes(),
            SHASUMS_SIG,
            "SHASUMS256.txt",
        )
        .unwrap();
    }

    #[test]
    fn verify_file_refuses_a_signature_made_for_another_file() {
        verify(&MISE_PUB_KEY, SHASUMS.as_bytes(), SHASUMS_SIG).unwrap();
        let err = verify_file(
            &MISE_PUB_KEY,
            SHASUMS.as_bytes(),
            SHASUMS_SIG,
            "SHASUMS512.txt",
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .starts_with("minisign signature was made for SHASUMS256.txt, not for the downloaded SHASUMS512.txt"),
            "{err}"
        );
    }

    #[test]
    fn signed_filename_reads_the_file_field() {
        assert_eq!(
            signed_filename("timestamp:1785776133\tfile:SHASUMS256.txt\thashed"),
            Some("SHASUMS256.txt")
        );
        assert_eq!(
            signed_filename("timestamp:1785776133\tfile:dist/tool-1.0.0.tar.gz\thashed"),
            Some("tool-1.0.0.tar.gz")
        );
        // minio/mc writes `filename:` with a name that is not the asset's; it is not a `file:` field
        assert_eq!(
            signed_filename("timestamp:1757203863\tfilename:mc.RELEASE.2025-08-13T08-35-41Z"),
            None
        );
        assert_eq!(
            signed_filename("cargo-watch v8.5.3 signed with automated key"),
            None
        );
    }
}
