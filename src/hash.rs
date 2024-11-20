use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::Path;

use crate::file;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;
use digest::Digest;
use eyre::{bail, Result};
use md5::Md5;
use rayon::prelude::*;
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

pub fn file_hash_sha256(path: &Path) -> Result<String> {
    file_hash_prog::<Sha256>(path, None)
}

pub fn file_hash_prog<D>(path: &Path, pr: Option<&dyn SingleReport>) -> Result<String>
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
    pr: Option<&dyn SingleReport>,
    algo: &str,
) -> Result<()> {
    let actual = match algo {
        "sha512" => file_hash_prog::<Sha512>(path, pr)?,
        "sha256" => file_hash_prog::<Sha256>(path, pr)?,
        "sha1" => file_hash_prog::<Sha1>(path, pr)?,
        "md5" => file_hash_prog::<Md5>(path, pr)?,
        _ => bail!("Unknown checksum algorithm: {}", algo),
    };
    if actual != checksum {
        bail!("Checksum mismatch for file {}:\nExpected: {algo}:{checksum}\nActual:   {algo}:{actual}",
        display_path(path));
    }
    Ok(())
}

pub fn parse_shasums(text: &str) -> HashMap<String, String> {
    text.par_lines()
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
    use test_log::test;

    use crate::test::reset;

    use super::*;

    #[test]
    fn test_hash_to_str() {
        assert_eq!(hash_to_str(&"foo"), "e1b19adfb2e348a2");
    }

    #[test]
    fn test_hash_sha256() {
        reset();
        let path = Path::new(".test-tool-versions");
        let hash = file_hash_sha256(path).unwrap();
        assert_snapshot!(hash);
    }
}
