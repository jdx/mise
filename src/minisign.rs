use crate::*;
use minisign_verify::*;
use std::io::Read;
use std::iter::Iterator;
use std::path::Path;
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

/// [`verify`] for the file at `path`, hashed in chunks so a file of any size
/// is checked without being held in memory. Like `verify`, it accepts only a
/// prehashed signature.
pub fn verify_file(pub_key: &str, path: &Path, sig: &str) -> Result<()> {
    let public_key = PublicKey::from_base64(pub_key)?;
    let signature = Signature::decode(sig)?;
    let mut verifier = public_key.verify_stream(&signature)?;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        verifier.update(&buf[..n]);
    }
    verifier.finalize()?;
    Ok(())
}
