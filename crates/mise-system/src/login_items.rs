//! macOS login items for `[bootstrap.macos.login_items]`: the applications listed
//! under System Settings → General → Login Items & Extensions → Open at Login.
//!
//! `SMAppService` only registers the calling app's own bundle, so this uses the
//! `LSSharedFileList` session login items list in CoreServices. That API has been
//! deprecated since macOS 10.11 but is still the one that manages other
//! applications' entries. Declared apps are added; undeclared entries are only
//! removed with `exclusive = true`, because apps register themselves at runtime.

use std::path::{Component, Path, PathBuf};

use eyre::Result;
pub use mise_config::login_items::LoginItemsTomlConfig;

/// The merged `[bootstrap.macos.login_items]` declaration.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LoginItemsRequest {
    pub apps: Vec<PathBuf>,
    pub exclusive: bool,
}

impl LoginItemsRequest {
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty() && !self.exclusive
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemState {
    /// the app is a login item
    Present,
    /// the app is declared but not a login item
    Missing,
    /// the app is a login item that is not declared (`exclusive` only)
    Undeclared,
}

impl LoginItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "set",
            Self::Missing => "missing",
            Self::Undeclared => "undeclared",
        }
    }

    pub fn needs_apply(self) -> bool {
        self != Self::Present
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoginItemStatus {
    pub path: PathBuf,
    pub state: LoginItemState,
}

pub fn is_available() -> bool {
    cfg!(target_os = "macos")
}

pub fn unavailable_reason() -> String {
    "only available on macos".to_string()
}

/// Validate one declared path the way `[bootstrap.macos.dock].apps` does.
pub fn app_path(value: &str) -> Result<PathBuf> {
    let path = if let Some(rest) = value.strip_prefix("~/") {
        eyre::ensure!(
            !Path::new(rest).has_root(),
            "[bootstrap.macos.login_items].apps: expected a path relative to home: {value}"
        );
        mise_util::env::HOME.join(rest)
    } else {
        PathBuf::from(value)
    };
    eyre::ensure!(
        path.is_absolute()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            && !path.components().any(|c| matches!(c, Component::ParentDir))
            && !value.contains('\0'),
        "[bootstrap.macos.login_items].apps: expected an absolute or ~/ application path: {value}"
    );
    Ok(path.components().collect())
}

/// Validate declared paths, rejecting duplicates after canonicalizing.
pub fn app_paths(values: &[String]) -> Result<Vec<PathBuf>> {
    let mut seen = std::collections::HashSet::new();
    let mut paths = Vec::new();
    for value in values {
        let path = app_path(value)?;
        eyre::ensure!(
            seen.insert(mise_util::file::canonicalize_or_self(&path)),
            "duplicate login item: {}",
            path.display()
        );
        paths.push(path);
    }
    Ok(paths)
}

/// Compare the declaration with the current login items. Pure.
pub fn plan(request: &LoginItemsRequest, current: &[PathBuf]) -> Vec<LoginItemStatus> {
    let current: Vec<_> = current
        .iter()
        .map(|path| (path, mise_util::file::canonicalize_or_self(path)))
        .collect();
    let mut out = vec![];
    let mut declared = std::collections::HashSet::new();
    for path in &request.apps {
        let canonical = mise_util::file::canonicalize_or_self(path);
        let present = current.iter().any(|(_, c)| *c == canonical);
        declared.insert(canonical);
        out.push(LoginItemStatus {
            path: path.clone(),
            state: if present {
                LoginItemState::Present
            } else {
                LoginItemState::Missing
            },
        });
    }
    if request.exclusive {
        for (path, canonical) in current {
            if !declared.contains(&canonical) {
                out.push(LoginItemStatus {
                    path: path.clone(),
                    state: LoginItemState::Undeclared,
                });
            }
        }
    }
    out
}

/// Query the current state of each declared login item. Side-effect free.
pub async fn status(request: &LoginItemsRequest) -> Result<Vec<LoginItemStatus>> {
    let request = request.clone();
    tokio::task::spawn_blocking(move || Ok(plan(&request, &sys::current()?))).await?
}

/// Add the missing items and, for `exclusive`, remove the undeclared ones.
pub async fn apply(statuses: &[LoginItemStatus], dry_run: bool) -> Result<()> {
    for s in statuses {
        match s.state {
            LoginItemState::Present => {}
            LoginItemState::Missing => {
                debug!("adding login item {}", s.path.display());
                if dry_run {
                    miseprintln!("login item add {}", s.path.display());
                }
            }
            LoginItemState::Undeclared => {
                debug!("removing login item {}", s.path.display());
                if dry_run {
                    miseprintln!("login item remove {}", s.path.display());
                }
            }
        }
    }
    if dry_run {
        return Ok(());
    }
    let statuses = statuses.to_vec();
    tokio::task::spawn_blocking(move || {
        for s in statuses {
            match s.state {
                LoginItemState::Present => {}
                LoginItemState::Missing => sys::add(&s.path)?,
                LoginItemState::Undeclared => sys::remove(&s.path)?,
            }
        }
        Ok(())
    })
    .await?
}

#[cfg(target_os = "macos")]
mod sys {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};
    use std::ptr;

    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation::url::CFURL;
    use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
    use core_foundation_sys::base::{CFRelease, CFTypeRef};
    use core_foundation_sys::string::CFStringRef;
    use core_foundation_sys::url::CFURLRef;

    use eyre::Result;

    type ListRef = *const c_void;
    type ItemRef = *const c_void;

    /// `kLSSharedFileListNoUserInteraction | kLSSharedFileListDoNotMountVolumes`
    const RESOLVE_FLAGS: u32 = 0b11;

    // These are deprecated in the SDK but still present in CoreServices.
    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        static kLSSharedFileListSessionLoginItems: CFStringRef;
        static kLSSharedFileListItemLast: ItemRef;
        fn LSSharedFileListCreate(
            allocator: *const c_void,
            list_type: CFStringRef,
            list_options: CFTypeRef,
        ) -> ListRef;
        fn LSSharedFileListCopySnapshot(list: ListRef, seed: *mut u32) -> CFArrayRef;
        fn LSSharedFileListItemCopyResolvedURL(
            item: ItemRef,
            flags: u32,
            error: *mut *const c_void,
        ) -> CFURLRef;
        fn LSSharedFileListInsertItemURL(
            list: ListRef,
            insert_after: ItemRef,
            display_name: CFStringRef,
            icon: *const c_void,
            url: CFURLRef,
            properties_to_set: *const c_void,
            properties_to_clear: CFArrayRef,
        ) -> ItemRef;
        fn LSSharedFileListItemRemove(list: ListRef, item: ItemRef) -> i32;
    }

    /// An owned `LSSharedFileListRef` for the session login items.
    struct List(ListRef);

    impl List {
        fn open() -> Result<Self> {
            let list = unsafe {
                LSSharedFileListCreate(ptr::null(), kLSSharedFileListSessionLoginItems, ptr::null())
            };
            eyre::ensure!(!list.is_null(), "cannot open the macOS login items list");
            Ok(Self(list))
        }

        fn snapshot(&self) -> Result<Snapshot> {
            let array = unsafe { LSSharedFileListCopySnapshot(self.0, ptr::null_mut()) };
            eyre::ensure!(!array.is_null(), "cannot read the macOS login items");
            Ok(Snapshot(array))
        }
    }

    impl Drop for List {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    /// A copied snapshot of the list; its items are only valid while it lives.
    struct Snapshot(CFArrayRef);

    impl Snapshot {
        /// Every item with the path it resolves to; unresolvable items are skipped.
        fn entries(&self) -> Vec<(PathBuf, ItemRef)> {
            let mut out = vec![];
            for i in 0..unsafe { CFArrayGetCount(self.0) } {
                let item = unsafe { CFArrayGetValueAtIndex(self.0, i) };
                let url = unsafe {
                    LSSharedFileListItemCopyResolvedURL(item, RESOLVE_FLAGS, ptr::null_mut())
                };
                if url.is_null() {
                    warn!("a macOS login item could not be resolved; leaving it alone");
                    continue;
                }
                let url = unsafe { CFURL::wrap_under_create_rule(url) };
                if let Some(path) = url.to_path() {
                    out.push((path, item));
                }
            }
            out
        }
    }

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }

    pub(super) fn current() -> Result<Vec<PathBuf>> {
        let list = List::open()?;
        let snapshot = list.snapshot()?;
        Ok(snapshot.entries().into_iter().map(|(p, _)| p).collect())
    }

    pub(super) fn add(path: &Path) -> Result<()> {
        let list = List::open()?;
        let url = CFURL::from_path(path, true)
            .ok_or_else(|| eyre::eyre!("invalid login item path: {}", path.display()))?;
        let name = CFString::new(&path.file_stem().unwrap_or_default().to_string_lossy());
        let item = unsafe {
            LSSharedFileListInsertItemURL(
                list.0,
                kLSSharedFileListItemLast,
                name.as_concrete_TypeRef(),
                ptr::null(),
                url.as_concrete_TypeRef(),
                ptr::null(),
                ptr::null(),
            )
        };
        eyre::ensure!(!item.is_null(), "cannot add login item {}", path.display());
        unsafe { CFRelease(item) };
        Ok(())
    }

    pub(super) fn remove(path: &Path) -> Result<()> {
        let list = List::open()?;
        let want = mise_util::file::canonicalize_or_self(path);
        let snapshot = list.snapshot()?;
        for (found, item) in snapshot.entries() {
            if mise_util::file::canonicalize_or_self(&found) == want {
                let status = unsafe { LSSharedFileListItemRemove(list.0, item) };
                eyre::ensure!(
                    status == 0,
                    "cannot remove login item {} (OSStatus {status})",
                    path.display()
                );
            }
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use std::path::{Path, PathBuf};

    use eyre::Result;

    pub(super) fn current() -> Result<Vec<PathBuf>> {
        eyre::bail!("macOS login items {}", super::unavailable_reason())
    }

    pub(super) fn add(_: &Path) -> Result<()> {
        eyre::bail!("macOS login items {}", super::unavailable_reason())
    }

    pub(super) fn remove(_: &Path) -> Result<()> {
        eyre::bail!("macOS login items {}", super::unavailable_reason())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(apps: &[&str], exclusive: bool) -> LoginItemsRequest {
        LoginItemsRequest {
            apps: apps.iter().map(PathBuf::from).collect(),
            exclusive,
        }
    }

    // `/Applications/...` is not absolute on Windows
    #[cfg(unix)]
    #[test]
    fn validate_app_paths() {
        let home = mise_util::env::HOME.join("Applications/Some.app");
        let paths = app_paths(&[
            "/Applications/Bitwarden.app".into(),
            "~/Applications/Some.app".into(),
        ])
        .unwrap();
        assert_eq!(paths[0], PathBuf::from("/Applications/Bitwarden.app"));
        assert_eq!(paths[1], home);
        for bad in [
            "Bitwarden.app",
            "/Applications/Bitwarden",
            "/Applications/../tmp/X.app",
            "~//etc/X.app",
        ] {
            assert!(app_paths(&[bad.into()]).is_err(), "{bad}");
        }
        assert!(app_paths(&["/Applications/A.app".into(), "/Applications//A.app".into()]).is_err());
    }

    #[test]
    fn additive_by_default() {
        let current = vec![
            PathBuf::from("/Applications/Rectangle.app"),
            PathBuf::from("/Applications/Ghostty.app"),
        ];
        let statuses = plan(
            &request(
                &["/Applications/Ghostty.app", "/Applications/Bitwarden.app"],
                false,
            ),
            &current,
        );
        assert_eq!(
            statuses,
            vec![
                LoginItemStatus {
                    path: "/Applications/Ghostty.app".into(),
                    state: LoginItemState::Present
                },
                LoginItemStatus {
                    path: "/Applications/Bitwarden.app".into(),
                    state: LoginItemState::Missing
                },
            ]
        );
    }

    #[test]
    fn exclusive_removes_undeclared() {
        let current = vec![
            PathBuf::from("/Applications/Rectangle.app"),
            PathBuf::from("/Applications/Ghostty.app"),
        ];
        let statuses = plan(&request(&["/Applications/Ghostty.app"], true), &current);
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].state, LoginItemState::Present);
        assert_eq!(
            statuses[1],
            LoginItemStatus {
                path: "/Applications/Rectangle.app".into(),
                state: LoginItemState::Undeclared
            }
        );
    }
}
