use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::Path;

use crate::file;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;
use digest::Digest;
use eyre::{Result, bail};
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use siphasher::sip::SipHasher;

pub fn hash_to_str<T: Hash>(t: &T) -> String {
    let mut s = SipHasher::new();
    t.hash(&mut s);
    format!("{:x}", s.finish())
}

pub fn hash_sha256_to_str(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s);
    format!("{:x}", hasher.finalize())
}

pub fn file_hash_sha256(path: &Path, pr: Option<&Box<dyn SingleReport>>) -> Result<String> {
    let use_external_hasher = file::size(path).unwrap_or_default() > 50 * 1024 * 1024;
    if use_external_hasher && file::which("sha256sum").is_some() {
        let out = cmd!("sha256sum", path).read()?;
        Ok(out.split_whitespace().next().unwrap().to_string())
    } else {
        file_hash_prog::<Sha256>(path, pr)
    }
}

fn file_hash_prog<D>(path: &Path, pr: Option<&Box<dyn SingleReport>>) -> Result<String>
where
    D: Digest + Write,
    D::OutputSize: std::ops::Add,
    <D::OutputSize as std::ops::Add>::Output: digest::generic_array::ArrayLength<u8>,
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
        hasher.write_all(&buf[..n])?;
        if let Some(pr) = pr {
            pr.inc(n as u64);
        }
    }
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(format!("{hash:x}"))
}

pub fn ensure_checksum(
    path: &Path,
    checksum: &str,
    pr: Option<&Box<dyn SingleReport>>,
    algo: &str,
) -> Result<()> {
    let use_external_hasher = file::size(path).unwrap_or(u64::MAX) > 10 * 1024 * 1024;
    let actual = match algo {
        "sha512" => {
            if use_external_hasher && file::which("sha512sum").is_some() {
                let out = cmd!("sha512sum", path).read()?;
                out.split_whitespace().next().unwrap().to_string()
            } else {
                file_hash_prog::<Sha512>(path, pr)?
            }
        }
        "sha256" => file_hash_prog::<Sha256>(path, pr)?,
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
        bail!(
            "Checksum mismatch for file {}:\nExpected: {algo}:{checksum}\nActual:   {algo}:{actual}",
            display_path(path)
        );
    }
    Ok(())
}

pub fn parse_shasums(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(|l| {
            let mut parts = l.split_whitespace();
            let hash = parts.next().unwrap();
            let name = parts.next().unwrap();
            (name.into(), hash.into())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_hash_to_str() {
        let _config = Config::get().await.unwrap();
        assert_eq!(hash_to_str(&"foo"), "e1b19adfb2e348a2");
    }

    #[tokio::test]
    async fn test_hash_sha256() {
        let _config = Config::get().await.unwrap();
        let path = Path::new(".test-tool-versions");
        let hash = file_hash_prog::<Sha256>(path, None).unwrap();
        assert_snapshot!(hash);
    }
}
