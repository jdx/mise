//! Client for the mise-versions versions index, `data/index.v1.json`.
//!
//! The index lets one request answer `latest` and the common fuzzy prefixes
//! for every registry tool, where the full lists cost one `data/<tool>.toml`
//! each. For each tool it holds an order-preserving subset of that tool's full
//! list plus coverage markers. mise runs its own filters and matcher over the
//! subset, exactly as over the full list, and keeps the answer only when the
//! markers prove the full list would give the same one. Otherwise resolution
//! reads the full list as before. The contract is `docs/versions-index.md` in
//! jdx/mise-versions; [`IndexTool::resolve`] is its client algorithm.
//!
//! The index is about 140 KB compressed, against about 5 KB for one tool's
//! list, so it is only worth fetching when it replaces several lists. A copy
//! on disk that is still fresh (`fetch_remote_versions_cache`) is used by any
//! command. A command fetches it only once a second tool in the same process
//! needs a list it does not have: the first tool reads its own list as it
//! always did, so a single-tool command never downloads the index, and a
//! command that resolves many tools makes two requests instead of one per
//! tool. A stale copy is revalidated with `If-None-Match`.

use crate::backend::VersionInfo;
use crate::config::{Settings, SettingsExt};
use crate::dirs;
use crate::file;
use crate::http::HTTP_FETCH;
use crate::versions_host::VERSIONS_HOST_HEADERS;
use eyre::{Result, bail};
use reqwest::StatusCode;
use reqwest::header::{ETAG, HeaderValue, IF_NONE_MATCH};
use serde::de::{self, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;
use tokio::sync::OnceCell;

/// The only `format` this client understands.
const INDEX_FORMAT: u64 = 1;
const INDEX_URL: &str = "https://mise-versions.jdx.dev/data/index.v1.json";
const FLAG_ROLLING: u64 = 1;
const FLAG_PRERELEASE: u64 = 2;

#[derive(Debug, Deserialize)]
struct IndexHeader {
    format: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct VersionsIndex {
    tools: HashMap<String, IndexTool>,
}

impl VersionsIndex {
    /// Parse an index, refusing any `format` but [`INDEX_FORMAT`] before the
    /// rest of the file is read, since another format may change its shape.
    pub(crate) fn parse(body: &str) -> Result<Self> {
        let header: IndexHeader = serde_json::from_str(body)?;
        if header.format != INDEX_FORMAT {
            bail!("unsupported versions index format {}", header.format);
        }
        Ok(serde_json::from_str(body)?)
    }

    /// The entry for `tool`, when it is listed and its markers point inside
    /// its subset. A malformed entry is treated as absent, which falls back.
    pub(crate) fn tool(&self, tool: &str) -> Option<&IndexTool> {
        self.tools.get(tool).filter(|t| t.is_well_formed())
    }
}

/// One tool's entry. `versions` is a subsequence of the tool's full list L, in
/// L's order; see the guarantees on each field.
#[derive(Debug, Deserialize)]
pub(crate) struct IndexTool {
    /// The length of L. When it equals the subset's length, the subset is L.
    count: usize,
    /// `versions[tail..]` is exactly the last `versions.len() - tail` entries of L.
    tail: usize,
    versions: Vec<IndexEntry>,
    /// For each key `P`, every entry of L at or after `versions[P]` that the
    /// shared fuzzy matcher selects for `P` is in the subset, and so is an
    /// entry spelled exactly `P`.
    #[serde(default)]
    prefixes: HashMap<String, usize>,
}

/// Which of the matching versions resolution takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexPick {
    /// The query itself when it is listed, otherwise the last match, as
    /// `latest` and fuzzy requests resolve.
    Request,
    /// The last match, as a `prefix:` request resolves.
    Last,
}

impl IndexTool {
    fn is_well_formed(&self) -> bool {
        let len = self.versions.len();
        len > 0
            && self.count >= len
            && self.tail <= len
            && self.prefixes.values().all(|&i| i < len)
            && self
                .versions
                .iter()
                .map(|e| &e.version)
                .collect::<HashSet<_>>()
                .len()
                == len
    }

    /// The subset as the full list would describe it, in order.
    pub(crate) fn version_infos(&self) -> Vec<VersionInfo> {
        self.versions.iter().map(IndexEntry::version_info).collect()
    }

    /// Resolve `query` from `matches`: the subset after the client's own
    /// filters and matcher, in order, exactly as resolution builds them from
    /// the full list. `None` when the subset cannot prove the answer, including
    /// when nothing matched, so resolution reads the full list: its "no
    /// version found" message names what a release-age cutoff hid, and
    /// `latest` falls back to non-numeric versions there.
    ///
    /// The matcher must select only versions the shared fuzzy matcher selects
    /// for `query`, apart from `query` itself; for `latest` any matcher works.
    pub(crate) fn resolve(
        &self,
        query: &str,
        pick: IndexPick,
        matches: &[String],
    ) -> Option<String> {
        let last = matches.last()?;
        if pick == IndexPick::Request && matches.iter().any(|m| m == query) {
            // Listed in L too, with the same per-entry attributes, and a listed
            // version that equals the query always wins.
            return Some(query.to_string());
        }
        if self.count == self.versions.len() {
            return Some(last.clone());
        }
        let covered_from = if query == "latest" {
            self.tail
        } else {
            (*self.prefixes.get(query)?).min(self.tail)
        };
        // Every entry of L after `versions[covered_from]` that could match is in
        // the subset and was rejected by the same filters, so the full list's
        // last match is this one too.
        let position = self.versions.iter().position(|e| &e.version == last)?;
        (position >= covered_from).then(|| last.clone())
    }
}

/// `[version, created_at, flags]`, where `created_at` is Unix epoch
/// milliseconds. Elements after the third are reserved and ignored.
#[derive(Debug)]
struct IndexEntry {
    version: String,
    created_at: i64,
    flags: u64,
}

impl IndexEntry {
    fn version_info(&self) -> VersionInfo {
        VersionInfo {
            version: self.version.clone(),
            created_at: jiff::Timestamp::from_millisecond(self.created_at)
                .ok()
                .map(|ts| ts.to_string()),
            rolling: self.flags & FLAG_ROLLING != 0,
            // The full lists only ever record `prerelease = true`.
            prerelease: (self.flags & FLAG_PRERELEASE != 0).then_some(true),
            ..Default::default()
        }
    }
}

impl<'de> Deserialize<'de> for IndexEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntryVisitor;

        impl<'de> Visitor<'de> for EntryVisitor {
            type Value = IndexEntry;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("[version, created_at, flags, ...]")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<IndexEntry, A::Error> {
                let version = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let created_at = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let flags = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(IndexEntry {
                    version,
                    created_at,
                    flags,
                })
            }
        }

        deserializer.deserialize_seq(EntryVisitor)
    }
}

/// The index for this process, when using it costs at most one request that
/// would not otherwise replace several. `tool` is the tool asking, and
/// `refresh` asks for a copy validated by this process, as a refreshed version
/// list would be. `None` means read the tool's own list.
pub(crate) async fn get(tool: &str, refresh: bool) -> Option<Arc<VersionsIndex>> {
    // Loaded from the network by this process: as fresh as it gets.
    static FETCHED: OnceCell<Option<Arc<VersionsIndex>>> = OnceCell::const_new();
    // The copy on disk, when it is still fresh.
    static ON_DISK: OnceCell<Option<Arc<VersionsIndex>>> = OnceCell::const_new();
    // The tools that asked. The first reads its own list; any later one means
    // the index replaces several requests.
    static ASKED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

    if let Some(index) = FETCHED.get() {
        return index.clone();
    }
    if !refresh && let Some(index) = ON_DISK.get_or_init(read_fresh_copy).await {
        return Some(index.clone());
    }
    let several = {
        let mut asked = ASKED.lock().unwrap();
        asked.insert(tool.to_string());
        asked.len() > 1
    };
    if !several {
        trace!("versions index: {tool} is the first tool to need a version list, reading its own");
        return None;
    }
    FETCHED.get_or_init(fetch).await.clone()
}

/// The cached response: its `ETag` on the first line, then the body. One
/// file, replaced atomically, so a body is never paired with the `ETag` of
/// another deploy that a concurrent command fetched.
fn cache_path() -> PathBuf {
    dirs::CACHE.join("versions-index").join("index.v1")
}

/// Its mtime is when the cached response was last validated.
fn validated_path() -> PathBuf {
    dirs::CACHE
        .join("versions-index")
        .join("index.v1.validated")
}

fn validated_at() -> Option<SystemTime> {
    validated_path().metadata().ok()?.modified().ok()
}

/// The cached `ETag` and body.
fn read_cached() -> Result<(String, String)> {
    let cached = file::read_to_string(cache_path())?;
    let Some((etag, body)) = cached.split_once('\n') else {
        bail!("malformed cached versions index");
    };
    Ok((etag.to_string(), body.to_string()))
}

async fn read_fresh_copy() -> Option<Arc<VersionsIndex>> {
    let max_age = Settings::get().fetch_remote_versions_cache()?;
    let age = validated_at()?.elapsed().ok()?;
    if age >= max_age {
        return None;
    }
    match read_cached().and_then(|(_, body)| VersionsIndex::parse(&body)) {
        Ok(index) => {
            trace!("versions index: using the copy validated {age:?} ago");
            Some(Arc::new(index))
        }
        Err(err) => {
            debug!("versions index: cannot read the cached copy: {err:#}");
            None
        }
    }
}

/// Fetch the index, revalidating the copy on disk when there is one. A failure
/// is remembered for the rest of the process by the caller's `OnceCell`, and
/// every tool then reads its own list, as before the index existed.
async fn fetch() -> Option<Arc<VersionsIndex>> {
    match fetch_().await {
        Ok(index) => Some(Arc::new(index)),
        Err(err) => {
            debug!("versions index: unavailable, reading version lists one by one: {err:#}");
            None
        }
    }
}

async fn fetch_() -> Result<VersionsIndex> {
    let mut headers = VERSIONS_HOST_HEADERS.clone();
    // Read once: a 304 vouches for the body this `ETag` came with.
    let cached = read_cached().ok().filter(|(etag, _)| !etag.is_empty());
    if let Some((etag, _)) = &cached {
        headers.insert(IF_NONE_MATCH, HeaderValue::from_str(etag)?);
    }
    let resp = HTTP_FETCH
        .get_async_with_headers_allow_error_status(INDEX_URL, &headers)
        .await?;
    match (resp.status(), cached) {
        (StatusCode::NOT_MODIFIED, Some((_, body))) => {
            let index = VersionsIndex::parse(&body)?;
            if let Err(err) = file::write(validated_path(), "") {
                debug!("versions index: cannot mark the copy validated: {err:#}");
            }
            trace!("versions index: not modified");
            Ok(index)
        }
        (StatusCode::OK, _) => {
            let etag = resp
                .headers()
                .get(ETAG)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let body = resp.text().await?;
            let index = VersionsIndex::parse(&body)?;
            // A copy that cannot be written only costs the next command a fetch.
            if let Err(err) = write_copy(&etag, &body) {
                debug!("versions index: cannot cache the copy: {err:#}");
            }
            trace!("versions index: fetched {} tools", index.tools.len());
            Ok(index)
        }
        (status, _) => bail!("{INDEX_URL} returned {status}"),
    }
}

fn write_copy(etag: &str, body: &str) -> Result<()> {
    let path = cache_path();
    file::create_dir_all(path.parent().unwrap())?;
    let partial = path.with_extension(format!("part-{}", std::process::id()));
    file::write(&partial, format!("{etag}\n{body}"))?;
    file::rename(&partial, &path)?;
    // Last, so a copy is never marked validated before it is in place.
    file::write(validated_path(), "")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::BackendArg;
    use crate::backend::aqua::aqua_fuzzy_match;
    use crate::backend::ubi::ubi_fuzzy_match;
    use crate::backend::{
        Backend, fuzzy_match_versions, fuzzy_match_versions_pep440, resolve_in_versions_index,
    };
    use crate::config::Config;
    use crate::install_context::InstallContext;
    use crate::toolset::{ToolVersion, ToolVersionOptions};
    use async_trait::async_trait;
    use jiff::{SignedDuration, Timestamp};
    use std::collections::{BTreeMap, BTreeSet};

    /// Real mise-versions data from one deploy: the index entries of a few tools,
    /// and those tools' full lists from the same deploy as `[version,
    /// created_at_ms, flags]`.
    const INDEX: &str = include_str!("../test/fixtures/versions-index/index.v1.json");
    const LISTS: &str = include_str!("../test/fixtures/versions-index/lists.json");

    fn index() -> VersionsIndex {
        VersionsIndex::parse(INDEX).unwrap()
    }

    fn generated_at() -> Timestamp {
        let value: serde_json::Value = serde_json::from_str(INDEX).unwrap();
        Timestamp::from_millisecond(value["generated_at"].as_i64().unwrap()).unwrap()
    }

    fn lists() -> BTreeMap<String, Vec<VersionInfo>> {
        let lists: BTreeMap<String, Vec<IndexEntry>> = serde_json::from_str(LISTS).unwrap();
        lists
            .into_iter()
            .map(|(tool, list)| (tool, list.iter().map(IndexEntry::version_info).collect()))
            .collect()
    }

    fn tool(json: &str) -> IndexTool {
        serde_json::from_str(json).unwrap()
    }

    fn strings(versions: &[&str]) -> Vec<String> {
        versions.iter().map(|v| v.to_string()).collect()
    }

    /// The full list has 6 entries: `1.0.0`, `1.1.0`, `2.0.0`, `2.1.0`,
    /// `2.2.0`, `3.0.0`. The subset keeps the tail `3.0.0` and the `2`
    /// bucket from `2.1.0`.
    const PARTIAL: &str = r#"{
        "count": 6,
        "tail": 2,
        "versions": [["2.1.0", 0, 0], ["2.2.0", 0, 0], ["3.0.0", 0, 0]],
        "prefixes": {"2": 0, "3": 2}
    }"#;

    #[test]
    fn test_resolves_a_match_inside_the_coverage() {
        let t = tool(PARTIAL);
        let req = IndexPick::Request;
        assert_eq!(
            t.resolve("latest", req, &strings(&["2.1.0", "2.2.0", "3.0.0"])),
            Some("3.0.0".into())
        );
        assert_eq!(
            t.resolve("2", req, &strings(&["2.1.0", "2.2.0"])),
            Some("2.2.0".into())
        );
        // A cutoff hid 2.2.0; 2.1.0 still starts the bucket.
        assert_eq!(
            t.resolve("2", req, &strings(&["2.1.0"])),
            Some("2.1.0".into())
        );
    }

    #[test]
    fn test_falls_back_when_the_subset_cannot_prove_the_answer() {
        let t = tool(PARTIAL);
        let req = IndexPick::Request;
        // A cutoff hid the whole tail: 2.2.0 is older than the tail, and an
        // entry between it and the tail that is not in the subset could match.
        assert_eq!(
            t.resolve("latest", req, &strings(&["2.1.0", "2.2.0"])),
            None
        );
        // Not a key.
        assert_eq!(t.resolve("1", req, &strings(&[])), None);
        assert_eq!(t.resolve("2.1", req, &strings(&["2.1.0"])), None);
        // Nothing matched: the full list's error names what the cutoff hid.
        assert_eq!(t.resolve("3", req, &[]), None);
    }

    #[test]
    fn test_a_listed_query_answers_itself_unless_the_last_match_is_asked_for() {
        let t = tool(PARTIAL);
        let matches = strings(&["2.1.0", "2.2.0"]);
        assert_eq!(
            t.resolve("2.1.0", IndexPick::Request, &matches),
            Some("2.1.0".into())
        );
        // Not a key, so a `prefix:` request cannot be answered.
        assert_eq!(t.resolve("2.1.0", IndexPick::Last, &matches), None);
        assert_eq!(
            t.resolve("2", IndexPick::Last, &matches),
            Some("2.2.0".into())
        );
    }

    #[test]
    fn test_a_complete_list_answers_any_query() {
        let t = tool(r#"{"count": 2, "tail": 1, "versions": [["1.0.0", 0, 0], ["2.0.0", 0, 0]]}"#);
        assert_eq!(
            t.resolve("1", IndexPick::Request, &strings(&["1.0.0"])),
            Some("1.0.0".into())
        );
        assert_eq!(
            t.resolve("latest", IndexPick::Request, &strings(&["1.0.0"])),
            Some("1.0.0".into())
        );
        assert_eq!(t.resolve("3", IndexPick::Request, &[]), None);
    }

    #[test]
    fn test_rejects_another_format_and_malformed_entries() {
        let err = VersionsIndex::parse(r#"{"format": 2, "tools": {"x": 1}}"#).unwrap_err();
        assert!(err.to_string().contains("format 2"), "{err}");
        let index = VersionsIndex::parse(
            r#"{"format": 1, "future": true, "tools": {
                "ok": {"count": 1, "tail": 0, "versions": [["1.0.0", 0, 0, "later"]], "new": 1},
                "empty": {"count": 0, "tail": 0, "versions": []},
                "tail": {"count": 2, "tail": 2, "versions": [["1.0.0", 0, 0]]},
                "prefix": {"count": 2, "tail": 0, "versions": [["1.0.0", 0, 0]], "prefixes": {"1": 1}},
                "dupe": {"count": 3, "tail": 0, "versions": [["1.0.0", 0, 0], ["1.0.0", 0, 0]]}
            }}"#,
        )
        .unwrap();
        assert!(index.tool("ok").is_some());
        for malformed in ["empty", "tail", "prefix", "dupe", "missing"] {
            assert!(index.tool(malformed).is_none(), "{malformed}");
        }
    }

    #[test]
    fn test_entries_read_like_the_full_list() {
        let toml = crate::versions_host::parse_version_list(
            r#"
[versions]
"1.0.0" = { created_at = 2026-10-08T12:34:56.789Z }
"nightly" = { created_at = 2026-10-09T00:00:00.000Z, rolling = true }
"2.0.0-rc1" = { created_at = 2026-10-10T00:00:00.000Z, prerelease = true }
"#,
        )
        .unwrap();
        let index = tool(
            r#"{"count": 3, "tail": 0, "versions": [
                ["1.0.0", 1791462896789, 0],
                ["nightly", 1791504000000, 1],
                ["2.0.0-rc1", 1791590400000, 2]
            ]}"#,
        )
        .version_infos();
        for (from_toml, from_index) in toml.iter().zip(&index) {
            assert_eq!(from_toml.version, from_index.version);
            assert_eq!(
                from_toml.created_at_timestamp(),
                from_index.created_at_timestamp()
            );
            assert_eq!(from_toml.rolling, from_index.rolling);
            assert_eq!(from_toml.prerelease, from_index.prerelease);
        }
    }

    /// A version list served by `_list_remote_versions`, so that resolution
    /// reads it through the real listing path, with one of the matchers
    /// backends that the versions host serves use.
    #[derive(Debug)]
    struct ListBackend {
        ba: Arc<BackendArg>,
        list: Vec<VersionInfo>,
        matcher: Matcher,
    }

    #[derive(Debug, Clone, Copy)]
    enum Matcher {
        /// The trait's own: `fuzzy_match_versions` after an
        /// `is_backend_prerelease` filter that is a no-op by default.
        Shared,
        Pep440,
        Aqua,
        Ubi,
    }

    #[async_trait]
    impl Backend for ListBackend {
        fn ba(&self) -> &Arc<BackendArg> {
            &self.ba
        }

        async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
            Ok(self.list.clone())
        }

        fn fuzzy_match_filter(
            &self,
            versions: Vec<String>,
            query: &str,
            filter_prereleases: bool,
        ) -> Vec<String> {
            match self.matcher {
                Matcher::Shared => fuzzy_match_versions(versions, query, filter_prereleases),
                Matcher::Pep440 => fuzzy_match_versions_pep440(versions, query, filter_prereleases),
                Matcher::Aqua => aqua_fuzzy_match(versions, query, filter_prereleases),
                Matcher::Ubi => ubi_fuzzy_match(versions, query, filter_prereleases),
            }
        }

        async fn install_version_(
            &self,
            _ctx: &InstallContext,
            tv: ToolVersion,
        ) -> Result<ToolVersion> {
            Ok(tv)
        }
    }

    /// Every key of the entry, `latest`, and queries that are not keys: the
    /// oldest minors of the list (keys the index left out), versions spelled
    /// in full, and names.
    fn queries(list: &[VersionInfo], entry: &IndexTool) -> BTreeSet<String> {
        let mut queries: BTreeSet<String> = ["latest", "v1", "lts", "nightly", "stable", "1.2"]
            .into_iter()
            .map(String::from)
            .collect();
        queries.extend(entry.prefixes.keys().cloned());
        let minors = list.iter().filter_map(|v| {
            let body = v.version.strip_prefix('v').unwrap_or(&v.version);
            let mut cuts = body.match_indices(['+', '-', '.']).map(|(i, _)| i);
            let end = cuts.nth(1)?;
            body.starts_with(|c: char| c.is_ascii_digit())
                .then(|| body[..end].to_string())
        });
        queries.extend(minors.take(3));
        queries.extend(list.iter().rev().take(3).map(|v| v.version.clone()));
        queries.extend(list.first().map(|v| v.version.clone()));
        queries
    }

    /// What resolving from the full list gives, for both picks: `latest`
    /// through `latest_version_for_query_with_selection_options`, and any
    /// other query as `resolve_remote_version` and `resolve_prefix` take it
    /// from the matching versions.
    async fn full_list_answers(
        backend: &ListBackend,
        config: &Arc<Config>,
        query: &str,
        opts: &ToolVersionOptions,
        before: Option<Timestamp>,
    ) -> (Option<String>, Option<String>) {
        let matches = backend
            .list_versions_matching_with_selection_options(config, query, opts, before, false)
            .await
            .unwrap();
        let request = if query == "latest" {
            backend
                .latest_version_for_query_with_selection_options(config, query, opts, before, false)
                .await
                .unwrap()
        } else if matches.iter().any(|m| m == query) {
            Some(query.to_string())
        } else {
            matches.last().cloned()
        };
        (request, matches.last().cloned())
    }

    /// The index never gives an answer the full list would not: for every
    /// fixture tool, every shared-matcher variant, prereleases on and off, and
    /// release-age cutoffs from none and 0s to a year, including a client clock
    /// two days ahead of the index.
    #[tokio::test]
    async fn test_index_answers_agree_with_the_full_list() {
        let config = Config::get().await.unwrap();
        let index = index();
        let now = generated_at();
        let mut cutoffs = vec![None];
        for (lag, age) in [
            (0, 0),
            (0, 1),
            (0, 24),
            (0, 168),
            (0, 336),
            (0, 720),
            (0, 8760),
            (48, 24),
            (48, 336),
        ] {
            cutoffs.push(Some(now + SignedDuration::from_hours(lag - age)));
        }
        // A digest in the name keeps a list cached by an older fixture out.
        let fixture = crate::hash::hash_to_str(&LISTS);
        let (mut hits, mut total) = (0, 0);
        for (name, list) in lists() {
            let entry = index.tool(&name).unwrap();
            let matchers = match name.as_str() {
                "python" => &[
                    Matcher::Shared,
                    Matcher::Pep440,
                    Matcher::Aqua,
                    Matcher::Ubi,
                ][..],
                _ => &[Matcher::Shared, Matcher::Aqua, Matcher::Ubi][..],
            };
            for &matcher in matchers {
                let backend = ListBackend {
                    ba: Arc::new(
                        format!("test-versions-index-{name}-{fixture}")
                            .as_str()
                            .into(),
                    ),
                    list: list.clone(),
                    matcher,
                };
                for prereleases in [false, true] {
                    let mut opts = ToolVersionOptions::default();
                    opts.opts
                        .insert("prerelease".into(), toml::Value::Boolean(prereleases));
                    if prereleases {
                        let listed = backend
                            .list_remote_versions_with_selection_options(&config, &opts, false)
                            .await
                            .unwrap();
                        assert_eq!(
                            listed.len(),
                            list.len(),
                            "{name}: the listing dropped versions"
                        );
                    }
                    for &before in &cutoffs {
                        for query in queries(&list, entry) {
                            let got = [IndexPick::Request, IndexPick::Last].map(|pick| {
                                resolve_in_versions_index(
                                    &backend, entry, &query, pick, &opts, before,
                                )
                            });
                            total += 2;
                            if got.iter().all(Option::is_none) {
                                continue;
                            }
                            let (request, last) =
                                full_list_answers(&backend, &config, &query, &opts, before).await;
                            for (pick, got, want) in [
                                (IndexPick::Request, &got[0], request),
                                (IndexPick::Last, &got[1], last),
                            ] {
                                let Some(got) = got else { continue };
                                hits += 1;
                                assert_eq!(
                                    Some(got),
                                    want.as_ref(),
                                    "{name}@{query} {matcher:?} {pick:?} prereleases={prereleases} before={before:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        // Not vacuous: most of these resolve from the index.
        assert!(
            hits * 2 > total,
            "only {hits} of {total} resolved from the index"
        );
    }

    /// The worked examples in the spec, which used this go entry.
    #[test]
    fn test_spec_worked_examples() {
        let index = index();
        let backend = ListBackend {
            ba: Arc::new("test-versions-index-examples".into()),
            list: vec![],
            matcher: Matcher::Shared,
        };
        let opts = ToolVersionOptions::default();
        let resolve = |tool: &str, query: &str, before| {
            resolve_in_versions_index(
                &backend,
                index.tool(tool).unwrap(),
                query,
                IndexPick::Request,
                &opts,
                before,
            )
        };
        let now = generated_at();
        let day = SignedDuration::from_hours(24);
        assert_eq!(
            resolve("go", "latest", Some(now - day)),
            Some("1.27.2".into())
        );
        assert_eq!(
            resolve("go", "1.26", Some(now - day * 30)),
            Some("1.26.8".into())
        );
        assert_eq!(resolve("go", "1.19", None), None);
        assert_eq!(
            resolve("node", "20", Some(now - day)),
            Some("20.20.2".into())
        );
    }
}
