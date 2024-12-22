use crate::*;
use minisign_verify::*;

pub const MISE_PUB_KEY: &str = include_str!("../minisign.pub");

pub fn verify(pub_key: &str, data: &[u8], sig: &str) -> Result<()> {
    let public_key = PublicKey::from_base64(pub_key)?;
    let signature = Signature::decode(sig)?;
    public_key.verify(data, &signature, false)?;
    Ok(())
}
