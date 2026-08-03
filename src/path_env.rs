use crate::config::Settings;
use crate::dirs;
use std::collections::HashSet;
use std::env::{join_paths, split_paths};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct PathEnv {
    pre: Vec<PathBuf>,
    mise: Vec<PathBuf>,
    post: Vec<PathBuf>,
    seen_shims: bool,
}

impl PathEnv {
    pub fn new() -> Self {
        Self {
            pre: Vec::new(),
            mise: Vec::new(),
            post: Vec::new(),
            seen_shims: false,
        }
    }

    pub fn add(&mut self, path: PathBuf) {
        for part in split_paths(&path) {
            self.mise.push(part);
        }
    }

    /// First occurrence wins; later exact duplicates are dropped. A later duplicate of
    /// an earlier PATH entry can never win a lookup, so removing it changes nothing for
    /// resolution — but it keeps stale copies left by a previous activation (a session
    /// that inherited PATH without the `__MISE_*` state vars) from surfacing in every
    /// computed environment: `mise env`/`mise x` child PATHs and `mise doctor`'s `path:`
    /// section (#5397). mise re-adds its managed dirs on each activation, so the fresh
    /// copy in `mise` outranks a stale one in `post` and supplies the surviving entry.
    ///
    /// Only for environments mise computes — children and display. A surface that hands
    /// PATH back to the user's live shell must use [`Self::join_verbatim`] instead:
    /// user-owned duplicates there are preserved exactly as written.
    pub fn to_vec(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        self.pre
            .iter()
            .chain(self.mise.iter())
            .chain(self.post.iter())
            .filter(|p| seen.insert(*p))
            .map(|p| p.to_path_buf())
            .collect()
    }

    /// Every entry, duplicates included, in order. The projection for the one surface
    /// that writes PATH back into the user's live shell — `mise activate`'s shim
    /// removal — where a duplicate the user put there deliberately is theirs to keep
    /// (`cli/test_deactivate` pins that contract, three copies and all). Deduplicating
    /// here would silently rewrite the user's PATH as a side effect of removing shims.
    pub fn to_vec_verbatim(&self) -> Vec<PathBuf> {
        self.pre
            .iter()
            .chain(self.mise.iter())
            .chain(self.post.iter())
            .map(|p| p.to_path_buf())
            .collect()
    }

    pub fn join(&self) -> OsString {
        let joined = join_paths(self.to_vec()).unwrap();
        warn_if_cmd_ignores_path(&joined);
        joined
    }

    /// [`Self::join`] over the verbatim projection — see [`Self::to_vec_verbatim`].
    pub fn join_verbatim(&self) -> OsString {
        join_paths(self.to_vec_verbatim()).unwrap()
    }
}

impl Display for PathEnv {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.join().to_string_lossy())
    }
}

impl FromIterator<PathBuf> for PathEnv {
    fn from_iter<T: IntoIterator<Item = PathBuf>>(paths: T) -> Self {
        let settings = Settings::get();

        // When not_found_auto_install is enabled, preserve shims in PATH so they can
        // trigger auto-install for tools that aren't installed yet
        let preserve_shims = settings.not_found_auto_install;

        let mut path_env = Self::new();

        for path in paths {
            if path_env.seen_shims {
                path_env.post.push(path);
            } else if crate::file::paths_eq(&crate::file::replace_path(&path), &dirs::SHIMS)
                && !settings.activate_aggressive
            {
                path_env.seen_shims = true;
                if preserve_shims {
                    path_env.post.push(path);
                }
            } else {
                path_env.pre.push(path);
            }
        }
        if !path_env.seen_shims {
            path_env.post = path_env.pre;
            path_env.pre = Vec::new();
        }

        path_env
    }
}

impl PathEnv {
    pub fn from_path_str(path: &str) -> Self {
        Self::from_iter(split_paths(path))
    }
}

impl FromStr for PathEnv {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_path_str(s))
    }
}

/// All mise-managed install dirs: the primary install dir plus any shared/system
/// install dirs (`MISE_SHARED_INSTALL_DIRS` and the system installs dir) that
/// `env::find_in_shared_installs` resolves tool runtime paths into. Computed once
/// and passed to [`is_mise_install_path`] so the per-PATH-entry check stays cheap.
pub(crate) fn mise_install_dirs() -> Vec<PathBuf> {
    let mut install_dirs = vec![dirs::INSTALLS.to_path_buf()];
    install_dirs.extend(crate::env::shared_install_dirs());
    install_dirs
}

/// Whether `path` is under one of `install_dirs` (see [`mise_install_dirs`]),
/// checked both literally and via canonicalized paths. Such dirs are mise-managed,
/// so a stale one left on PATH (e.g. carried in from a frozen env snapshot) must
/// not outrank the version the current toolset selects. Shared by hook-env
/// reactivation (#10162) and the `mise x`/`run`/`env` child PATH (#10345).
pub(crate) fn is_mise_install_path(path: &std::path::Path, install_dirs: &[PathBuf]) -> bool {
    if install_dirs.iter().any(|d| path.starts_with(d)) {
        return true;
    }
    let Some(path) = crate::file::canonicalize_cached(path) else {
        return false;
    };
    install_dirs
        .iter()
        .filter_map(|d| crate::file::canonicalize_cached(d))
        .any(|d| path.starts_with(d))
}

/// Past this many UTF-16 code units, `cmd.exe` ignores an inherited environment variable
/// outright — the whole value, not just the tail — so everything that was found through
/// PATH stops resolving at once. Programs in the system directory keep working, since
/// `cmd.exe` finds those without consulting PATH, which is what makes the failure look
/// arbitrary. Microsoft documents it in KB 830473, whose "Applies to" stops at Windows 7 /
/// Server 2008 R2 / 2012 R2 and hedges with "as appropriate to the operating system";
/// measured to still hold on Windows 11 26200, where a program outside System32 resolves
/// at 8184 and is not found at 8239.
///
/// <https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/command-line-string-limitation>
pub(crate) const WINDOWS_CMD_PATH_LIMIT: usize = 8191;

/// Windows counts a variable in UTF-16 code units. `encode_utf16` gives exactly that on any
/// platform, where `OsStr::len` would give WTF-8 bytes and over-count every non-ASCII path.
pub(crate) fn path_len_utf16(path: &OsStr) -> usize {
    path.to_string_lossy().encode_utf16().count()
}

/// Whether a PATH of `len` UTF-16 code units would be ignored by `cmd.exe`. Pure, so the
/// boundary is unit-testable everywhere; only the caller is platform-gated.
pub(crate) fn cmd_would_ignore_path(len: usize) -> bool {
    cfg!(windows) && len > WINDOWS_CMD_PATH_LIMIT
}

/// Keyed on the condition rather than on the message. `warn_once!` dedups on formatted
/// text, and a single `hook-env` run measures two different PATHs — the computed toolset
/// environment and the shell's own — so the length embedded in the message would differ
/// and the same problem would be reported twice.
static WARNED_CMD_PATH_LIMIT: AtomicBool = AtomicBool::new(false);

/// Warn when a PATH mise computed is long enough for `cmd.exe` to drop. What follows
/// otherwise is opaque: `npm`, `npx` and batch scripts stop finding anything they look up
/// on PATH, with nothing naming PATH, cmd.exe, or mise.
///
/// At most once per **invocation**, which is not once per user-visible event: under
/// `mise activate`, `hook-env` is a fresh process on every prompt, so a PATH left over the
/// limit warns on every prompt. Whether that is the right cadence is an open question on
/// this PR.
fn warn_if_cmd_ignores_path(path: &OsStr) {
    let len = path_len_utf16(path);
    if !cmd_would_ignore_path(len) {
        return;
    }
    if WARNED_CMD_PATH_LIMIT.swap(true, Ordering::Relaxed) {
        return;
    }
    warn!(
        "PATH is {} characters, longer than the {} cmd.exe accepts. cmd.exe ignores an \
         inherited variable that long outright, so anything run through it — npm, npx, batch \
         scripts — stops finding whatever it looks up on PATH: \
         https://mise.jdx.dev/troubleshooting.html#path-limits",
        len, WINDOWS_CMD_PATH_LIMIT
    );
}

/// [`warn_if_cmd_ignores_path`] for a PATH assembled outside [`PathEnv`]. `hook-env` builds
/// the shell's own PATH by hand, and that copy — not a computed child environment — is what
/// breaks a tool the user starts directly from an activated shell.
pub fn warn_if_cmd_ignores_path_str(path: &str) {
    warn_if_cmd_ignores_path(OsStr::new(path));
}

// Platform-neutral, unlike `tests` below: dedup touches no filesystem and joins nothing,
// so these also run in the windows-unit job.
#[cfg(test)]
mod dedup_tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn to_vec_drops_later_exact_duplicates() {
        // The reactivation residue shape from #5397: a stale copy of a mise-managed dir
        // sits in the inherited PATH (post), and mise adds a fresh copy (mise). The fresh
        // copy comes first in pre+mise+post order, so it wins and the stale one drops.
        let mut path_env = PathEnv::from_iter(
            ["/stale-extra", "/usr/bin", "/stale-extra", "/bin"].map(PathBuf::from),
        );
        path_env.add("/stale-extra".into());
        path_env.add("/tool".into());
        assert_eq!(
            path_env.to_vec(),
            ["/stale-extra", "/tool", "/usr/bin", "/bin"].map(PathBuf::from)
        );
    }

    #[test]
    fn to_vec_verbatim_preserves_user_duplicates() {
        // The live-shell projection: `mise activate`'s shim removal writes this PATH
        // back into the user's shell, and a duplicate the user put there deliberately
        // must survive exactly as written — the boundary cli/test_deactivate pins.
        let mut path_env =
            PathEnv::from_iter(["/shared", "/usr/bin", "/shared"].map(PathBuf::from));
        path_env.add("/tool".into());
        assert_eq!(
            path_env.to_vec_verbatim(),
            ["/tool", "/shared", "/usr/bin", "/shared"].map(PathBuf::from)
        );
    }

    #[test]
    fn to_vec_dedups_by_path_components_not_bytes() {
        // `PathBuf` equality is component-wise, so a trailing-separator variant is the
        // same entry (`/dir/` == `/dir`) and collapses too — those resolve to identical
        // lookups, so dropping one is still semantics-preserving. The casing of a normal
        // component is compared byte-wise and is NOT collapsed, on every platform
        // including Windows: whether `/Dir` and `/dir` are the same place is a filesystem
        // property mise does not assume.
        let path_env = PathEnv::from_iter(["/dir", "/dir/", "/Dir", "/dir-2"].map(PathBuf::from));
        assert_eq!(
            path_env.to_vec(),
            ["/dir", "/Dir", "/dir-2"].map(PathBuf::from)
        );
    }

    /// The prefix is the one component Windows compares case-insensitively, so
    /// drive-letter variants *are* one entry and do collapse. Pinned because it is the
    /// exception to the case rule above rather than a contradiction of it — the two are
    /// easy to conflate when reading `to_vec()`.
    #[cfg(windows)]
    #[test]
    fn to_vec_collapses_drive_letter_case() {
        let path_env = PathEnv::from_iter([r"C:\x", r"c:\x", r"C:\y"].map(PathBuf::from));
        assert_eq!(path_env.to_vec(), [r"C:\x", r"C:\y"].map(PathBuf::from));
    }

    /// The boundary itself, since the constant is the whole point of the check. Measured on
    /// Windows 11 26200: a program outside System32 resolves with a PATH of 8184 and is not
    /// found at 8239, so the limit sits between them and the value below is inclusive.
    #[test]
    fn cmd_would_ignore_path_at_the_boundary() {
        assert!(!cmd_would_ignore_path(0));
        assert!(!cmd_would_ignore_path(WINDOWS_CMD_PATH_LIMIT));
        assert_eq!(
            cmd_would_ignore_path(WINDOWS_CMD_PATH_LIMIT + 1),
            cfg!(windows),
            "the limit is a cmd.exe property, so it must never fire off Windows"
        );
    }

    /// Windows counts UTF-16 code units. Counting `OsStr` bytes instead would treat a
    /// three-byte UTF-8 character as three, and warn about a PATH cmd.exe accepts.
    #[test]
    fn path_len_is_utf16_units_not_bytes() {
        let s = "あ".repeat(WINDOWS_CMD_PATH_LIMIT);
        assert_eq!(path_len_utf16(OsStr::new(&s)), WINDOWS_CMD_PATH_LIMIT);
        assert!(
            s.len() > WINDOWS_CMD_PATH_LIMIT,
            "byte length must differ, or this test proves nothing"
        );
        assert!(!cmd_would_ignore_path(path_len_utf16(OsStr::new(&s))));
    }
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_path_env() {
        let _config = Config::get().await.unwrap();
        let shims = dirs::SHIMS.to_str().unwrap();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                shims,
                "/after-1",
                "/after-2",
                "/after-3",
            ]
            .map(PathBuf::from),
        );
        path_env.add("/1".into());
        path_env.add("/2".into());
        path_env.add("/3".into());
        assert_eq!(
            path_env.to_string(),
            format!("/before-1:/before-2:/before-3:/1:/2:/3:{shims}:/after-1:/after-2:/after-3")
        );
    }

    #[tokio::test]
    async fn test_path_env_no_mise() {
        let _config = Config::get().await.unwrap();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                "/after-1",
                "/after-2",
                "/after-3",
            ]
            .map(PathBuf::from),
        );
        path_env.add("/1".into());
        path_env.add("/2".into());
        path_env.add("/3".into());
        assert_eq!(
            path_env.to_string(),
            format!("/1:/2:/3:/before-1:/before-2:/before-3:/after-1:/after-2:/after-3")
        );
    }
    #[tokio::test]
    async fn test_path_env_with_colon() {
        let _config = Config::get().await.unwrap();
        let mut path_env = PathEnv::from_iter(["/item1", "/item2"].map(PathBuf::from));
        path_env.add("/1:/2".into());
        assert_eq!(path_env.to_string(), format!("/1:/2:/item1:/item2"));
    }
}
