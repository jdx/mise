use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::Path;

use eyre::Result;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::file::display_path;

pub fn hash_to_str<T: Hash>(t: &T) -> String {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    let bytes = s.finish();
    format!("{bytes:x}")
}

pub fn file_hash_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(format!("{hash:x}"))
}

pub fn ensure_checksum_sha256(path: &Path, checksum: &str) -> Result<()> {
    let actual = file_hash_sha256(path)?;
    ensure!(
        actual == checksum,
        "Checksum mismatch for file {}:\nExpected: {checksum}\nActual:   {actual}",
        display_path(path),
    );
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

    use super::*;

    #[test]
    fn test_hash_to_str() {
        assert_eq!(hash_to_str(&"foo"), "3e8b8c44c3ca73b7");
    }

    #[test]
    fn test_hash_sha256() {
        let path = Path::new(".test-tool-versions");
        let hash = file_hash_sha256(path).unwrap();
        assert_snapshot!(hash);
    }
}
