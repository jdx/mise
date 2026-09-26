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
