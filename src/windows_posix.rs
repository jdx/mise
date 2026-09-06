//! PATH translation at the boundary between native Windows mise and an active
//! MSYS2/Cygwin shell.
//!
//! mise keeps Windows paths internally, including the opaque `__MISE_ORIG_PATH`
//! snapshot. Only shell PATH assignments and executable references are mapped.
//! The reverse mapper supports original-PATH snapshots inherited from older mise.

use std::borrow::Cow;
#[cfg(any(windows, test))]
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;
use std::sync::LazyLock;

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeKind {
    Msys,
    Cygwin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Mount {
    windows: String,
    posix: String,
    precedence: usize,
}

/// A persisted MSYS2/Cygwin mount table.
#[derive(Debug, Clone)]
struct PathMapper {
    mounts: Vec<Mount>,
    cygdrive: String,
    #[cfg(any(windows, test))]
    user_temp: Option<String>,
    #[cfg(any(windows, test))]
    next_precedence: usize,
}

impl PathMapper {
    #[cfg(any(windows, test))]
    fn new(kind: RuntimeKind, root: &Path, subsystem: Option<&str>) -> Self {
        let mut mapper = Self {
            mounts: Vec::new(),
            user_temp: None,
            cygdrive: if kind == RuntimeKind::Cygwin {
                "/cygdrive".into()
            } else {
                "/".into()
            },
            #[cfg(any(windows, test))]
            next_precedence: 0,
        };
        mapper.add_mount(root.to_string_lossy().as_ref(), "/");
        let usr_dir = if kind == RuntimeKind::Cygwin {
            root.to_path_buf()
        } else {
            root.join("usr")
        };
        if kind == RuntimeKind::Msys {
            mapper.add_mount(usr_dir.join("bin").to_string_lossy().as_ref(), "/bin");
        }
        mapper.add_mount(usr_dir.join("bin").to_string_lossy().as_ref(), "/usr/bin");
        mapper.add_mount(usr_dir.join("lib").to_string_lossy().as_ref(), "/usr/lib");
        if kind == RuntimeKind::Msys
            && let Some(subsystem) = subsystem
            && !subsystem.eq_ignore_ascii_case("MSYS")
        {
            let subsystem = subsystem.to_ascii_lowercase();
            mapper.add_mount(
                root.join(&subsystem).to_string_lossy().as_ref(),
                &format!("/{subsystem}"),
            );
        }
        mapper
    }

    #[cfg(any(windows, test))]
    fn add_mount(&mut self, windows: &str, posix: &str) {
        let windows = normalize_windows(windows);
        let posix = normalize_posix(posix);
        if windows.is_empty() || !posix.starts_with('/') {
            return;
        }
        // A later fstab entry replaces a default/system entry at the same mountpoint.
        self.mounts.retain(|mount| !eq_ascii(&mount.posix, &posix));
        self.mounts.push(Mount {
            windows,
            posix,
            precedence: self.next_precedence,
        });
        self.next_precedence += 1;
    }

    #[cfg(any(windows, test))]
    fn apply_fstab(&mut self, contents: &str, root: &Path, source: &str) {
        for (line_idx, line) in contents.lines().enumerate() {
            match parse_fstab_line(line) {
                Ok(None) => {}
                Ok(Some(FstabEntry::Cygdrive(prefix))) => {
                    self.cygdrive = normalize_posix(&prefix);
                }
                Ok(Some(FstabEntry::Usertemp(prefix))) => {
                    if let Some(temp) = self.user_temp.clone() {
                        self.add_mount(&temp, &prefix);
                    } else {
                        warn!(
                            "ignoring usertemp mount in {source}: native temporary directory unavailable"
                        );
                    }
                }
                Ok(Some(FstabEntry::Mount {
                    native,
                    posix,
                    bind,
                })) => {
                    let native = if bind {
                        self.posix_to_windows(&native)
                    } else if is_windows_absolute(&native) {
                        normalize_windows(&native)
                    } else {
                        normalize_windows(root.join(native).to_string_lossy().as_ref())
                    };
                    if !is_windows_absolute(&native) {
                        warn!(
                            "ignoring malformed mount in {source}:{}: source cannot be resolved",
                            line_idx + 1
                        );
                    } else {
                        self.add_mount(&native, &posix);
                    }
                }
                Err(err) => warn!(
                    "ignoring malformed mount in {source}:{}: {err}",
                    line_idx + 1
                ),
            }
        }
    }

    fn windows_to_posix(&self, input: &str) -> String {
        if input.is_empty() || !is_windows_absolute(input) {
            return input.to_string();
        }
        let normalized = normalize_windows(input);
        if let Some(mount) = self.best_windows_mount(&normalized) {
            return append_tail(&mount.posix, &normalized[mount.windows.len()..]);
        }
        if let Some((drive, tail)) = split_drive(&normalized) {
            let drive = drive.to_ascii_lowercase();
            let base = if self.cygdrive == "/" {
                format!("/{drive}")
            } else {
                format!("{}/{drive}", self.cygdrive.trim_end_matches('/'))
            };
            return append_tail(&base, tail);
        }
        if let Some(tail) = normalized.strip_prefix("//") {
            return format!("//{tail}");
        }
        input.to_string()
    }

    fn posix_to_windows(&self, input: &str) -> String {
        if input.is_empty() || !input.starts_with('/') {
            return input.to_string();
        }
        let normalized = normalize_posix(input);

        if let Some(mount) = self.best_posix_mount(&normalized)
            && mount.posix != "/"
        {
            return append_windows_tail(&mount.windows, &normalized[mount.posix.len()..]);
        }
        // Drive mounts take precedence over the implicit `/` runtime mount.
        if let Some((drive, tail)) = self.split_cygdrive(&normalized) {
            return append_windows_tail(&format!("{}:\\", drive.to_ascii_uppercase()), tail);
        }
        if normalized.starts_with("//") {
            return normalized.replace('/', "\\");
        }
        if let Some(mount) = self.best_posix_mount(&normalized) {
            return append_windows_tail(&mount.windows, &normalized[mount.posix.len()..]);
        }
        input.to_string()
    }

    fn best_windows_mount<'a>(&'a self, path: &str) -> Option<&'a Mount> {
        self.mounts
            .iter()
            .filter(|mount| prefix_matches(path, &mount.windows))
            .max_by_key(|mount| (mount.windows.len(), mount.precedence))
    }

    fn best_posix_mount<'a>(&'a self, path: &str) -> Option<&'a Mount> {
        self.mounts
            .iter()
            .filter(|mount| prefix_matches(path, &mount.posix))
            .max_by_key(|mount| (mount.posix.len(), mount.precedence))
    }

    fn split_cygdrive<'a>(&self, path: &'a str) -> Option<(char, &'a str)> {
        let rest = if let Some(rest) = path.strip_prefix("/proc/cygdrive/") {
            rest
        } else if self.cygdrive == "/" {
            path.strip_prefix('/')?
        } else {
            let rest = strip_prefix_ascii(path, self.cygdrive.trim_end_matches('/'))?;
            rest.strip_prefix('/')?
        };
        let mut chars = rest.chars();
        let drive = chars.next()?;
        if !drive.is_ascii_alphabetic() {
            return None;
        }
        let tail = chars.as_str();
        if tail.is_empty() || tail.starts_with('/') {
            Some((drive, tail))
        } else {
            None
        }
    }
}

#[cfg(any(windows, test))]
enum FstabEntry {
    Mount {
        native: String,
        posix: String,
        bind: bool,
    },
    Cygdrive(String),
    Usertemp(String),
}

#[cfg(any(windows, test))]
fn parse_fstab_line(line: &str) -> std::result::Result<Option<FstabEntry>, String> {
    let fields = split_fstab_fields(line)?;
    if fields.is_empty() {
        return Ok(None);
    }
    if fields.len() < 3 {
        return Err("expected source, mountpoint, and filesystem type".into());
    }
    let source = &fields[0];
    let target = &fields[1];
    let fs_type = &fields[2];
    let options = fields.get(3).map(String::as_str).unwrap_or("");
    if fs_type.eq_ignore_ascii_case("usertemp") {
        if !target.starts_with('/') {
            return Err("usertemp mountpoint must be absolute".into());
        }
        return Ok(Some(FstabEntry::Usertemp(target.clone())));
    }
    if fs_type.eq_ignore_ascii_case("cygdrive") {
        if !target.starts_with('/') {
            return Err("cygdrive prefix must be absolute".into());
        }
        return Ok(Some(FstabEntry::Cygdrive(target.clone())));
    }
    if source == "none" {
        // Valid pseudo-filesystem mounts do not participate in path conversion.
        return Ok(None);
    }
    if !target.starts_with('/') {
        return Err("mountpoint must be absolute".into());
    }
    Ok(Some(FstabEntry::Mount {
        native: source.clone(),
        posix: target.clone(),
        bind: options.split(',').any(|option| option == "bind"),
    }))
}

#[cfg(any(windows, test))]
fn split_fstab_fields(line: &str) -> std::result::Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut field = Vec::new();
    let bytes = line.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'#' if field.is_empty() => break,
            b' ' | b'\t' => {
                if !field.is_empty() {
                    fields.push(
                        String::from_utf8(std::mem::take(&mut field))
                            .map_err(|_| "mount field is not UTF-8")?,
                    );
                }
                idx += 1;
            }
            b'\\' => {
                if idx + 3 < bytes.len()
                    && bytes[idx + 1..=idx + 3]
                        .iter()
                        .all(|byte| matches!(byte, b'0'..=b'7'))
                {
                    let octal = std::str::from_utf8(&bytes[idx + 1..=idx + 3]).unwrap();
                    field.push(u8::from_str_radix(octal, 8).map_err(|e| e.to_string())?);
                    idx += 4;
                } else if idx + 1 < bytes.len() {
                    field.push(bytes[idx + 1]);
                    idx += 2;
                } else {
                    return Err("trailing escape".into());
                }
            }
            byte => {
                field.push(byte);
                idx += 1;
            }
        }
    }
    if !field.is_empty() {
        fields.push(String::from_utf8(field).map_err(|_| "mount field is not UTF-8")?);
    }
    Ok(fields)
}

fn normalize_windows(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while normalized.len() > 3 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

fn normalize_posix(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    if !normalized.starts_with('/') {
        normalized.insert(0, '/');
    }
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || path.starts_with("//")
        || path.starts_with("\\\\")
}

fn split_drive(path: &str) -> Option<(char, &str)> {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        Some((char::from(bytes[0]), &path[2..]))
    } else {
        None
    }
}

fn prefix_matches(path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return path.starts_with('/');
    }
    strip_prefix_ascii(path, prefix)
        .is_some_and(|tail| prefix.ends_with('/') || tail.is_empty() || tail.starts_with('/'))
}

fn strip_prefix_ascii<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

#[cfg(any(windows, test))]
fn eq_ascii(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn append_tail(base: &str, tail: &str) -> String {
    if tail.is_empty() {
        base.to_string()
    } else {
        format!(
            "{}{}",
            base.trim_end_matches('/'),
            ensure_leading_slash(tail)
        )
    }
}

fn append_windows_tail(base: &str, tail: &str) -> String {
    let base = base.replace('/', "\\");
    if tail.is_empty() {
        base
    } else {
        format!(
            "{}{}",
            base.trim_end_matches('\\'),
            ensure_leading_slash(tail).replace('/', "\\")
        )
    }
}

fn ensure_leading_slash(tail: &str) -> Cow<'_, str> {
    if tail.starts_with('/') {
        Cow::Borrowed(tail)
    } else {
        Cow::Owned(format!("/{tail}"))
    }
}

fn split_posix_path_list(value: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0;
    for (idx, ch) in value.char_indices() {
        if ch != ':' {
            continue;
        }
        // Accept a native drive path in a mixed list without splitting its drive letter.
        let entry = &value[start..idx];
        let after = value.as_bytes().get(idx + 1).copied();
        if entry.len() == 1
            && entry.as_bytes()[0].is_ascii_alphabetic()
            && matches!(after, Some(b'/' | b'\\'))
        {
            continue;
        }
        entries.push(&value[start..idx]);
        start = idx + 1;
    }
    entries.push(&value[start..]);
    entries
}

fn map_windows_path_list(mapper: &PathMapper, value: &str) -> String {
    windows_path_entries(value)
        .iter()
        .filter(|entry| !entry.is_empty())
        .map(|entry| mapper.windows_to_posix(entry))
        .collect::<Vec<_>>()
        .join(":")
}

fn map_posix_path_list(mapper: &PathMapper, value: &str) -> String {
    // Activation stores native Windows PATH as an opaque value. Older sessions
    // can supply either a POSIX list or an MSYS-converted native list.
    if value.contains(';') || is_windows_absolute(value) && split_posix_path_list(value).len() == 1
    {
        return value.to_owned();
    }
    split_posix_path_list(value)
        .into_iter()
        .map(|entry| mapper.posix_to_windows(entry))
        .collect::<Vec<_>>()
        .join(";")
}

fn windows_path_entries(value: &str) -> Vec<String> {
    // Match Windows PATH quoting even when mapper unit tests run on Unix.
    let mut quoted = false;
    let mut entries = vec![String::new()];
    for ch in value.chars() {
        match ch {
            '"' => quoted = !quoted,
            ';' if !quoted => entries.push(String::new()),
            _ => entries.last_mut().unwrap().push(ch),
        }
    }
    entries
}

pub(crate) fn executable_for_shell(value: &str) -> Cow<'_, str> {
    match ACTIVE_MAPPER.as_ref() {
        Some(mapper) => Cow::Owned(mapper.windows_to_posix(value)),
        _ => Cow::Borrowed(value),
    }
}

static ACTIVE_MAPPER: LazyLock<Option<PathMapper>> = LazyLock::new(detect_active_mapper);

pub(crate) fn windows_path_list_for_shell(value: &str) -> Cow<'_, str> {
    match ACTIVE_MAPPER.as_ref() {
        Some(mapper) => Cow::Owned(map_windows_path_list(mapper, value)),
        _ => Cow::Borrowed(value),
    }
}

pub(crate) fn windows_path_entries_for_shell(value: &str) -> Vec<String> {
    match ACTIVE_MAPPER.as_ref() {
        Some(mapper) => windows_path_entries(value)
            .iter()
            .filter(|entry| !entry.is_empty())
            .map(|entry| mapper.windows_to_posix(entry))
            .collect(),
        _ => std::env::split_paths(value)
            .map(|p| p.to_string_lossy().into())
            .collect(),
    }
}

pub(crate) fn orig_path_for_windows(value: &str) -> Cow<'_, str> {
    match ACTIVE_MAPPER.as_ref() {
        Some(mapper) => Cow::Owned(map_posix_path_list(mapper, value)),
        _ => Cow::Borrowed(value),
    }
}

#[cfg(any(not(windows), test))]
fn detect_active_mapper() -> Option<PathMapper> {
    // Unit tests exercise explicit PathMapper values. Ambient caller state must
    // not change shell snapshots; production detection is covered by Windows E2E.
    None
}

#[cfg(all(windows, not(test)))]
fn detect_active_mapper() -> Option<PathMapper> {
    // SHELL and MSYSTEM can be absent in a raw shell, or inherited by PowerShell,
    // BusyBox, WSL, or a different installation. The immediate caller is evidence
    // of the runtime actually crossing this boundary. Do not search PATH.
    let executable = parent_executable()?;
    let (kind, root) = runtime_root(&executable)?;
    let msystem = std::env::var("MSYSTEM").ok();
    let mut mapper = PathMapper::new(kind, &root, msystem.as_deref());
    // usertemp mounts (the default /tmp in Git for Windows) use GetTempPathW
    // in the runtime. Windows temp_dir uses the same user environment inputs.
    mapper.user_temp = std::env::temp_dir()
        .to_str()
        .filter(|path| is_windows_absolute(path))
        .map(str::to_owned);
    load_fstab(&mut mapper, &root.join("etc/fstab"), &root);
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok();
    if let Some(username) = username
        && !username.contains(['/', '\\'])
    {
        load_fstab(&mut mapper, &root.join("etc/fstab.d").join(username), &root);
    }
    Some(mapper)
}

#[cfg(all(windows, not(test)))]
fn parent_executable() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    // SAFETY: no pointers are passed; validate the returned handle before owning it.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: the valid handle was freshly allocated and has exactly one owner.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: entry is initialized, correctly sized, and writable.
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) } != 0;
    while found && entry.th32ProcessID != std::process::id() {
        // SAFETY: the snapshot remains alive and entry remains writable.
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) } != 0;
    }
    if !found {
        return None;
    }
    // SAFETY: query-only access, with no inherited handle and no pointer arguments.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            entry.th32ParentProcessID,
        )
    };
    if process.is_null() {
        return None;
    }
    // SAFETY: the valid process handle is freshly allocated and uniquely owned.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut image = vec![0u16; 32768];
    let mut len = image.len() as u32;
    // SAFETY: image has len writable UTF-16 elements and the handle remains alive.
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, image.as_mut_ptr(), &mut len)
    } == 0
    {
        return None;
    }
    Some(std::ffi::OsString::from_wide(&image[..len as usize]).into())
}
#[cfg(any(windows, test))]
fn runtime_root(executable: &Path) -> Option<(RuntimeKind, PathBuf)> {
    let name = executable.file_name()?.to_str()?.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "sh.exe" | "bash.exe" | "zsh.exe" | "fish.exe"
    ) {
        return None;
    }
    for ancestor in executable.ancestors().skip(1).take(5) {
        if ancestor.join("usr/bin/msys-2.0.dll").is_file() {
            return Some((RuntimeKind::Msys, ancestor.to_path_buf()));
        }
        if ancestor.join("bin/cygwin1.dll").is_file() {
            return Some((RuntimeKind::Cygwin, ancestor.to_path_buf()));
        }
    }
    None
}

#[cfg(all(windows, not(test)))]
fn load_fstab(mapper: &mut PathMapper, path: &Path, root: &Path) {
    match std::fs::read_to_string(path) {
        Ok(contents) => mapper.apply_fstab(&contents, root, &path.to_string_lossy()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => warn!("failed to read {}: {err}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msys() -> PathMapper {
        PathMapper::new(
            RuntimeKind::Msys,
            Path::new(r"C:\Program Files\Git"),
            Some("MINGW64"),
        )
    }

    #[test]
    fn maps_git_bash_defaults_both_directions() {
        let mapper = msys();
        for (windows, posix) in [
            (r"C:\Program Files\Git\usr\bin", "/usr/bin"),
            (r"c:/program files/git/mingw64/bin", "/mingw64/bin"),
            (r"D:\Tools\bin", "/d/Tools/bin"),
            (r"\\server\share\bin", "//server/share/bin"),
        ] {
            assert_eq!(mapper.windows_to_posix(windows), posix);
            assert!(
                normalize_windows(&mapper.posix_to_windows(posix))
                    .eq_ignore_ascii_case(&normalize_windows(windows))
            );
        }
    }

    #[test]
    fn maps_a_custom_msys2_root_and_active_subsystem() {
        let mapper = PathMapper::new(
            RuntimeKind::Msys,
            Path::new(r"D:\portable\msys64"),
            Some("UCRT64"),
        );
        assert_eq!(
            mapper.windows_to_posix(r"D:\portable\msys64\ucrt64\bin"),
            "/ucrt64/bin"
        );
        assert_eq!(
            mapper.posix_to_windows("/usr/lib"),
            r"D:\portable\msys64\usr\lib"
        );
    }

    #[test]
    fn cygwin_uses_cygdrive_and_custom_prefix() {
        let mut mapper = PathMapper::new(RuntimeKind::Cygwin, Path::new(r"C:\cygwin64"), None);
        assert_eq!(mapper.windows_to_posix(r"C:\cygwin64\bin"), "/usr/bin");
        assert_eq!(mapper.posix_to_windows("/usr/lib"), r"C:\cygwin64\lib");
        assert_eq!(mapper.windows_to_posix(r"D:\bin"), "/cygdrive/d/bin");
        mapper.apply_fstab(
            "none /drives cygdrive binary 0 0",
            Path::new(r"C:\cygwin64"),
            "fstab",
        );
        assert_eq!(mapper.windows_to_posix(r"D:\bin"), "/drives/d/bin");
        assert_eq!(mapper.posix_to_windows("/drives/D/bin"), r"D:\bin");
    }

    #[test]
    fn fstab_overrides_defaults_and_supports_bind_and_octal_escapes() {
        let mut mapper = msys();
        mapper.apply_fstab(
            "D:/SDK\\040Files /opt/sdk ntfs binary 0 0\n/opt/sdk/tools /tools none bind 0 0\nE:/usr /usr ntfs binary 0 0\nD:/caf\\303\\251 /cafe ntfs",
            Path::new(r"C:\Program Files\Git"),
            "fstab",
        );
        assert_eq!(mapper.posix_to_windows("/opt/sdk/bin"), r"D:\SDK Files\bin");
        assert_eq!(mapper.posix_to_windows("/tools/x"), r"D:\SDK Files\tools\x");
        assert_eq!(
            mapper.posix_to_windows("/usr/bin"),
            r"C:\Program Files\Git\usr\bin"
        );
        assert_eq!(mapper.posix_to_windows("/cafe/bin"), r"D:\café\bin");
    }

    #[test]
    fn longest_prefix_requires_a_directory_boundary() {
        let mut mapper = msys();
        mapper.add_mount(r"D:\work", "/work");
        mapper.add_mount(r"D:\work\project", "/src");
        assert_eq!(mapper.windows_to_posix(r"d:/WORK/project/lib"), "/src/lib");
        assert_eq!(mapper.windows_to_posix(r"D:\workbench"), "/d/workbench");
        assert_eq!(
            mapper.posix_to_windows("/src2"),
            r"C:\Program Files\Git\src2"
        );
    }

    #[test]
    fn lists_preserve_relative_unicode_and_mixed_entries_without_empty_windows_entries() {
        let mapper = msys();
        let posix = map_windows_path_list(&mapper, r"C:\工具\bin;;relative;D:/two");
        assert_eq!(posix, "/c/工具/bin:relative:/d/two");
        assert_eq!(
            map_posix_path_list(&mapper, &posix),
            r"C:\工具\bin;relative;D:\two"
        );
        assert_eq!(
            map_posix_path_list(&mapper, r"C:\native:/d/posix::relative"),
            r"C:\native;D:\posix;;relative"
        );
    }

    #[test]
    fn malformed_lines_do_not_discard_valid_mounts() {
        let mut mapper = msys();
        mapper.apply_fstab(
            "broken\nD:/valid /valid ntfs\nnone relative cygdrive",
            Path::new(r"C:\Program Files\Git"),
            "fstab",
        );
        assert_eq!(mapper.posix_to_windows("/valid/bin"), r"D:\valid\bin");
    }

    #[test]
    fn later_user_mounts_override_system_mounts() {
        let mut mapper = msys();
        mapper.apply_fstab(
            "D:/system /workspace ntfs",
            Path::new(r"C:\Program Files\Git"),
            "fstab",
        );
        mapper.apply_fstab(
            "E:/user /workspace ntfs",
            Path::new(r"C:\Program Files\Git"),
            "fstab.d/user",
        );
        assert_eq!(mapper.posix_to_windows("/workspace/bin"), r"E:\user\bin");
    }

    #[test]
    fn quotes_are_ordinary_path_characters() {
        let mapper = msys();
        assert_eq!(mapper.windows_to_posix("D:/say'hi/bin"), "/d/say'hi/bin");
    }

    #[test]
    fn native_original_paths_are_not_converted_twice() {
        let mapper = msys();
        for value in [
            r"C:\a;D:\b",
            r"C:\a",
            r"relative;D:\b",
            r"\\server\share;C:\bin",
        ] {
            assert_eq!(map_posix_path_list(&mapper, value), value);
        }
        assert_eq!(map_posix_path_list(&mapper, "/c/a:/d/b"), r"C:\a;D:\b");
    }

    #[test]
    fn drive_roots_and_msys_aliases_are_absolute() {
        let mapper = msys();
        assert_eq!(mapper.posix_to_windows("/d"), "D:\\");
        assert_eq!(mapper.posix_to_windows("/proc/cygdrive/d"), "D:\\");
        assert_eq!(
            mapper.posix_to_windows("/bin"),
            r"C:\Program Files\Git\usr\bin"
        );
        let mut mapper = PathMapper::new(RuntimeKind::Cygwin, Path::new(r"C:\cygwin"), None);
        mapper.cygdrive = "/drives".into();
        assert_eq!(mapper.posix_to_windows("/drives/d"), "D:\\");
        assert_eq!(mapper.posix_to_windows("/proc/cygdrive/d"), "D:\\");
    }

    #[test]
    fn drive_root_mounts_match_descendants() {
        let mut mapper = msys();
        mapper.apply_fstab(
            "D:/ /data ntfs binary 0 0\nD:/tools /tools ntfs binary 0 0",
            Path::new(r"C:\Program Files\Git"),
            "fstab",
        );
        assert_eq!(mapper.windows_to_posix(r"D:\"), "/data");
        assert_eq!(
            mapper.windows_to_posix(r"d:\projects\bin"),
            "/data/projects/bin"
        );
        assert_eq!(mapper.windows_to_posix(r"D:\tools\bin"), "/tools/bin");
        assert_eq!(
            mapper.posix_to_windows("/data/projects/bin"),
            r"D:\projects\bin"
        );
        assert_eq!(mapper.posix_to_windows("/data"), r"D:\");
        assert_eq!(mapper.windows_to_posix(r"E:\projects"), "/e/projects");
        assert_eq!(mapper.windows_to_posix(r"D:relative"), "D:relative");
    }

    #[test]
    fn parent_mounts_do_not_remove_child_mounts() {
        let mut mapper = msys();
        mapper.add_mount(r"D:\child", "/custom/deep");
        mapper.add_mount(r"E:\parent", "/custom");
        assert_eq!(mapper.posix_to_windows("/custom/deep/bin"), r"D:\child\bin");
        assert_eq!(mapper.posix_to_windows("/custom/bin"), r"E:\parent\bin");
    }

    #[test]
    fn windows_path_quotes_protect_semicolons_without_adding_empty_entries() {
        assert_eq!(
            map_windows_path_list(&msys(), "\"C:\\a;b\";;D:\\c;"),
            "/c/a;b:/d/c"
        );
    }

    #[test]
    fn usertemp_maps_the_native_temporary_directory() {
        let mut mapper = msys();
        mapper.user_temp = Some(r"C:\Users\someone\AppData\Local\Temp".into());
        mapper.apply_fstab(
            "none /tmp usertemp binary,posix=0 0 0",
            Path::new(r"C:\Git"),
            "fstab",
        );
        assert_eq!(
            mapper.posix_to_windows("/tmp/bin"),
            r"C:\Users\someone\AppData\Local\Temp\bin"
        );
        assert_eq!(
            mapper.windows_to_posix(r"C:\Users\someone\AppData\Local\Temp\bin"),
            "/tmp/bin"
        );
    }

    #[test]
    fn a_shell_name_without_a_runtime_dll_is_not_an_emulator() {
        let temp = tempfile::tempdir().unwrap();
        let bash = temp.path().join("bin/bash.exe");
        std::fs::create_dir_all(bash.parent().unwrap()).unwrap();
        std::fs::write(&bash, []).unwrap();
        assert_eq!(runtime_root(&bash), None);

        let dll = temp.path().join("usr/bin/msys-2.0.dll");
        std::fs::create_dir_all(dll.parent().unwrap()).unwrap();
        std::fs::write(dll, []).unwrap();
        assert_eq!(
            runtime_root(&bash),
            Some((RuntimeKind::Msys, temp.path().into()))
        );
        // A native program installed under a runtime is not a POSIX shell.
        assert_eq!(runtime_root(&temp.path().join("bin/pwsh.exe")), None);
    }
}
