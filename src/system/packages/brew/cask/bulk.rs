//! The bulk cask index.
//!
//! Homebrew publishes every cask in one static document, `api/cask.json`.
//! Reading that once beats asking for `api/cask/<token>.json` per cask: a
//! machine declaring 150 casks paid 150 sequential round trips to answer a
//! question one request answers.
//!
//! The document is ~19MB, so it is not parsed per lookup. Alongside it we keep a
//! sidecar mapping each token to a byte range, and a lookup seeks to that range
//! and parses a few hundred bytes. Homebrew does the same thing for its own
//! bundle (`.payload.index`, entries like `"0-ad": [7422487, 596]`).
//!
//! Deliberately mise's own cache rather than Homebrew's. Homebrew keeps a signed
//! JWS bundle under a per-architecture name it has changed at least once, and
//! validates it against a size and mtime it records itself, so both reading that
//! file and writing beside it would couple correctness here to another tool's
//! private layout. Re-fetching ~19MB per staleness window is the cheaper trade.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use eyre::{Result, WrapErr};
use reqwest::header::{HeaderMap, HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED};
use serde::{Deserialize, Serialize};

use super::model::Cask;
use crate::http::HTTP_FETCH;

const BULK_URL: &str = "https://formulae.brew.sh/api/cask.json";

/// How long the cached document is trusted without asking upstream. Matches
/// Homebrew's own default (`HOMEBREW_API_AUTO_UPDATE_SECS`, 24h): within the
/// window there is no request at all, and after it a conditional request is
/// usually a 304 carrying no body.
const STALE_AFTER: Duration = Duration::from_secs(86_400);

/// Bumped when the sidecar's meaning changes, so an older one is rebuilt rather
/// than misread.
const INDEX_VERSION: u32 = 1;

fn dir() -> PathBuf {
    crate::dirs::CACHE.join("system-brew").join("api")
}

fn document_path() -> PathBuf {
    dir().join("cask.json")
}

fn index_path() -> PathBuf {
    dir().join("cask.index.json")
}

#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    /// Size and mtime of the document this index was built from. An index that
    /// does not describe the document sitting next to it is worse than no index:
    /// its offsets would slice the wrong bytes, so a mismatch rebuilds.
    source_size: u64,
    source_mtime_ns: u128,
    /// token -> (offset, length) into the document.
    casks: HashMap<String, (u64, u64)>,
    /// Old tokens and aliases resolve to the same range as their current token,
    /// so a renamed cask keeps working without a second lookup.
    aliases: HashMap<String, String>,
}

fn stat(path: &Path) -> Result<(u64, u128)> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok((meta.len(), mtime))
}

fn is_fresh(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() == 0 {
        return false;
    }
    meta.modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age < STALE_AFTER)
}

/// Fetch the document if the cached copy is missing or past the staleness
/// window. A 304 leaves the bytes alone and only restamps the mtime, which is
/// what makes the next `is_fresh` check pass without another request.
async fn refresh() -> Result<()> {
    let path = document_path();
    if is_fresh(&path) {
        return Ok(());
    }

    let mut headers = HeaderMap::new();
    // Ask conditionally when there is something to compare against. Sending the
    // stored Last-Modified rather than an ETag keeps the sidecar simple; the
    // server offers both and either yields a 304.
    if let Some(stamp) = std::fs::read_to_string(dir().join("cask.last-modified"))
        .ok()
        .filter(|_| path.exists())
        && let Ok(value) = HeaderValue::from_str(stamp.trim())
    {
        headers.insert(IF_MODIFIED_SINCE, value);
    }

    let resp = HTTP_FETCH
        .get_async_with_headers_allow_error_status(BULK_URL, &headers)
        .await
        .wrap_err("failed to fetch the Homebrew cask index")?;

    if resp.status() == reqwest::StatusCode::NOT_MODIFIED && path.exists() {
        // Unchanged upstream: restamp so the window restarts, and keep the
        // index, which still describes these exact bytes.
        let now = filetime::FileTime::now();
        filetime::set_file_mtime(&path, now).ok();
        if let Ok((size, mtime)) = stat(&path)
            && let Ok(mut index) = load_index_file()
        {
            index.source_mtime_ns = mtime;
            index.source_size = size;
            write_index(&index).ok();
        }
        return Ok(());
    }

    resp.error_for_status_ref()
        .wrap_err("failed to fetch the Homebrew cask index")?;
    let last_modified = resp
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = resp.bytes().await?;

    crate::file::create_dir_all(dir())?;
    // Write through a temporary file: a half-written document would be indexed
    // as if complete, and every later lookup would slice garbage out of it.
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, &path)?;
    if let Some(stamp) = last_modified {
        std::fs::write(dir().join("cask.last-modified"), stamp).ok();
    }

    let index = build_index(&body)?;
    write_index(&index)?;
    Ok(())
}

fn write_index(index: &Index) -> Result<()> {
    crate::file::create_dir_all(dir())?;
    let tmp = index_path().with_extension("json.part");
    std::fs::write(&tmp, serde_json::to_vec(index)?)?;
    std::fs::rename(&tmp, index_path())?;
    Ok(())
}

fn load_index_file() -> Result<Index> {
    let raw = std::fs::read(index_path())?;
    Ok(serde_json::from_slice(&raw)?)
}

/// Load the sidecar, rebuilding it when it does not describe the document next
/// to it.
fn load_index() -> Result<Index> {
    let (size, mtime) = stat(&document_path())?;
    if let Ok(index) = load_index_file()
        && index.version == INDEX_VERSION
        && index.source_size == size
        && index.source_mtime_ns == mtime
    {
        return Ok(index);
    }
    let body = std::fs::read(document_path())?;
    let mut index = build_index(&body)?;
    index.source_size = size;
    index.source_mtime_ns = mtime;
    write_index(&index)?;
    Ok(index)
}

/// Walk the top-level array and record each element's byte range.
///
/// A streaming scan rather than a `serde_json::Value` parse: the whole point is
/// to avoid holding ~19MB of parsed JSON, and only the `token`, `aliases` and
/// `old_tokens` of each element are needed to build the map.
fn build_index(body: &[u8]) -> Result<Index> {
    #[derive(Deserialize)]
    struct Head {
        token: String,
        #[serde(default)]
        aliases: Vec<String>,
        #[serde(default)]
        old_tokens: Vec<String>,
    }

    let mut casks = HashMap::new();
    let mut aliases = HashMap::new();

    for (start, end) in top_level_elements(body)? {
        let slice = &body[start..end];
        // A cask whose shape this version does not understand is skipped rather
        // than failing the whole index: the per-cask path still resolves it.
        let Ok(head) = serde_json::from_slice::<Head>(slice) else {
            continue;
        };
        for alias in head.aliases.iter().chain(head.old_tokens.iter()) {
            aliases.insert(alias.clone(), head.token.clone());
        }
        casks.insert(head.token, (start as u64, (end - start) as u64));
    }

    if casks.is_empty() {
        eyre::bail!("the Homebrew cask index parsed to zero casks");
    }

    Ok(Index {
        version: INDEX_VERSION,
        source_size: 0,
        source_mtime_ns: 0,
        casks,
        aliases,
    })
}

/// Byte ranges of each element in a top-level JSON array.
///
/// Tracks string state so a brace or bracket inside a value (a description, a
/// URL with a `}` in it) does not close an object early.
fn top_level_elements(body: &[u8]) -> Result<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    let mut iter = body
        .iter()
        .enumerate()
        .skip_while(|(_, b)| b.is_ascii_whitespace());
    match iter.next() {
        Some((_, b'[')) => {}
        _ => eyre::bail!("the Homebrew cask index is not a JSON array"),
    }

    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in iter {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' | b']' => {
                depth = match depth.checked_sub(1) {
                    Some(d) => d,
                    // The closing bracket of the outer array.
                    None => break,
                };
                if depth == 0
                    && let Some(s) = start.take()
                {
                    out.push((s, i + 1));
                }
            }
            _ => {}
        }
    }

    Ok(out)
}

/// Resolve one cask from the bulk document.
///
/// `Ok(None)` means the index does not carry this token, which is a normal
/// answer for a tap cask or a token this snapshot predates; the caller falls
/// back to the per-cask request.
pub(super) async fn cask(token: &str) -> Result<Option<Cask>> {
    if let Err(err) = refresh().await {
        // An unreachable index is not fatal: the per-cask path still works, and
        // failing here would turn a cache miss into a failed run.
        debug!("brew-cask: bulk index unavailable ({err:#}); falling back to per-cask metadata");
        return Ok(None);
    }
    let index = match load_index() {
        Ok(index) => index,
        Err(err) => {
            debug!("brew-cask: bulk index unreadable ({err:#}); falling back to per-cask metadata");
            return Ok(None);
        }
    };

    let canonical = index
        .aliases
        .get(token)
        .map(String::as_str)
        .unwrap_or(token);
    let Some(&(offset, len)) = index.casks.get(canonical) else {
        return Ok(None);
    };

    let body = read_range(&document_path(), offset, len)?;
    let cask: Cask = serde_json::from_slice(&body)
        .wrap_err_with(|| format!("invalid metadata for cask '{token}' in the bulk index"))?;
    Ok(Some(cask))
}

fn read_range(path: &Path, offset: u64, len: u64) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len as usize];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"[
      {"token":"alpha","version":"1.0","desc":"braces } and ] inside a string"},
      {"token":"beta","version":"2.0","aliases":["beta-alias"],"old_tokens":["beta-old"]},
      {"token":"gamma","version":"3.0","artifacts":[{"app":["Gamma.app"]}]}
    ]"#;

    #[test]
    fn element_ranges_survive_punctuation_in_strings() {
        let ranges = top_level_elements(DOC.as_bytes()).unwrap();
        assert_eq!(ranges.len(), 3, "one range per cask");
        for (start, end) in ranges {
            let slice = &DOC.as_bytes()[start..end];
            // Each range must be independently parseable; that is the property
            // every lookup depends on.
            serde_json::from_slice::<serde_json::Value>(slice)
                .expect("each element parses on its own");
        }
    }

    #[test]
    fn nested_arrays_do_not_split_an_element() {
        let ranges = top_level_elements(DOC.as_bytes()).unwrap();
        let (start, end) = ranges[2];
        let value: serde_json::Value = serde_json::from_slice(&DOC.as_bytes()[start..end]).unwrap();
        assert_eq!(value["token"], "gamma");
        assert_eq!(value["artifacts"][0]["app"][0], "Gamma.app");
    }

    #[test]
    fn index_maps_tokens_aliases_and_old_tokens() {
        let index = build_index(DOC.as_bytes()).unwrap();
        assert_eq!(index.casks.len(), 3);
        assert_eq!(
            index.aliases.get("beta-alias").map(String::as_str),
            Some("beta")
        );
        assert_eq!(
            index.aliases.get("beta-old").map(String::as_str),
            Some("beta")
        );

        // The recorded range must slice the cask it claims to.
        let &(offset, len) = index.casks.get("beta").unwrap();
        let slice = &DOC.as_bytes()[offset as usize..(offset + len) as usize];
        let value: serde_json::Value = serde_json::from_slice(slice).unwrap();
        assert_eq!(value["token"], "beta");
    }

    #[test]
    fn a_non_array_document_is_rejected() {
        assert!(top_level_elements(br#"{"token":"alpha"}"#).is_err());
    }

    #[test]
    fn an_empty_array_indexes_to_an_error_rather_than_an_empty_index() {
        // An empty index would silently answer "not in Homebrew" for every cask.
        assert!(build_index(b"[]").is_err());
    }
}
