use crate::*;
use minisign_verify::*;
#[allow(dead_code)]
pub const MISE_PUB_KEY: &str = include_str!("../minisign.pub");

#[allow(dead_code)]
pub fn verify(pub_key: &str, data: &str, sig: &str) -> Result<()> {
    let public_key = PublicKey::from_base64(pub_key)?;
    let signature = Signature::decode(sig)?;
    public_key.verify(data.as_bytes(), &signature, false)?;
    Ok(())
}
