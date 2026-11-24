use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::Path;

use crate::file;
use crate::file::display_path;
use crate::progress::SingleReport;
use blake3::Hasher as Blake3Hasher;
use digest::Digest;
use eyre::{Result, bail};
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};
use siphasher::sip::SipHasher;

/// A downloaded file's hash differs from the expected checksum.
#[derive(Debug, thiserror::Error)]
#[error(
    "Checksum mismatch for file {path}:\nExpected: {algo}:{expected}\nActual:   {algo}:{actual}"
)]
pub struct ChecksumMismatch {
    path: String,
    algo: String,
    expected: String,
    actual: String,
}

pub fn hash_to_str<T: Hash>(t: &T) -> String {
    let mut s = SipHasher::new();
    t.hash(&mut s);
    format!("{:x}", s.finish())
}

pub fn hash_sha256_to_str(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn file_hash_sha256(path: &Path, pr: Option<&dyn SingleReport>) -> Result<String> {
    let use_external_hasher = file::size(path).unwrap_or_default() > 50 * 1024 * 1024;
    if use_external_hasher && file::which("sha256sum").is_some() {
        let out = cmd!("sha256sum", path).read()?;
        Ok(parse_sum_output(&out))
    } else {
        file_hash_prog::<Sha256>(path, pr)
    }
}

/// The digest from `sha256sum`-style output. GNU coreutils prefixes the line
/// with `\` when the file name contains a backslash or newline.
fn parse_sum_output(out: &str) -> String {
    let digest = out.split_whitespace().next().unwrap();
    digest.trim_start_matches('\\').to_string()
}

pub fn file_hash_prog<D>(path: &Path, pr: Option<&dyn SingleReport>) -> Result<String>
where
    D: Digest,
{
    let mut file = file::open(path)?;
    if let Some(pr) = pr {
        pr.set_length(file.metadata()?.len());
    }
    let mut hasher = D::new();
    let mut buf = [0; 32 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(pr) = pr {
            pr.inc(n as u64);
        }
    }
    let hash = hasher.finalize();
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn hash_blake3_to_str(s: &str) -> String {
    let mut hasher = Blake3Hasher::new();
    hasher.update(s.as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub fn file_hash_blake3(path: &Path, pr: Option<&dyn SingleReport>) -> Result<String> {
    let mut file = file::open(path)?;
    if let Some(pr) = pr {
        pr.set_length(file.metadata()?.len());
    }
    let mut hasher = Blake3Hasher::new();
    let mut buf = [0; 32 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(pr) = pr {
            pr.inc(n as u64);
        }
    }
    let hash = hasher.finalize();
    Ok(format!("{}", hash.to_hex()))
}

/// The algorithms `ensure_checksum` verifies, as written before the `:` in
/// a `<algorithm>:<digest>` checksum.
pub const CHECKSUM_ALGORITHMS: &[&str] = &[
    "blake3", "sha512", "sha256", "sha224", "sha384", "sha1", "md5",
];

/// Whether `checksum` is `<algorithm>:<digest>` with an algorithm
/// `ensure_checksum` can verify.
pub fn is_verifiable_checksum(checksum: &str) -> bool {
    checksum
        .split_once(':')
        .is_some_and(|(algo, _)| CHECKSUM_ALGORITHMS.contains(&algo))
}

/// Whether `checksum` is `<algorithm>:<digest>` with an algorithm
/// `ensure_checksum` can verify and a digest of that algorithm's length in
/// hexadecimal.
pub fn is_well_formed_checksum(checksum: &str) -> bool {
    let Some((algo, digest)) = checksum.split_once(':') else {
        return false;
    };
    let hex_len = match algo {
        "blake3" | "sha256" => 64,
        "sha512" => 128,
        "sha384" => 96,
        "sha224" => 56,
        "sha1" => 40,
        "md5" => 32,
        _ => return false,
    };
    digest.len() == hex_len && digest.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn ensure_checksum(
    path: &Path,
    checksum: &str,
    pr: Option<&dyn SingleReport>,
    algo: &str,
) -> Result<()> {
    let use_external_hasher = file::size(path).unwrap_or(u64::MAX) > 10 * 1024 * 1024;
    let actual = match algo {
        "blake3" => file_hash_blake3(path, pr)?,
        "sha512" => {
            if use_external_hasher && file::which("sha512sum").is_some() {
                let out = cmd!("sha512sum", path).read()?;
                out.split_whitespace().next().unwrap().to_string()
            } else {
                file_hash_prog::<Sha512>(path, pr)?
            }
        }
        "sha256" => file_hash_prog::<Sha256>(path, pr)?,
        "sha224" => file_hash_prog::<Sha224>(path, pr)?,
        "sha384" => file_hash_prog::<Sha384>(path, pr)?,
        "sha1" => {
            if use_external_hasher && file::which("sha1sum").is_some() {
                let out = cmd!("sha1sum", path).read()?;
                out.split_whitespace().next().unwrap().to_string()
            } else {
                file_hash_prog::<Sha1>(path, pr)?
            }
        }
        "md5" => {
            if use_external_hasher && file::which("md5sum").is_some() {
                let out = cmd!("md5sum", path).read()?;
                out.split_whitespace().next().unwrap().to_string()
            } else {
                file_hash_prog::<Md5>(path, pr)?
            }
        }
        _ => bail!("Unknown checksum algorithm: {}", algo),
    };
    let checksum = checksum.to_lowercase();
    if actual != checksum {
        return Err(ChecksumMismatch {
            path: display_path(path),
            algo: algo.to_string(),
            expected: checksum,
            actual,
        }
        .into());
    }
    Ok(())
}

pub fn parse_shasums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            // Strip coreutils binary-mode marker (e.g. "<hash> *file.tar.gz").
            let name = name.strip_prefix('*').unwrap_or(name);
            Some((name.into(), hash.into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sum_output_drops_the_escape_prefix() {
        let digest = "a".repeat(64);
        assert_eq!(parse_sum_output(&format!("{digest}  /tmp/tool\n")), digest);
        assert_eq!(
            parse_sum_output(&format!("\\{digest}  /tmp/my\\\\tool\n")),
            digest
        );
    }

    #[test]
    fn checksum_algorithms_are_the_ones_ensure_checksum_computes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact");
        file::write(&path, b"artifact").unwrap();
        for algo in CHECKSUM_ALGORITHMS {
            let err = ensure_checksum(&path, "00", None, algo).unwrap_err();
            assert!(err.is::<ChecksumMismatch>(), "{algo}: {err}");
            assert!(is_verifiable_checksum(&format!("{algo}:00")));
        }
        assert!(ensure_checksum(&path, "00", None, "no-such-algorithm").is_err());
        assert!(!is_verifiable_checksum("no-such-algorithm:00"));
        assert!(!is_verifiable_checksum("00"));
    }

    #[test]
    fn well_formed_checksums_have_a_full_hex_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact");
        file::write(&path, b"artifact").unwrap();
        for algo in CHECKSUM_ALGORITHMS {
            // the digest `ensure_checksum` computes, which has the length to accept
            let err = ensure_checksum(&path, "00", None, algo).unwrap_err();
            let digest = err
                .downcast_ref::<ChecksumMismatch>()
                .unwrap()
                .actual
                .clone();
            assert!(
                is_well_formed_checksum(&format!("{algo}:{digest}")),
                "{algo}"
            );
            assert!(
                is_well_formed_checksum(&format!("{algo}:{}", digest.to_uppercase())),
                "{algo}"
            );
            assert!(!is_well_formed_checksum(&format!("{algo}:")), "{algo}");
            assert!(!is_well_formed_checksum(&format!("{algo}:00")), "{algo}");
            assert!(
                !is_well_formed_checksum(&format!("{algo}:{}0", digest)),
                "{algo}"
            );
            let not_hex = "z".repeat(digest.len());
            assert!(
                !is_well_formed_checksum(&format!("{algo}:{not_hex}")),
                "{algo}"
            );
        }
        assert!(!is_well_formed_checksum(&format!(
            "crc32:{}",
            "0".repeat(8)
        )));
        assert!(!is_well_formed_checksum(&"0".repeat(64)));
    }

    #[test]
    fn test_corepack_checksum_algorithms() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("package.tgz");
        file::write(&path, b"corepack").unwrap();

        ensure_checksum(
            &path,
            "8aba612a6193520492b81c698962a0d81c3609da7ab498d028ac452e",
            None,
            "sha224",
        )
        .unwrap();
        ensure_checksum(
            &path,
            "cbedf1fe9f759bba045da15cecdfb24340308ffdae357bbb0a10bed2535aa957c7cad13d8081847203cd409a7a7e3cda",
            None,
            "sha384",
        )
        .unwrap();
    }
}
