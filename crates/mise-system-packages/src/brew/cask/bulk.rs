//! Casks from the bulk index (`super::super::bulk`).

use eyre::Result;

use super::super::bulk;
use super::Cask;

/// Resolve one cask from the bulk document, or `None` to ask the per-cask
/// endpoint instead. See `bulk::entry` for why this cannot fail.
pub(super) async fn cask(token: &str) -> Option<Cask> {
    let body = bulk::entry(&bulk::CASKS, token, true).await?;
    match parse_entry(&body, super::super::tag::cask_variation_tag().as_deref()) {
        Ok(cask) => Some(cask),
        Err(err) => {
            debug!(
                "brew-cask: bulk index entry for '{token}' did not parse ({err:#}); falling back to per-cask metadata"
            );
            None
        }
    }
}

/// Build a `Cask` from one indexed entry, as this host sees it.
///
/// Through `cask_from_api_json`, not `from_slice::<Cask>`, and that is the
/// whole point of the function existing. The bulk document carries the same
/// shape as the per-cask endpoint, which means its top-level `url`, `version`
/// and `sha256` describe the newest macOS only, and every other platform takes
/// its values from `variations`. Deserializing an entry straight into `Cask`
/// silently skips that merge, so on an older macOS release the install would
/// take the newest release's URL and checksum, and a cask whose variation nulls
/// those fields would look available on a platform it does not support.
///
/// Errors rather than guessing: `cask()` turns that into a fallback to the
/// per-cask endpoint, which reaches the same answer the slow way.
fn parse_entry(body: &[u8], tag: Option<&str>) -> Result<Cask> {
    super::fetch::cask_from_api_json(serde_json::from_slice(body)?, tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bulk document repeats the per-cask shape, so its top-level fields
    /// describe the newest macOS and every other platform reads `variations`.
    /// An entry deserialized straight into `Cask` skips that, which is how a
    /// host on an older release would install the newest release's build.
    #[test]
    fn an_entry_is_read_as_this_platform_sees_it() {
        const ENTRY: &str = r#"{
          "token":"example","version":"2.0","sha256":"newest",
          "url":"https://example.com/newest.dmg",
          "variations":{
            "arm64_sequoia":{
              "version":"1.0","sha256":"older",
              "url":"https://example.com/older.dmg"
            },
            "x86_64_linux":{"url":null,"version":null}
          }
        }"#;

        // The host's own tag wins over the top level.
        let cask = parse_entry(ENTRY.as_bytes(), Some("arm64_sequoia")).unwrap();
        assert_eq!(cask.version, "1.0");
        assert_eq!(cask.url, "https://example.com/older.dmg");

        // A tag with no entry of its own keeps the top level, which is what the
        // API means by omitting it.
        let newest = parse_entry(ENTRY.as_bytes(), Some("arm64_tahoe")).unwrap();
        assert_eq!(newest.version, "2.0");
        assert_eq!(newest.url, "https://example.com/newest.dmg");

        // A variation that nulls these says "not available here", and must not
        // fall through to the top-level build.
        assert!(parse_entry(ENTRY.as_bytes(), Some("x86_64_linux")).is_err());
    }
}
