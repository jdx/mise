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
use tokio::sync::OnceCell;

use super::model::Cask;
use crate::config::Settings;
use crate::http::HTTP;

const BULK_URL: &str = "https://formulae.brew.sh/api/cask.json";

/// How long the cached document is trusted without asking upstream.
///
/// Homebrew's `HOMEBREW_API_AUTO_UPDATE_SECS` default, which is 450s and not the
/// 24h of the similarly named `HOMEBREW_AUTO_UPDATE_SECS`. The distinction
/// matters: mise resolves metadata afresh on every run today, so a window of a
/// day would be a real regression rather than a cache. It hides a version
/// published this morning, and it breaks any cask whose vendor drops the old
/// download URL on release, which is common enough to be the usual reason a
/// cask install fails.
///
/// The shorter window is close to free. Inside it there is no request at all,
/// and past it the conditional request is almost always a 304 carrying no body,
/// so the cost is one small round trip per run rather than one per cask.
const STALE_AFTER: Duration = Duration::from_secs(450);

/// How old a cached document may be and still answer a lookup whose refresh
/// failed.
///
/// Reaching for yesterday's bytes when today's fetch failed is right, but not
/// forever, because not every refresh failure heals on its own. A document
/// whose shape `build_index` no longer understands fails identically on every
/// run, and so does a permanently retired endpoint, while the per-cask endpoint
/// would have answered correctly the whole time. Unbounded, that is precisely
/// the silent staleness `STALE_AFTER` exists to prevent, just reached by a
/// different route: an install failing because the vendor dropped the download
/// URL the cached version names.
///
/// Generous, because the bound is there to stop indefinite drift rather than to
/// second-guess a bad afternoon upstream. Past it, lookups fall back to the
/// per-cask endpoint: slower, and correct.
const FALLBACK_AFTER: Duration = Duration::from_secs(7 * 86_400);

/// Total wall clock allowed for one refresh, retries and body included.
///
/// The fetch client's 20s per-request cap was wrong for a ~19MB body, but
/// removing it leaves nothing bounding a mirror that dribbles bytes slowly
/// enough to keep resetting `read_timeout` and never finishes. This is an
/// optimization that must not hold up package resolution, and the bound follows
/// from that: resolving 150 casks one request at a time takes well under a
/// minute, so a bulk fetch still running after two has already lost to the path
/// it replaces, whatever it eventually returns. Past it the refresh is abandoned
/// and lookups fall back.
///
/// Clamped by `http_download_timeout` so that someone who has deliberately
/// tightened downloads is not overridden by this.
const REFRESH_DEADLINE: Duration = Duration::from_secs(120);

/// Bumped when the sidecar's meaning changes, so an older one is rebuilt rather
/// than misread.
const INDEX_VERSION: u32 = 1;

/// Cache directory for the bulk document, keyed to where the document actually
/// came from.
///
/// `url_replacements` can point mise at a mirror or, in the e2e suite, at a
/// local fixture. Those are different documents, so they cannot share a cache
/// file: one run would otherwise answer from bytes another run fetched
/// somewhere else. The canonical endpoint keeps the plain path so the common
/// case stays readable; anything redirected gets its own directory.
fn dir() -> PathBuf {
    let base = crate::dirs::CACHE.join("system-brew").join("api");
    let mut url = match url::Url::parse(BULK_URL) {
        Ok(url) => url,
        Err(_) => return base,
    };
    crate::http::apply_url_replacements(&mut url);
    if url.as_str() == BULK_URL {
        return base;
    }
    let digest = crate::hash::hash_sha256_to_str(url.as_str());
    base.join(format!("redirected-{}", &digest[..16]))
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

/// When upstream was last asked, tracked separately from the document itself.
///
/// The document's own mtime cannot carry this. It is half of the index's
/// identity check, so restamping it to restart the staleness window would either
/// invalidate a perfectly good index or, worse, have to be papered over by
/// copying the new stamp into the index and asserting agreement that was never
/// verified.
fn checked_path() -> PathBuf {
    dir().join("cask.last-checked")
}

/// An HTTP date, for ordering one generation of the document against another.
///
/// `parse_from_rfc2822` is what `src/http.rs` already uses on `Last-Modified`:
/// the IMF-fixdate HTTP sends parses as RFC 2822.
fn parse_http_date(raw: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc2822(raw.trim()).ok()
}

/// The `Last-Modified` belonging to the document currently on disk.
///
/// Gated on the document actually being there: a validator with no document
/// describes nothing, and treating it as a generation to compare against would
/// let a leftover file suppress a real publication.
fn stored_last_modified() -> Option<chrono::DateTime<chrono::FixedOffset>> {
    if !usable_document() {
        return None;
    }
    let raw = std::fs::read_to_string(dir().join("cask.last-modified")).ok()?;
    parse_http_date(&raw)
}

/// Whether a file can actually be created in the cache directory.
///
/// `create_dir_all` returns `Ok` the moment the directory exists, which says
/// nothing about being allowed to put anything in it, so on a read-only cache
/// it is not a preflight at all. The probe is named per process so two mise
/// runs cannot delete each other's.
///
/// Deliberately not a test for free space: an empty file needs no data blocks,
/// so a full disk passes this and fails later at `write_atomic`, which leaves
/// the previous document in place and is handled like any other refresh
/// failure.
fn cache_is_writable() -> bool {
    let probe = dir().join(format!("cask.probe-{}", std::process::id()));
    let writable = std::fs::write(&probe, []).is_ok();
    let _ = std::fs::remove_file(&probe);
    writable
}

/// Whether there is a document on disk worth reading or revalidating.
///
/// An empty file is not one. It is the shape a truncated write or an
/// out-of-space failure leaves behind, and every caller that treats it as a
/// document (a conditional request, a freshness check) ends up confirming
/// something unreadable rather than replacing it.
fn usable_document() -> bool {
    std::fs::metadata(document_path()).is_ok_and(|m| m.len() > 0)
}

/// Fresh means "asked upstream recently AND still have a document to read".
fn is_fresh() -> bool {
    if !usable_document() {
        return false;
    }
    std::fs::metadata(checked_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age < STALE_AFTER)
}

/// Restart the staleness window without touching the document.
fn mark_checked() -> Result<()> {
    crate::file::create_dir_all(dir())?;
    crate::file::write_atomic(checked_path(), [])?;
    Ok(())
}

/// Whether this process has already attempted a refresh.
///
/// `cask()` runs once per declared cask, and each call used to re-enter
/// `refresh()`. That is free on the happy path, where `is_fresh()`
/// short-circuits immediately, but a refresh that *fails* leaves nothing behind
/// recording the attempt. A 200 whose body `build_index` rejects (an upstream
/// format change, a captive portal, a proxy serving HTML) returns before
/// `mark_checked()`, so the next cask downloads the same ~19MB again: a machine
/// declaring 150 casks would fetch it 150 times, which is far worse than the
/// per-cask requests this is meant to replace. A mirror without `cask.json`, or
/// an outage, merely doubles the request count instead.
///
/// Memoizing the attempt makes one failure cost one attempt.
static REFRESHED: OnceCell<bool> = OnceCell::const_new();

/// Attempt a refresh at most once per process, logging a failure once rather
/// than once per cask. Returns whether it succeeded.
async fn refresh_once() -> bool {
    *REFRESHED
        .get_or_init(|| async {
            let deadline = REFRESH_DEADLINE.min(Settings::get().http_download_timeout());
            match tokio::time::timeout(deadline, refresh()).await {
                Ok(Ok(())) => true,
                Err(_) => {
                    debug!(
                        "brew-cask: bulk index refresh exceeded {deadline:?}; falling back to per-cask metadata"
                    );
                    false
                }
                Ok(Err(err)) => {
                    debug!(
                        "brew-cask: bulk index refresh failed ({err:#}); falling back to the cached document if it is recent enough"
                    );
                    false
                }
            }
        })
        .await
}

/// Whether a document left by an earlier run may answer for a failed refresh.
///
/// Measured from `checked_path()`, which only moves when upstream actually
/// answered, so this is the age of the last successful check rather than of the
/// last attempt. No stamp at all means nothing was ever fetched here, which is
/// not a stale cache but an absent one.
fn within_fallback_window() -> bool {
    std::fs::metadata(checked_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|stamp| SystemTime::now().duration_since(stamp).ok())
        .is_some_and(|age| age < FALLBACK_AFTER)
}

/// Fetch the document if the cached copy is missing or past the staleness
/// window. A 304 leaves both the bytes and their index alone and only restarts
/// the window.
async fn refresh() -> Result<()> {
    let path = document_path();
    if is_fresh() {
        return Ok(());
    }

    // Before the request, not after it. Every byte downloaded into a cache that
    // cannot be written is waste paid again by the next process, and nothing is
    // learned afterwards that was not knowable beforehand. Failing here sends
    // every lookup to the per-cask endpoint, which is what mise did before this
    // cache existed.
    crate::file::create_dir_all(dir()).wrap_err("the Homebrew cask index cache is not writable")?;
    eyre::ensure!(
        cache_is_writable(),
        "the Homebrew cask index cache directory is not writable"
    );

    let mut headers = HeaderMap::new();
    // Ask conditionally when there is something to compare against. Sending the
    // stored Last-Modified rather than an ETag keeps the sidecar simple; the
    // server offers both and either yields a 304.
    //
    // `usable_document()` rather than `path.exists()`, and for the same reason
    // `is_fresh` does not count an empty file as fresh. A zero-length or
    // otherwise damaged local copy alongside a surviving `cask.last-modified`
    // would otherwise earn a 304 on every run: the stamp renews, the document
    // stays broken, `load_index` rebuilds and bails, and the cache never repairs
    // itself while still costing a round trip per process. Asking
    // unconditionally gets a 200 and a real document back.
    if let Some(stamp) = std::fs::read_to_string(dir().join("cask.last-modified"))
        .ok()
        .filter(|_| usable_document())
        && let Ok(value) = HeaderValue::from_str(stamp.trim())
    {
        headers.insert(IF_MODIFIED_SINCE, value);
    }

    // `HTTP`, not `HTTP_FETCH`. The fetch client is for version lookups, and it
    // is the one client kind that puts a single timeout around the whole
    // request, body included: 20s by default, which is a budget for a few KB of
    // JSON and not for ~19MB (2MB gzipped). A link that cannot move that in 20s
    // is not exotic (tethered, hotel wifi, a throttled office egress), and
    // because a timeout counts as transient the whole download would then be
    // retried `http_retries` times, turning one slow request into a minute of
    // stalling before falling back to the per-cask endpoint that would have
    // worked. `HTTP` applies `connect_timeout` and `read_timeout` instead, so a
    // dead connection still fails promptly while a slow but progressing
    // download is allowed to finish.
    let resp = HTTP
        .get_async_with_headers_allow_error_status(BULK_URL, &headers)
        .await
        .wrap_err("failed to fetch the Homebrew cask index")?;

    if resp.status() == reqwest::StatusCode::NOT_MODIFIED && usable_document() {
        // Unchanged upstream, so the document and its index are both still
        // whatever they were. Only the window restarts.
        //
        // Nothing is stamped onto the index here on purpose. Copying the
        // document's fingerprint into the sidecar would make `load_index`
        // accept it without ever having checked that it describes this
        // document: if an earlier refresh died between promoting the document
        // and writing the index, that assertion would be false, and the wrong
        // offsets would be trusted for a renewed 24 hours. Leaving both alone
        // means `load_index` still decides on the evidence, and rebuilds if
        // they disagree.
        mark_checked_best_effort();
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

    // Index BEFORE promoting. A readable but malformed 200 that replaced the
    // cached document would still count as fresh, so every lookup would fall
    // back to a per-cask request for the whole staleness window: slower than
    // having never cached anything, and silent. Failing here instead leaves the
    // last known-good document in place.
    let mut index = build_index(&body)?;

    crate::file::create_dir_all(dir())?;

    // Publishing the document and its index is one operation and has to be
    // serialized. Interleaved, two refreshes can install different bodies and
    // then stamp one body's index with the other body's size and mtime, which
    // `load_index` accepts because they agree: the offsets then slice the wrong
    // document. The lock is held across the synchronous publication only, not
    // across the download above, so a second process waits a moment rather than
    // blocking on a ~19MB transfer.
    let _lock = crate::lock_file::LockFile::at(&dir().join("cask.lock")).lock()?;

    // Re-check under the lock, because the wait for it can be long enough for
    // the answer to have changed. Two processes both past the window download
    // concurrently; if upstream publishes between their two responses, the one
    // holding the OLDER body can take the lock second and overwrite the newer
    // one. Nothing downstream would notice: it replaces the document, its index
    // and the validator together, so they agree with each other and
    // `load_index` accepts them, and the window then serves that older
    // generation until it expires.
    //
    // Compared on the validator, not on the freshness stamp. A fresh stamp only
    // says somebody spoke to upstream, and a 304 stamps too: a process whose
    // cached copy was still current when it asked renews the window without
    // publishing anything. Deciding on the stamp would make that 304 discard
    // THIS newer body and keep the older one for another window, which is the
    // very thing being fixed here, just with the processes swapped.
    //
    // Falls through when either side is missing or unparseable, which is what
    // this did before: a server sending no `Last-Modified` gives nothing to
    // order by, and publishing is the better guess than silently keeping
    // whatever happens to be on disk.
    if let Some(stored) = stored_last_modified()
        && let Some(fetched) = last_modified.as_deref().and_then(parse_http_date)
        && stored >= fetched
    {
        debug!(
            "brew-cask: another process published a bulk index at least as new; keeping its copy"
        );
        // Their document is current, so the window is as much theirs to restart
        // as ours: not stamping would send the next run back to upstream for a
        // document it already has.
        mark_checked_best_effort();
        return Ok(());
    }

    crate::file::write_atomic(&path, &body)?;
    if let Some(stamp) = last_modified {
        crate::file::write_atomic(dir().join("cask.last-modified"), stamp).ok();
    }

    // Stamp from the promoted file rather than leaving zeros: `load_index`
    // rejects an index whose recorded size and mtime do not match the document,
    // so writing zeros here would make the very next lookup rescan all ~19MB
    // and write the index a second time.
    let (size, mtime) = stat(&path)?;
    index.source_size = size;
    index.source_mtime_ns = mtime;
    write_index(&index)?;
    // Last, so that a refresh which dies partway leaves the cache stale rather
    // than fresh-but-unindexed: the next run retries instead of trusting it.
    mark_checked_best_effort();
    Ok(())
}

/// Stamp the window, and treat failing to do so as not worth failing over.
///
/// The document and its index are already published by the time this runs, so
/// the only consequence of no stamp is asking upstream again next time, which is
/// safe. Propagating the error instead would make `refresh` report failure for a
/// cache it just wrote correctly, and on a cold cache that is actively harmful:
/// `within_fallback_window` finds no stamp at all, so every lookup discards the
/// document this call just downloaded and falls back, and the next process
/// repeats the whole download.
fn mark_checked_best_effort() {
    if let Err(err) = mark_checked() {
        debug!(
            "brew-cask: could not stamp the bulk index check time ({err:#}); will re-check next run"
        );
    }
}

fn write_index(index: &Index) -> Result<()> {
    crate::file::create_dir_all(dir())?;
    // `write_atomic` rather than a fixed `.part` name plus rename: concurrent
    // mise processes would otherwise share one temporary path and clobber each
    // other's partial writes. It also carries the Windows sharing-violation
    // retry that a raw rename does not.
    crate::file::write_atomic(index_path(), serde_json::to_vec(index)?)?;
    Ok(())
}

fn load_index_file() -> Result<Index> {
    let raw = std::fs::read(index_path())?;
    Ok(serde_json::from_slice(&raw)?)
}

/// Whether every recorded range lies inside a document of `size` bytes.
///
/// The fingerprint check proves the index was built from this document; it says
/// nothing about the ranges themselves, which arrive as numbers from a file on
/// disk. `read_range` allocates `len` bytes up front, so a corrupted or
/// hand-edited `len` is an allocation of that size: `[0, u64::MAX]` aborts the
/// process on capacity overflow. That would take down package resolution from
/// inside the one function written to be incapable of it, so the ranges are
/// checked before anything trusts them.
fn ranges_fit(index: &Index, size: u64) -> bool {
    index
        .casks
        .values()
        .all(|&(offset, len)| matches!(offset.checked_add(len), Some(end) if end <= size))
}

/// Load the sidecar, rebuilding it when it does not describe the document next
/// to it.
fn load_index() -> Result<Index> {
    let (size, mtime) = stat(&document_path())?;
    if let Ok(index) = load_index_file()
        && index.version == INDEX_VERSION
        && index.source_size == size
        && index.source_mtime_ns == mtime
        && ranges_fit(&index, size)
    {
        return Ok(index);
    }
    let body = std::fs::read(document_path())?;
    let mut index = match build_index(&body) {
        Ok(index) => index,
        Err(err) => {
            // These bytes can never answer a lookup, and `usable_document` cannot
            // tell: a document truncated to `[` is non-empty. Left in place it is
            // worse than nothing, because `cask.last-modified` survives with it,
            // so every later run revalidates it, is told 304, renews the window
            // on garbage, and falls back anyway. Removing both makes the next
            // refresh unconditional, which is the only thing that repairs this.
            //
            // Only on an indexing failure. A `write_index` failure below leaves a
            // perfectly good document alone.
            discard_document();
            return Err(err);
        }
    };
    index.source_size = size;
    index.source_mtime_ns = mtime;
    write_index(&index)?;
    Ok(index)
}

/// Drop a cached document and the validator that would revalidate it.
///
/// Best effort: if the removal fails the next run simply finds the same state
/// and tries again, which is where it already was.
fn discard_document() {
    debug!("brew-cask: discarding an unusable cached bulk index so the next run refetches it");
    let _ = crate::file::remove_file(document_path());
    let _ = crate::file::remove_file(dir().join("cask.last-modified"));
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

    // Drop any alias that names a live cask. `cask()` prefers the token map
    // anyway, but an alias that can never be reached is a trap for the next
    // reader, and doing it here rather than only at lookup time means the two
    // maps cannot disagree. Done after the loop because the cask claiming a name
    // may be indexed after the one retiring it, so neither insertion order nor
    // last-writer-wins can decide this on its own.
    aliases.retain(|alias, _| !casks.contains_key(alias));

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
/// `None` means "ask the per-cask endpoint": either the index genuinely does not
/// carry this token (a tap cask, or one this snapshot predates) or something
/// about the cache went wrong.
///
/// Infallible on purpose. This is an optimization, so no failure in it may
/// abort package resolution, and returning `Option` rather than
/// `Result<Option<_>>` makes that a property of the type rather than of every
/// caller remembering to catch. Each failure logs before falling back, so a
/// persistently broken cache is visible at debug level rather than silent.
pub(super) async fn cask(token: &str) -> Option<Cask> {
    // A failed refresh is not by itself a reason to ignore a document an earlier
    // run already fetched: those bytes are still metadata Homebrew published,
    // and if the refresh failed because the network is unreachable then the
    // per-cask endpoint this would otherwise fall back to is unreachable too, so
    // discarding the cache would trade a slightly stale answer for no answer.
    //
    // Bounded, though. Some refresh failures never heal, and serving a cached
    // document indefinitely would reintroduce the staleness `STALE_AFTER` is
    // set short to avoid. Past the bound this falls back like any other miss.
    if !refresh_once().await && !within_fallback_window() {
        debug!(
            "brew-cask: no bulk index within the fallback window; falling back to per-cask metadata"
        );
        return None;
    }
    // Validating the index and then reading through it is one operation, for the
    // same reason publishing is. Without the lock a concurrent publication can
    // replace the document in between, and if the stale range happens to parse
    // as the requested token, `validate_cask_identity` accepts it: it checks the
    // token and a path-safe version, not which generation of the document the
    // bytes came from. The result would be a url, version and checksum taken
    // from a document that is no longer there, rather than a fallback.
    //
    // Taken after `refresh`, which acquires and releases it internally, so these
    // never nest. Uncontended except during a publication, which is rare.
    let _lock = match crate::lock_file::LockFile::at(&dir().join("cask.lock")).lock() {
        Ok(lock) => lock,
        Err(err) => {
            debug!(
                "brew-cask: bulk index lock unavailable ({err:#}); falling back to per-cask metadata"
            );
            return None;
        }
    };
    let index = match load_index() {
        Ok(index) => index,
        Err(err) => {
            debug!("brew-cask: bulk index unreadable ({err:#}); falling back to per-cask metadata");
            return None;
        }
    };

    // A live token wins over another cask's old token. Homebrew reuses a name
    // after retiring it: if `foo` is retired into `foo@legacy`, that cask
    // records `old_tokens: ["foo"]`, and a later, unrelated `foo` can be
    // published. Consulting the alias map first would resolve `foo` to
    // `foo@legacy`, and `validate_cask_identity` would accept it, because on the
    // official API it trusts `old_tokens` — so mise would install the wrong
    // software, silently, where the per-cask endpoint would have been right.
    let canonical = if index.casks.contains_key(token) {
        token
    } else {
        index
            .aliases
            .get(token)
            .map(String::as_str)
            .unwrap_or(token)
    };
    let &(offset, len) = index.casks.get(canonical)?;

    // Read and parse are fallible for a reason that is nobody's fault: another
    // mise process can replace the document between `load_index` validating it
    // and this read, leaving these offsets pointing at different bytes.
    // Propagating that would abort the whole resolution, which is precisely what
    // this optimization must never do, so it falls back like every other failure
    // here.
    let body = match read_range(&document_path(), offset, len) {
        Ok(body) => body,
        Err(err) => {
            debug!(
                "brew-cask: bulk index read failed for '{token}' ({err:#}); falling back to per-cask metadata"
            );
            return None;
        }
    };
    match serde_json::from_slice::<Cask>(&body) {
        Ok(cask) => Some(cask),
        Err(err) => {
            debug!(
                "brew-cask: bulk index entry for '{token}' did not parse ({err}); falling back to per-cask metadata"
            );
            None
        }
    }
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

    /// Homebrew reuses a cask name after retiring it. The retired cask records
    /// the old name in `old_tokens`, so for a window both a live `foo` and a
    /// `foo@legacy` claiming `foo` exist, and resolving `foo` to the retired one
    /// installs the wrong software: `validate_cask_identity` accepts it, because
    /// on the official API an `old_tokens` match is trusted.
    #[test]
    fn a_live_token_is_not_shadowed_by_another_casks_old_token() {
        // The claimant is listed FIRST, so last-writer-wins on insertion order
        // would not save this on its own.
        const REUSED: &str = r#"[
          {"token":"foo@legacy","version":"1.0","old_tokens":["foo"]},
          {"token":"foo","version":"2.0"}
        ]"#;

        let index = build_index(REUSED.as_bytes()).unwrap();
        assert!(
            !index.aliases.contains_key("foo"),
            "an alias naming a live cask must not survive indexing"
        );

        let &(offset, len) = index.casks.get("foo").unwrap();
        let slice = &REUSED.as_bytes()[offset as usize..(offset + len) as usize];
        let value: serde_json::Value = serde_json::from_slice(slice).unwrap();
        assert_eq!(value["version"], "2.0", "resolved the retired cask");
    }

    /// An old token that names nothing live still resolves, which is the whole
    /// point of carrying them.
    #[test]
    fn an_old_token_still_resolves_when_nothing_live_claims_it() {
        let index = build_index(DOC.as_bytes()).unwrap();
        assert_eq!(
            index.aliases.get("beta-old").map(String::as_str),
            Some("beta")
        );
    }

    /// `read_range` allocates from the recorded length before reading, so a
    /// range the document cannot contain is an allocation the document cannot
    /// justify. The fingerprint check does not cover this: it proves which
    /// document the index was built from, not that its numbers are sane.
    #[test]
    fn ranges_reaching_past_the_document_are_rejected() {
        let mut index = build_index(DOC.as_bytes()).unwrap();
        let size = DOC.len() as u64;
        assert!(ranges_fit(&index, size));

        index.casks.insert("huge".to_string(), (0, u64::MAX));
        assert!(!ranges_fit(&index, size), "overflowing length accepted");

        index.casks.insert("huge".to_string(), (size, 1));
        assert!(!ranges_fit(&index, size), "range past the end accepted");
    }

    /// The publication race is decided by comparing these, so they have to
    /// order correctly.
    #[test]
    fn http_dates_order_one_generation_against_another() {
        // Real weekday names: chrono rejects a date whose day-of-week does not
        // match, and so would silently make this comparison unreachable.
        let older = parse_http_date("Thu, 17 Sep 2026 10:00:00 GMT").unwrap();
        let newer = parse_http_date("Fri, 18 Sep 2026 09:00:00 GMT").unwrap();
        assert!(newer > older);
        assert!(older >= parse_http_date("  Thu, 17 Sep 2026 10:00:00 GMT  ").unwrap());
        assert_eq!(parse_http_date("not a date"), None);
        assert_eq!(parse_http_date(""), None);
        assert_eq!(
            parse_http_date("Wed, 17 Sep 2026 10:00:00 GMT"),
            None,
            "17 Sep 2026 is a Thursday"
        );
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
