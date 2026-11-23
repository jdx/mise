use mlua::{ExternalResult, Lua, Table};
use std::path::Path;

const SUPPORTED: &str = "sha256, sha512, sha1 or md5";

pub(crate) fn mod_checksum(lua: &Lua) -> mlua::Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let checksum = lua.create_table_from(vec![
        ("digest", lua.create_function(digest)?),
        ("verify", lua.create_function(verify)?),
    ])?;
    loaded.set("checksum", checksum.clone())?;
    loaded.set("vfox.checksum", checksum)?;
    Ok(())
}

/// Hash a file with `algo` and return the digest as lowercase hex.
fn hash_file(path: &str, algo: &str) -> mlua::Result<String> {
    let path = Path::new(path);
    match algo {
        "sha256" => xx::hash::file_hash_sha256(path),
        "sha512" => xx::hash::file_hash_sha512(path),
        "sha1" => xx::hash::file_hash_sha1(path),
        "md5" => xx::hash::file_hash_md5(path),
        _ => {
            return Err(mlua::Error::external(format!(
                "unsupported checksum algorithm {algo:?}: use {SUPPORTED}"
            )));
        }
    }
    .into_lua_err()
}

fn digest(_lua: &Lua, (path, algo): (String, Option<String>)) -> mlua::Result<String> {
    hash_file(
        &path,
        &algo.unwrap_or_else(|| "sha256".to_string()).to_lowercase(),
    )
}

fn verify(_lua: &Lua, (path, expected): (String, String)) -> mlua::Result<bool> {
    let Some((algo, hex)) = expected.trim().split_once(':') else {
        return Err(mlua::Error::external(format!(
            "invalid checksum {expected:?}: expected \"<algorithm>:<hex digest>\""
        )));
    };
    let algo = algo.to_lowercase();
    // Published checksum files are inconsistent about case; the digest is hex.
    let hex = hex.to_lowercase();
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(mlua::Error::external(format!(
            "invalid checksum {expected:?}: the digest must be hexadecimal"
        )));
    }
    let actual = hash_file(&path, &algo)?;
    if actual != hex {
        return Err(mlua::Error::external(format!(
            "Checksum mismatch for file {path}:\nExpected: {algo}:{hex}\nActual:   {algo}:{actual}"
        )));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SHA256: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    const SHA512: &str = "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f";
    const SHA1: &str = "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed";
    const MD5: &str = "5eb63bbbe01eeed093cb22bb8f5acdc3";

    fn hello_world() -> (tempfile::TempDir, String) {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("hello.txt");
        fs::write(&path, "hello world").unwrap();
        let path = path.to_string_lossy().to_string();
        (temp_dir, path)
    }

    fn lua() -> Lua {
        let lua = Lua::new();
        mod_checksum(&lua).unwrap();
        lua
    }

    #[test]
    fn test_digest_computes_every_algorithm_as_lowercase_hex() {
        let (_dir, path) = hello_world();
        lua()
            .load(mlua::chunk! {
                local checksum = require("checksum")
                assert(checksum.digest($path) == $SHA256, "sha256 is the default")
                assert(checksum.digest($path, "sha256") == $SHA256)
                assert(checksum.digest($path, "SHA512") == $SHA512, "the algorithm is case-insensitive")
                assert(checksum.digest($path, "sha1") == $SHA1)
                assert(checksum.digest($path, "md5") == $MD5)
            })
            .exec()
            .unwrap();
    }

    #[test]
    fn test_digest_rejects_an_unsupported_algorithm_and_a_missing_file() {
        let (dir, path) = hello_world();
        let missing = dir.path().join("missing").to_string_lossy().to_string();
        lua()
            .load(mlua::chunk! {
                local checksum = require("checksum")
                local ok, err = pcall(checksum.digest, $path, "crc32")
                assert(not ok and tostring(err):find("unsupported checksum algorithm", 1, true), tostring(err))
                ok, err = pcall(checksum.digest, $missing, "sha256")
                assert(not ok, "a missing file raises")
            })
            .exec()
            .unwrap();
    }

    #[test]
    fn test_verify_accepts_a_match_in_any_case() {
        let (_dir, path) = hello_world();
        let upper = format!("SHA256:{}", SHA256.to_uppercase());
        let sha1 = format!("sha1:{SHA1}");
        let sha256 = format!("sha256:{SHA256}");
        lua()
            .load(mlua::chunk! {
                local checksum = require("checksum")
                assert(checksum.verify($path, $sha256) == true)
                assert(checksum.verify($path, $upper) == true)
                assert(checksum.verify($path, $sha1) == true)
            })
            .exec()
            .unwrap();
    }

    #[test]
    fn test_verify_raises_on_a_mismatch_naming_both_digests() {
        let (_dir, path) = hello_world();
        let wrong = format!("sha256:{}", "0".repeat(64));
        lua()
            .load(mlua::chunk! {
                local checksum = require("checksum")
                local ok, err = pcall(checksum.verify, $path, $wrong)
                assert(not ok, "a mismatch must raise")
                err = tostring(err)
                assert(err:find("Checksum mismatch", 1, true), err)
                assert(err:find("Expected: " .. $wrong, 1, true), err)
                assert(err:find("Actual:   sha256:" .. $SHA256, 1, true), err)
            })
            .exec()
            .unwrap();
    }

    #[test]
    fn test_verify_rejects_a_malformed_expectation() {
        let (_dir, path) = hello_world();
        lua()
            .load(mlua::chunk! {
                local checksum = require("checksum")
                for _, bad in ipairs({ $SHA256, "sha256:", "sha256:xyz", "crc32:abcd" }) do
                    local ok = pcall(checksum.verify, $path, bad)
                    assert(not ok, "expected " .. bad .. " to raise")
                end
            })
            .exec()
            .unwrap();
    }
}
