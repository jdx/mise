//! On-disk wings credentials. One file per machine:
//! `MISE_STATE_DIR/wings/credentials.json`, mode 0600 on Unix.
//!
//! ## Why a flat JSON file (not keyring)
//!
//! The first cut deliberately skips `keyring`-crate integration.
//! Adding a keyring dep ripples across every install target (macOS
//! Keychain, Windows Credential Manager, libsecret, KWallet); each
//! has UX edges (the keyring may prompt for a password on every
//! mise invocation, or block in CI environments without an unlocked
//! keyring). A 0600 JSON file is straightforward, debuggable, and
//! matches the access posture of `~/.netrc` / `~/.config/gh/hosts.yml`
//! / etc. — files an attacker with read access to the user's account
//! can already see. A keyring-backed alternative is a clean
//! follow-up if customer signal demands it.
//!
//! ## Refresh token at rest
//!
//! The plaintext refresh token lives in the same file. The proxy
//! stores only its SHA-256 hash, so a leaked refresh token can be
//! used to mint access tokens until either the next rotation
//! (proxy detects the replay and revokes the chain) or the 30-day
//! TTL. That window is the same risk profile as a leaked GitHub
//! Personal Access Token; the 0600 mode is the principal mitigation.
//!
//! ## Format
//!
//! Versioned at the top level so a future shape change can detect
//! pre-versioned files and migrate or refuse cleanly. Field names
//! mirror the proxy's `DevAuthResponse` / `RefreshResponse` shapes
//! one-for-one — same wire format goes in, same JSON lands here.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env;
use crate::result::Result;

/// Current credential file schema version. Bump on any
/// breaking shape change; older versions get rejected by
/// [`Credentials::load`] so we never silently mis-decode.
const SCHEMA_VERSION: u32 = 1;

/// Filename within `MISE_STATE_DIR/wings/`. Single file, one set
/// of credentials per machine — multi-account support is a
/// future concern.
const CREDENTIALS_FILENAME: &str = "credentials.json";

/// Persistent wings credentials. Holds two tokens (access +
/// refresh) plus the identity and expiry metadata that the
/// CLI surfaces in `mise wings whoami`.
///
/// `expires_at` and `refresh_expires_at` are unix seconds (i64
/// to match `time::OffsetDateTime::unix_timestamp()`). Stored
/// alongside the tokens so the CLI can decide whether to refresh
/// without parsing the JWT itself; the JWT's `exp` claim is the
/// source of truth, but the cached field saves a parse on every
/// HTTP call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// Schema version. Currently always [`SCHEMA_VERSION`]; a
    /// load that finds a different value rejects the file with
    /// [`CredentialsError::SchemaMismatch`].
    pub version: u32,
    /// Wings session JWT (`kind = "dev"`). Sent as
    /// `Authorization: Bearer <token>` to cache subdomains.
    pub access_token: String,
    /// Long-lived refresh token. Posted to `/auth/dev/refresh`
    /// in exchange for a rotated access+refresh pair.
    pub refresh_token: String,
    /// Unix seconds at which the access token's `exp` lapses.
    /// Refresh fires when this is within the threshold window
    /// (see `should_refresh`).
    pub expires_at: i64,
    /// Unix seconds at which the refresh token expires.
    /// Re-login required when this passes.
    pub refresh_expires_at: i64,
    /// Apex wings host the credentials were minted against.
    /// Recorded so a `mise.host` setting change forces a fresh
    /// login (different deployment → different signing key).
    pub host: String,
    /// Clerk user id (`user_xxx`). Surfaced in `whoami`.
    pub user_id: String,
    /// GitHub org slug stamped into the wings session's `org`
    /// claim. The user identifies as the Clerk user; this is
    /// the cache routing / logging anchor.
    pub org: String,
}

/// Errors specific to credential file parsing. Surfaced as
/// distinct variants so a future caller can branch on
/// "malformed" vs "schema mismatch" — today both map to the
/// same "re-login required" UX, but the variants stay so the
/// log line can disambiguate. The "no credentials yet" case
/// is *not* an error: [`Credentials::load`] returns `Ok(None)`
/// for a missing file because "logged out" is the steady
/// state for a fresh install.
#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error("wings credentials file is malformed: {0}")]
    Malformed(String),
    #[error(
        "wings credentials file schema mismatch (got v{found}, expected v{expected}); \
         run `mise wings login` to refresh"
    )]
    SchemaMismatch { found: u32, expected: u32 },
}

impl Credentials {
    /// Path to the credentials file. Created lazily on save;
    /// directories are `mkdir -p`'d if missing.
    pub fn path() -> PathBuf {
        env::MISE_STATE_DIR.join("wings").join(CREDENTIALS_FILENAME)
    }

    /// Read the credentials file from disk. `Ok(None)` ↔ no
    /// file exists yet (the user hasn't logged in); `Err(...)`
    /// for files that exist but can't be decoded — the caller
    /// surfaces the distinction to the user.
    ///
    /// I/O errors (permission denied, mid-read disk failure)
    /// propagate as-is rather than being wrapped in
    /// [`CredentialsError::Malformed`] — Gemini flagged the
    /// previous wrap as misleading: "wings credentials file
    /// is malformed: permission denied" hides the real cause.
    /// Only JSON-shape failures + version mismatches surface
    /// as the typed errors.
    pub fn load() -> Result<Option<Self>> {
        let path = Self::path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = crate::file::read_to_string(&path)?;
        let creds: Self = serde_json::from_str(&raw)
            .map_err(|e| eyre::eyre!(CredentialsError::Malformed(e.to_string())))?;
        if creds.version != SCHEMA_VERSION {
            return Err(eyre::eyre!(CredentialsError::SchemaMismatch {
                found: creds.version,
                expected: SCHEMA_VERSION,
            }));
        }
        Ok(Some(creds))
    }

    /// Write the credentials file with mode 0600 on Unix.
    /// `mkdir -p` the parent directory; idempotent re-saves
    /// (e.g. on every refresh) overwrite cleanly.
    ///
    /// On Unix the file is opened with `mode(0o600)` from the
    /// start via `OpenOptions` — a `write` + later
    /// `set_permissions(0o600)` would create a TOCTOU window
    /// in which the file is briefly readable by other users
    /// at the umask default (commonly 0644). Greptile P1 +
    /// Gemini High both flagged the previous shape; the
    /// `OpenOptions` form closes the window.
    ///
    /// On Windows we fall back to `crate::file::write`. ACLs
    /// inherit from the parent directory (under
    /// `MISE_STATE_DIR`, which is the user's profile), so a
    /// fresh credential file is private to the same user
    /// without an explicit chmod equivalent.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            crate::file::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(json.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            crate::file::write(&path, json)?;
        }
        Ok(())
    }

    /// Delete the credentials file. Idempotent — missing file
    /// is success, since "logged out" is the steady state.
    pub fn delete() -> Result<()> {
        let path = Self::path();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Build a fresh credential record from the proxy's
    /// `/auth/dev` response. Computes the absolute expiry
    /// timestamps from `now + expires_in`.
    pub fn from_dev_auth(
        host: String,
        access_token: String,
        expires_in: u64,
        refresh_token: String,
        refresh_expires_in: u64,
        user_id: String,
        org: String,
    ) -> Result<Self> {
        let now = now_unix();
        let access_exp = parse_jwt_exp(&access_token).unwrap_or_else(|| {
            // Fallback to the server-reported `expires_in` if the
            // JWT body can't be parsed (corrupted token, schema
            // change, etc.). The auto-refresh decision only needs
            // an *approximate* expiry — the proxy is the
            // authoritative gate either way.
            now + expires_in as i64
        });
        Ok(Self {
            version: SCHEMA_VERSION,
            access_token,
            refresh_token,
            expires_at: access_exp,
            refresh_expires_at: now + refresh_expires_in as i64,
            host,
            user_id,
            org,
        })
    }

    /// Update self in place from a `/auth/dev/refresh`
    /// response. Identity (user_id, org, host) carries over
    /// from the prior record — refresh doesn't change those.
    pub fn apply_refresh(
        &mut self,
        access_token: String,
        expires_in: u64,
        refresh_token: String,
        refresh_expires_in: u64,
    ) {
        let now = now_unix();
        let access_exp = parse_jwt_exp(&access_token).unwrap_or_else(|| now + expires_in as i64);
        self.access_token = access_token;
        self.expires_at = access_exp;
        self.refresh_token = refresh_token;
        self.refresh_expires_at = now + refresh_expires_in as i64;
    }

    /// True if the access token is at or past `now + leeway`.
    /// Auto-refresh callers use a non-zero leeway (typically
    /// 300 s = 5 min) so an in-flight cache request doesn't
    /// land on the proxy with a token that's expired by a few
    /// hundred ms of wall-clock skew.
    pub fn should_refresh(&self, leeway_secs: i64) -> bool {
        now_unix() + leeway_secs >= self.expires_at
    }

    /// True if the refresh token itself has expired. Past
    /// this point the user must `mise wings login` again —
    /// no rotation can save them.
    pub fn refresh_token_expired(&self) -> bool {
        now_unix() >= self.refresh_expires_at
    }
}

/// Re-export of the module-level shared helper. The local
/// alias keeps the call sites inside this file short.
fn now_unix() -> i64 {
    super::now_unix()
}

/// Best-effort `exp` claim extraction from an unverified JWT.
///
/// Returns `Some(unix_seconds)` if the JWT body decodes and
/// has an integer `exp`, otherwise `None`. We deliberately do
/// not verify the signature here — that's the proxy's job.
/// Reading `exp` is for the local "should I refresh" decision,
/// which is best-effort: a wrong answer just means an extra
/// 401-driven retry, not a security boundary.
pub(crate) fn parse_jwt_exp(jwt: &str) -> Option<i64> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let body_b64 = jwt.split('.').nth(1)?;
    let body = URL_SAFE_NO_PAD.decode(body_b64).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
    v.get("exp")?.as_i64()
}

/// Process-wide cached credentials. Two-tier lock:
///
///   - [`CACHE`] — sync `RwLock` over the latest `Credentials`.
///     Hot-path readers (HTTP layer's pre-request gate) take
///     a read lock, clone, drop. Writes (login / refresh /
///     logout) take a write lock briefly to swap.
///   - [`REFRESH_LOCK`] — async `Mutex` that coordinates
///     "refresh in flight" so two simultaneous HTTP calls
///     don't both POST `/auth/dev/refresh` and trip the
///     proxy's rotation-tripwire (the second call lands on
///     a freshly-revoked token → 401 cascade).
///
/// The two-tier shape is needed because the HTTP layer's
/// `host_auth_headers` is sync (called from sync code before
/// `send_once` even starts), but the refresh itself does an
/// async HTTP round-trip that can't run under the sync lock.
static CACHE: std::sync::RwLock<Option<Credentials>> = std::sync::RwLock::new(None);

/// One-shot init guard. Loaded lazily from disk on first
/// `cached()` call. Without this, every read would re-try
/// the disk load — wasteful, and the second-attempt failure
/// state could differ from the first if the file shape
/// changed mid-process (vanishingly rare but).
static LOAD_ONCE: std::sync::Once = std::sync::Once::new();

/// Async coordination lock for the refresh path. The caller
/// holding this lock is the one allowed to call
/// `client::refresh` and store the result; everyone else
/// either waits and re-reads, or skips the refresh because
/// the holder did it for them.
static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn ensure_loaded() {
    LOAD_ONCE.call_once(|| match Credentials::load() {
        Ok(creds) => {
            *CACHE.write().expect("wings cache poisoned") = creds;
        }
        Err(e) => {
            // Best-effort load: a malformed file leaves the
            // cache empty (caller sees "not signed in") and
            // logs the parse error for debugging.
            log::debug!("wings: failed to load credentials: {e:?}");
        }
    });
}

/// Read the cached credentials. Returns `None` if no login
/// has been performed (or the file failed to load). Sync,
/// fast — clones the underlying `Credentials` out of the
/// read lock.
pub fn cached() -> Option<Credentials> {
    ensure_loaded();
    CACHE.read().expect("wings cache poisoned").clone()
}

/// Replace the cached credentials and persist to disk.
/// Used by `mise wings login` after a successful exchange,
/// and by the auto-refresh path after a successful rotation.
pub fn store(creds: Credentials) -> Result<()> {
    creds.save()?;
    *CACHE.write().expect("wings cache poisoned") = Some(creds);
    Ok(())
}

/// Clear cached + on-disk credentials. Used by
/// `mise wings logout` and on a refresh-failure cascade.
pub fn clear() -> Result<()> {
    Credentials::delete()?;
    *CACHE.write().expect("wings cache poisoned") = None;
    Ok(())
}

/// Acquire the refresh-coordination lock. The caller holds
/// the guard while running the HTTP refresh; concurrent
/// callers block here until the holder releases, then can
/// re-read [`cached`] to see whether the refresh already
/// got them what they wanted.
pub async fn lock_refresh() -> tokio::sync::MutexGuard<'static, ()> {
    REFRESH_LOCK.lock().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JWT with body `{"exp":1800000000,"sub":"x"}` URL-safe
    /// base64 encoded, with arbitrary header + signature.
    /// Hand-built rather than minted so the test pins the
    /// canonical "extract exp from claim payload" parse path.
    const SAMPLE_JWT: &str = "eyJhbGciOiJIUzI1NiJ9.\
         eyJleHAiOjE4MDAwMDAwMDAsInN1YiI6IngifQ.\
         signature";

    #[test]
    fn parse_jwt_exp_extracts_integer_claim() {
        assert_eq!(parse_jwt_exp(SAMPLE_JWT), Some(1_800_000_000));
    }

    #[test]
    fn parse_jwt_exp_returns_none_on_garbage() {
        assert_eq!(parse_jwt_exp("not.a.jwt"), None);
        assert_eq!(parse_jwt_exp(""), None);
        assert_eq!(parse_jwt_exp("only-one-segment"), None);
        // Body that decodes as base64 but isn't JSON.
        assert_eq!(parse_jwt_exp("hdr.aGVsbG8.sig"), None);
    }

    #[test]
    fn should_refresh_fires_inside_leeway() {
        let now = now_unix();
        let creds = sample(now + 60); // expires in 60s
        // 30s leeway → not yet ready (60 - 30 = 30 to go)
        assert!(!creds.should_refresh(30));
        // 90s leeway → past the threshold (now + 90 ≥ exp)
        assert!(creds.should_refresh(90));
    }

    #[test]
    fn should_refresh_handles_already_expired() {
        let now = now_unix();
        let creds = sample(now - 10);
        // Any non-negative leeway → past expiry already.
        assert!(creds.should_refresh(0));
        assert!(creds.should_refresh(60));
    }

    #[test]
    fn refresh_token_expired_after_refresh_window() {
        let now = now_unix();
        let mut creds = sample(now + 60);
        creds.refresh_expires_at = now - 1;
        assert!(creds.refresh_token_expired());
        creds.refresh_expires_at = now + 60;
        assert!(!creds.refresh_token_expired());
    }

    #[test]
    fn schema_mismatch_surfaces_clear_error() {
        let err = CredentialsError::SchemaMismatch {
            found: 99,
            expected: 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("v99"), "got: {msg}");
        assert!(msg.contains("v1"), "got: {msg}");
        assert!(msg.contains("mise wings login"), "got: {msg}");
    }

    fn sample(expires_at: i64) -> Credentials {
        Credentials {
            version: SCHEMA_VERSION,
            access_token: SAMPLE_JWT.into(),
            refresh_token: "rt".into(),
            expires_at,
            refresh_expires_at: now_unix() + 30 * 86_400,
            host: "mise-wings.en.dev".into(),
            user_id: "user_x".into(),
            org: "acme".into(),
        }
    }
}
