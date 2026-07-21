pub use std::path::*;

use crate::dirs;

pub trait PathExt {
    /// replaces $HOME with "~"
    fn display_user(&self) -> String;
    fn mount(&self, on: &Path) -> PathBuf;
    fn is_empty(&self) -> bool;
}

impl PathExt for Path {
    fn display_user(&self) -> String {
        let home = dirs::HOME.to_string_lossy();
        let home_str: &str = home.as_ref();
        match cfg!(unix) && self.starts_with(home_str) && home != "/" {
            true => self.to_string_lossy().replacen(home_str, "~", 1),
            false => self.to_string_lossy().to_string(),
        }
    }

    fn mount(&self, on: &Path) -> PathBuf {
        if PathExt::is_empty(self) {
            on.to_path_buf()
        } else {
            on.join(self)
        }
    }

    fn is_empty(&self) -> bool {
        self.as_os_str().is_empty()
    }
}

/// Convert a Windows-style path list (`;`-separated, drive-letter prefix, `\` or `/`
/// separator) into a POSIX Unix-style path list (`:`-separated, `/` separator) for a
/// shell that resolves commands from PATH itself.
///
/// `drive_prefix` selects the cygdrive mount style inserted before the drive letter
/// (no trailing slash):
///
/// - `""` → `/c/foo` — MSYS2 / Git Bash (the dominant case, and the default).
/// - `"/cygdrive"` → `/cygdrive/c/foo` — Cygwin's default mount.
/// - any other value (e.g. from `MISE_CYGDRIVE_PREFIX`) → a custom Cygwin `cygdrive`
///   prefix configured via `/etc/fstab`.
///
/// Pure Rust, no subprocess. Designed for the case where mise on Windows spawns a
/// POSIX shell (`bash -c`, `sh -c`, ...) for a task — that shell uses PATH itself to
/// resolve commands, and cannot read `C:\foo;D:\bar`.
///
/// Conversion rules per entry, applied independently:
///
/// - `<drive>:[\\/]...` (canonical Windows drive path) → `<drive_prefix>/<drive lowercase>/<rest with `/` separator>`
/// - already-Unix entries (start with `/`) → pass through unchanged
/// - empty entries (e.g. trailing `;`) → preserved as empty
/// - UNC (`\\?\...`, `\\server\share\...`) → pass through unchanged. bash will fail
///   to use them, which matches what would happen without conversion.
/// - other entries (relative paths, bare names, drive-relative `C:foo`, etc.) →
///   `\` is replaced with `/` so that bash can resolve entries like
///   `node_modules\.bin` or `.\bin` injected by tools that emit Windows separators.
///
/// `drive_prefix` only affects canonical drive entries; every other shape above is
/// prefix-independent.
///
/// Out of scope (kept narrow per maintainer guidance):
///
/// - Cygwin's `/etc/fstab` mount table is not parsed. A non-default `cygdrive` prefix
///   is supplied explicitly via `MISE_CYGDRIVE_PREFIX` (resolved by the caller) rather
///   than discovered from fstab.
/// - Git Bash's "magic" mount of `/usr` to its install dir — `/c/Program Files/Git/usr/bin`
///   is resolved by bash to the same executable as `/usr/bin`, so no remapping is needed
///   for PATH-resolution to succeed.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_path_list_to_unix(path_list: &str, drive_prefix: &str) -> String {
    let mut out = String::with_capacity(path_list.len());
    let mut first = true;
    for entry in path_list.split(WINDOWS_PATH_SEP) {
        if !first {
            out.push(':');
        }
        append_single_windows_path_to_unix(&mut out, entry, drive_prefix);
        first = false;
    }
    out
}

#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_PATH_SEP: char = ';';

#[cfg_attr(not(windows), allow(dead_code))]
fn append_single_windows_path_to_unix(out: &mut String, entry: &str, drive_prefix: &str) {
    if entry.is_empty() {
        return;
    }
    // Already-Unix entries and UNC paths are passed through verbatim.
    if entry.starts_with('/') || entry.starts_with("\\\\") {
        out.push_str(entry);
        return;
    }

    let bytes = entry.as_bytes();
    let is_canonical_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');

    let rest = if is_canonical_drive {
        // C:\foo → <prefix>/c/foo : emit the cygdrive prefix (empty for MSYS/Git
        // Bash, `/cygdrive` for Cygwin), then `/<drive lowercase>`, then the tail
        // with `\` → `/`.
        out.push_str(drive_prefix);
        out.push('/');
        out.push((bytes[0] as char).to_ascii_lowercase());
        &entry[2..]
    } else {
        // Other shapes (relative paths, bare names, `C:foo`) — keep as-is but
        // still translate `\` → `/` so bash can resolve them.
        entry
    };
    for c in rest.chars() {
        out.push(if c == '\\' { '/' } else { c });
    }
}

/// Returns the lowercase stem of `program`'s basename, with any final `.exe`
/// (case-insensitive) stripped. Splits on both `/` and `\` so the result is the
/// same regardless of host `Path` separator — important since this is
/// unit-tested on Linux/macOS too. Does not stat the file — input may be a bare
/// name like `"bash"` that resolves later via the launcher's PATH search.
///
/// Returns `None` only when `program` is not valid UTF-8.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn program_stem(program: &Path) -> Option<String> {
    let s = program.to_str()?;
    let basename = s.rsplit(['/', '\\']).next().unwrap_or(s);
    let stem = match basename.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem,
        _ => basename,
    };
    Some(stem.to_ascii_lowercase())
}

/// Returns true if `program` is the path or basename of a POSIX-style shell that
/// expects a Unix-style PATH. Used on Windows to decide whether to convert the
/// child's PATH before spawning.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_posix_shell_program(program: &Path) -> bool {
    const POSIX_SHELLS: &[&str] = &["bash", "sh", "zsh", "fish", "ksh", "dash"];
    let Some(stem) = program_stem(program) else {
        return false;
    };
    POSIX_SHELLS.iter().any(|name| *name == stem)
}

/// Returns true if `program` is `cmd` / `cmd.exe`, the Windows command
/// interpreter. Used on Windows to decide whether an inline task/hook command
/// must be passed to the shell *verbatim* (via raw command-line args) instead
/// of through std's MSVCRT-style argument quoting. cmd.exe does not understand
/// the `\"` escaping std emits for inner double quotes, so that quoting mangles
/// commands like `python -c "import x"`. See discussion #9355.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_cmd_shell_program(program: &Path) -> bool {
    program_stem(program).as_deref() == Some("cmd")
}

/// Assemble the args (everything after the `cmd.exe` program) for running
/// `script` — plus any forwarded `args` — through cmd.exe *verbatim*.
///
/// Returns the cmd switches from `shell_flags` (with `/s` ensured at the front),
/// followed by the whole command wrapped in a single outer double-quote pair.
/// The caller must append these to the command line as *raw* args (e.g.
/// [`crate::cmd::CmdLineRunner::raw_arg`] / `Command::raw_arg`) so std does not
/// apply its MSVCRT-style quoting. cmd's `/s` then strips exactly that one outer
/// pair and runs the remainder untouched, so any inner double quotes in the
/// command (e.g. `python -c "import x"`) survive to the child. See discussion
/// #9355.
///
/// `script` is emitted exactly as written (it carries the user's own quoting).
/// Forwarded `args` are separate argv values, so each is MSVCRT-quoted *inside*
/// the outer pair (cmd passes those inner quotes through untouched) — preserving
/// the spaces-in-forwarded-args fix from #6744 instead of splitting them.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn cmd_verbatim_args(shell_flags: &[String], script: &str, args: &[String]) -> Vec<String> {
    let mut body = script.to_string();
    for arg in args {
        body.push(' ');
        body.push_str(&quote_arg_for_cmd_body(arg));
    }
    let mut out: Vec<String> = Vec::with_capacity(shell_flags.len() + 2);
    if !shell_flags.iter().any(|f| f.eq_ignore_ascii_case("/s")) {
        out.push("/s".to_string());
    }
    out.extend(shell_flags.iter().cloned());
    out.push(format!("\"{body}\""));
    out
}

/// MSVCRT/`CommandLineToArgvW`-style quoting for a single argument, matching the
/// rules `std::process::Command` uses on Windows. Used for forwarded args placed
/// inside [`cmd_verbatim_args`]' outer quote pair so the *child program* (parsed
/// by the C runtime) sees each as one argument. Quotes when needed: empty, or
/// containing whitespace, `"`, or a cmd.exe metacharacter (`& | < > ( ) ^`).
/// The metacharacters matter because, after `cmd /s /c` strips the single outer
/// quote pair, an unquoted `a&b` would be parsed by cmd as shell syntax rather
/// than reaching the child as one argv value; double quotes suppress that. (`%`
/// is intentionally omitted — cmd expands `%VAR%` even inside quotes, so quoting
/// cannot protect it.) Backslashes are doubled only where they precede a `"`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn quote_arg_for_cmd_body(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"', '&', '|', '<', '>', '(', ')', '^']) {
        return arg.to_string();
    }
    let mut s = String::with_capacity(arg.len() + 2);
    s.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // Emit 2n+1 backslashes so the `"` is escaped, not a delimiter.
                for _ in 0..=backslashes {
                    s.push('\\');
                }
            }
            backslashes = 0;
        }
        s.push(c);
    }
    // Double the trailing backslashes so they don't escape the closing quote.
    for _ in 0..backslashes {
        s.push('\\');
    }
    s.push('"');
    s
}

/// Windows: if `program` is `cmd[.exe]` invoked with a `/c`|`/k` flag, build a
/// configured-but-unspawned [`std::process::Command`] that hands `body` to cmd
/// *verbatim* — raw args, a single outer quote pair, `/s` ensured (see
/// [`cmd_verbatim_args`]) — so inner double quotes survive. Returns `None` for
/// any non-cmd shell (or a cmd invocation that does not run a command string),
/// so the caller falls through to its existing duct/std path unchanged.
///
/// Only the program and args are set; the caller owns env, cwd, stdio, and
/// spawning. Mirrors the inline-task path in
/// `TaskExecutor::get_cmd_program_and_args` and the hook path in
/// `hooks::execute`, extended to the other `cmd /c` call sites. See #9355.
#[cfg(windows)]
pub fn cmd_verbatim_command(
    program: &str,
    flags: &[String],
    body: &str,
) -> Option<std::process::Command> {
    use std::os::windows::process::CommandExt;
    let runs_command = flags
        .iter()
        .any(|f| f.eq_ignore_ascii_case("/c") || f.eq_ignore_ascii_case("/k"));
    if !is_cmd_shell_program(Path::new(program)) || !runs_command {
        return None;
    }
    let mut c = std::process::Command::new(program);
    for a in cmd_verbatim_args(flags, body, &[]) {
        c.raw_arg(a);
    }
    Some(c)
}

/// Split a configured shell *command string* (program + args) into argv,
/// honoring host conventions.
///
/// On Windows, backslashes are ordinary path characters (NOT escapes) and only
/// double-quoted spans group whitespace — matching how a Windows user expects
/// `C:\path\bash.exe` or `"C:\Program Files\..\bash.exe" -c` to parse. A `""`
/// inside a quoted span is a literal `"`; single quotes are literal characters
/// (cmd does not use them, and they can occur in paths). On Unix, defer to
/// `shell_words::split` for POSIX quoting/escaping.
///
/// Used for every configured shell string — a task's `shell`, hook and
/// `[[watch_files]]` shells, and the `*_default_*_shell_args` settings — so an
/// explicit shell path with spaces (when double-quoted) or with backslashes
/// reaches the spawn verbatim instead of being mangled. Returns `Err` only on
/// an unbalanced double quote (Windows) or a `shell_words` parse error (Unix).
pub fn split_shell_command(s: &str) -> eyre::Result<Vec<String>> {
    #[cfg(windows)]
    {
        split_shell_command_windows(s)
    }
    #[cfg(not(windows))]
    {
        Ok(shell_words::split(s)?)
    }
}

/// Windows `CommandLineToArgvW`-style splitter, narrowed to mise's needs:
/// double quotes group whitespace, `""` inside a quoted span is a literal `"`,
/// and backslash is a plain character (never an escape — so Windows paths
/// survive). Single quotes are literal. Errors only on an unterminated
/// double-quoted span.
#[cfg(windows)]
fn split_shell_command_windows(s: &str) -> eyre::Result<Vec<String>> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            in_token = true;
            if in_quotes {
                if chars.peek() == Some(&'"') {
                    // `""` inside a quoted span → a literal `"`.
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                in_quotes = true;
            }
        } else if c.is_whitespace() && !in_quotes {
            if in_token {
                args.push(std::mem::take(&mut cur));
                in_token = false;
            }
        } else {
            in_token = true;
            cur.push(c);
        }
    }
    if in_quotes {
        return Err(eyre::eyre!("unbalanced quote in shell command: {s}"));
    }
    if in_token {
        args.push(cur);
    }
    Ok(args)
}

/// Returns true if `program` (typically a resolved absolute bash path) is a Cygwin
/// shell, detected by a `cygwin` / `cygwin64` / `cygwin32` path segment — Cygwin's
/// default install dirs are `C:\cygwin64` and `C:\cygwin`. Used on Windows to pick
/// the `/cygdrive/c/` PATH form instead of MSYS2 / Git Bash's `/c/`.
///
/// Splits on both `/` and `\` and compares segments case-insensitively, so it works
/// for backslash paths (`MISE_BASH_PATH`, `bash_candidates`) and forward-slash paths
/// (`which::which_in`) without allocating any temporaries. Matches whole path segments
/// so a directory that merely contains "cygwin" as a substring (e.g.
/// `my-cygwinish-tools`) does not trip it. `MSYSTEM` is deliberately not consulted —
/// PowerShell-launched mise inherits none, so it is not a reliable signal.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_cygwin_shell(program: &Path) -> bool {
    let Some(s) = program.to_str() else {
        return false;
    };
    s.split(['/', '\\']).any(|seg| {
        seg.eq_ignore_ascii_case("cygwin")
            || seg.eq_ignore_ascii_case("cygwin64")
            || seg.eq_ignore_ascii_case("cygwin32")
    })
}

/// Convert a single MSYS2/Git Bash (`/c/foo`) or Cygwin (`/cygdrive/c/foo`) style
/// absolute path entry back to Windows form (`C:\foo`). The per-entry inverse of
/// [`windows_path_list_to_unix`] for canonical drive paths, used when reading
/// paths *back* from a POSIX shell (e.g. PATH entries a sourced `[env] _.source`
/// script prepended).
///
/// Returns `None` when the entry has no recognizable Windows equivalent
/// (`/usr/bin`, `/mingw64/bin`, relative paths, empty strings, ...). A custom
/// fstab cygdrive mount root (e.g. `/mnt`) is not recognized either — callers
/// skip such entries; symmetric with the forward converter, which leaves
/// non-default mount discovery to `MISE_CYGDRIVE_PREFIX` rather than fstab.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn unix_path_to_windows(entry: &str) -> Option<String> {
    // UNC round-trip: bash represents `\\server\share` as `//server/share`.
    if let Some(rest) = entry.strip_prefix("//")
        && !rest.is_empty()
        && !rest.starts_with('/')
    {
        return Some(format!(r"\\{}", rest.replace('/', r"\")));
    }
    let bytes = entry.as_bytes();
    // Defensive: an already-Windows drive form (`C:\x` or `C:/x`) — normalize
    // separators only.
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some(entry.replace('/', r"\"));
    }
    // `/cygdrive/c/...` → treat as `/c/...`
    let unprefixed = entry.strip_prefix("/cygdrive").unwrap_or(entry);
    let b = unprefixed.as_bytes();
    // must be exactly `/<letter>` or `/<letter>/...`
    if b.len() >= 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/')
    {
        let drive = (b[1] as char).to_ascii_uppercase();
        let tail = unprefixed[2..].replace('/', r"\"); // "" or "\..."
        return Some(if tail == r"\" || tail.is_empty() {
            format!(r"{drive}:\")
        } else {
            format!("{drive}:{tail}")
        });
    }
    None
}

/// On Windows, when about to spawn a POSIX shell — for a task whose PATH we are
/// about to convert to Unix form, or to source an `[env] _.source` script —
/// resolve the program to its absolute path using the pre-conversion
/// (Windows-form) PATH from the child env.
///
/// Why: `Command::spawn` on Windows uses the *child* env's PATH (when set via
/// `.envs(...)`) to locate the program. If we hand it the converted
/// `/c/foo:/d/bar` PATH, Win32 cannot find `bash.exe`. Resolving here means
/// the child process gets an absolute path argument and does not need PATH
/// search at the OS level.
///
/// For `bash` specifically, prefer a real POSIX bash (Git Bash / MSYS2) over
/// the WSL launcher at `C:\Windows\System32\bash.exe`. The WSL launcher is on
/// PATH first when mise is invoked from PowerShell, and routing into WSL means
/// the spawned command runs inside a separate Linux filesystem where
/// mise-managed Windows tools (and `C:\...` script paths) aren't visible.
/// Resolution order:
///   1. `MISE_BASH_PATH` env var (explicit override).
///   2. Common Git Bash and MSYS2 install locations
///      (`C:\Program Files\Git\bin\bash.exe`,
///      `C:\Program Files (x86)\Git\bin\bash.exe`,
///      `%LOCALAPPDATA%\Programs\Git\bin\bash.exe`,
///      `C:\msys64\usr\bin\bash.exe`, `C:\msys32\usr\bin\bash.exe`).
///   3. `which::which_in_all` over the child env's PATH, picking the first
///      entry that isn't the WSL launcher. This rescues setups where a real
///      POSIX bash is on PATH but appears after `C:\Windows\System32`.
///
/// Returns `None` when the program is not a POSIX shell, the program is already
/// an explicit path (absolute, or relative with a directory component — that is
/// honored verbatim and never re-resolved), the env has no PATH, the PATH is
/// already in Unix form (no `;` and no `\`, so no conversion will fire), `which`
/// finds nothing, or every PATH match for `bash` is the WSL launcher — in those
/// cases the caller keeps the original program string and lets the stdlib spawn
/// it (which will then fail loudly rather than silently routing into WSL).
/// `MISE_BASH_PATH` (when set to an existing file) outranks the PATH-shape
/// gate: an explicit override is honored even when the env's PATH is missing
/// or already Unix-form.
#[cfg(windows)]
pub fn resolve_posix_shell_program_path(
    program: &std::ffi::OsStr,
    env: &std::collections::BTreeMap<String, String>,
) -> Option<std::ffi::OsString> {
    if !is_posix_shell_program(Path::new(program)) {
        return None;
    }
    // An explicit path (absolute, or relative with a directory component) is a
    // deliberate choice of *which* shell binary to run — honor it verbatim
    // rather than re-resolving via the bash candidate list or a PATH search.
    // Only a bare command name (`bash`, `bash.exe`) flows into the WSL-avoidance
    // resolution below. Regression fix for discussion #9932: PR #9750 over-
    // resolved and silently swapped an explicit Cygwin bash for Git Bash.
    if program_has_directory_component(program) {
        return None;
    }

    let is_bash = is_bash_basename(program);

    // The explicit override outranks everything, including the PATH-shape gate
    // below — a user who sets MISE_BASH_PATH wants that bash used even when
    // mise itself runs inside Git Bash (Unix-form PATH). Like an explicit
    // shell path, the choice is honored verbatim (no WSL filtering).
    if is_bash {
        let override_path = env
            .get("MISE_BASH_PATH")
            .cloned()
            .or_else(|| std::env::var("MISE_BASH_PATH").ok())
            .filter(|s| !s.is_empty());
        if let Some(p) = override_path {
            let path = PathBuf::from(&p);
            if path.is_file() {
                return Some(path.into_os_string());
            }
            warn!("MISE_BASH_PATH={p} does not exist; falling back to other candidates");
        }
    }

    let path_val = env.get(&*crate::env::PATH_KEY)?;
    if !path_val.contains(';') && !path_val.contains('\\') {
        return None;
    }

    if is_bash {
        for candidate in bash_candidates(env) {
            if candidate.is_file() {
                return Some(candidate.into_os_string());
            }
        }
    }

    let cwd = std::env::current_dir().ok()?;

    if is_bash {
        // For bash, walk every PATH match and pick the first that isn't the
        // WSL launcher. This rescues setups where a real POSIX bash sits later
        // on PATH than `C:\Windows\System32\bash.exe` — common under PowerShell
        // when Git Bash is installed somewhere `bash_candidates` doesn't probe.
        let mut all = which::which_in_all(program, Some(path_val.as_str()), cwd).ok()?;
        if let Some(p) = all.find(|p| !is_wsl_launcher_bash(p)) {
            return Some(p.into_os_string());
        }
        warn!(
            "no real POSIX bash found on PATH (only the WSL launcher) when resolving bash; \
             install Git Bash or MSYS2, or set MISE_BASH_PATH to a real POSIX bash to silence this"
        );
        return None;
    }

    which::which_in(program, Some(path_val.as_str()), cwd)
        .ok()
        .map(|p| p.into_os_string())
}

/// Returns true if `program`'s basename (case-insensitive, `.exe` stripped) is `bash`.
/// More specific than [`is_posix_shell_program`], which also accepts
/// sh/zsh/fish/ksh/dash. Used to scope the Windows bash-resolution heuristics so
/// they don't fire for other POSIX shells we might gain support for later.
#[cfg(windows)]
fn is_bash_basename(program: &std::ffi::OsStr) -> bool {
    program_stem(Path::new(program)).as_deref() == Some("bash")
}

/// Returns true if `program` carries an explicit directory component — an
/// absolute path (`C:\x\bash.exe`, `C:/x/bash.exe`) or a relative one with a
/// separator (`./bash`, `bin/bash`) — as opposed to a bare command name
/// (`bash`, `bash.exe`) that must be looked up on PATH. Uses `Path::components`
/// (allocation-free, and treats both `/` and `\` as separators on Windows): a
/// bare file name has exactly one component, anything with a directory has more.
#[cfg(windows)]
fn program_has_directory_component(program: &std::ffi::OsStr) -> bool {
    Path::new(program).components().count() > 1
}

/// Common real-POSIX-bash install locations on Windows (Git Bash + MSYS2), in
/// preference order. Pure given `env` (no filesystem access), so the caller
/// stats each candidate. `MISE_BASH_PATH` covers anything outside this list,
/// including non-`C:` drive installs.
#[cfg(windows)]
fn bash_candidates(env: &std::collections::BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"),
    ];
    let local_appdata = env
        .get("LOCALAPPDATA")
        .cloned()
        .or_else(|| std::env::var("LOCALAPPDATA").ok());
    if let Some(local) = local_appdata.filter(|s| !s.is_empty()) {
        candidates.push(PathBuf::from(local).join(r"Programs\Git\bin\bash.exe"));
    }
    // MSYS2 standalone installs (default `C:\msys64`, 32-bit fallback `C:\msys32`).
    candidates.push(PathBuf::from(r"C:\msys64\usr\bin\bash.exe"));
    candidates.push(PathBuf::from(r"C:\msys32\usr\bin\bash.exe"));
    candidates
}

/// Returns true if `path` looks like the Windows-shipped WSL launcher rather
/// than a real POSIX bash. Matches `C:\Windows\System32\bash.exe` and the
/// `WindowsApps\bash.exe` shim that App Execution Aliases install. Both
/// dispatch into a WSL distribution's Linux userspace, which is the wrong
/// place to run a command that uses mise-managed Windows tools or `C:\...`
/// script paths.
#[cfg(windows)]
pub fn is_wsl_launcher_bash(path: &Path) -> bool {
    let Some(s) = path.to_str() else {
        return false;
    };
    let lower = s.to_ascii_lowercase().replace('/', "\\");
    lower.ends_with(r"\windows\system32\bash.exe")
        || lower.contains(r"\microsoft\windowsapps\bash.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[cfg(windows)]
    fn env_with_path(path: &str) -> std::collections::BTreeMap<String, String> {
        let mut env = std::collections::BTreeMap::new();
        env.insert((*crate::env::PATH_KEY).to_string(), path.to_string());
        env.insert("OTHER".to_string(), "unchanged".to_string());
        env
    }

    /// MSYS2 / Git Bash style (`/c/...`) — the empty cygdrive prefix (the default).
    fn msys(s: &str) -> String {
        windows_path_list_to_unix(s, "")
    }

    /// Cygwin default style (`/cygdrive/c/...`).
    fn cygwin(s: &str) -> String {
        windows_path_list_to_unix(s, "/cygdrive")
    }

    #[test]
    fn test_windows_path_list_to_unix_basic() {
        assert_eq!(msys(r"C:\foo;D:\bar"), "/c/foo:/d/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_forward_slash() {
        assert_eq!(msys("C:/foo;D:/bar"), "/c/foo:/d/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_mixed_separators() {
        assert_eq!(msys(r"C:\foo\bar;D:/baz/qux"), "/c/foo/bar:/d/baz/qux");
    }

    #[test]
    fn test_windows_path_list_to_unix_passthrough_unix_entries() {
        assert_eq!(msys("/usr/bin;C:\\foo;/c/bar"), "/usr/bin:/c/foo:/c/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_passthrough_unc() {
        // UNC entries are passed through verbatim (they contain `:` themselves,
        // so we cannot split the result on `:` to inspect entries — bash receives
        // the whole string and will fail to use the UNC entry, which matches what
        // would happen without conversion).
        assert_eq!(msys(r"\\?\C:\foo;C:\bar"), r"\\?\C:\foo:/c/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_empty_entries() {
        assert_eq!(msys("C:\\foo;"), "/c/foo:");
        assert_eq!(msys(";C:\\foo"), ":/c/foo");
        assert_eq!(msys(""), "");
    }

    #[test]
    fn test_windows_path_list_to_unix_drive_letter_case() {
        assert_eq!(msys(r"C:\foo"), "/c/foo");
        assert_eq!(msys(r"c:\foo"), "/c/foo");
    }

    #[test]
    fn test_windows_path_list_to_unix_program_files_with_spaces() {
        assert_eq!(
            msys(r"C:\Program Files\Git\bin"),
            "/c/Program Files/Git/bin"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_bare_drive_letter_passthrough() {
        // Bare "C:" or "C:foo" (relative-to-drive) is unrecognized — pass through.
        assert_eq!(msys("C:"), "C:");
        assert_eq!(msys("C:foo"), "C:foo");
    }

    #[test]
    fn test_windows_path_list_to_unix_relative_paths_with_backslashes() {
        // mise can inject relative entries via `[env] _.path = ["./node_modules/.bin"]`,
        // and tools that emit Windows separators may produce backslash forms. bash
        // does not treat `\` as a separator, so we translate `\` → `/` for non-UNC,
        // non-canonical-drive entries too.
        assert_eq!(msys(r"node_modules\.bin"), "node_modules/.bin");
        assert_eq!(msys(r".\bin"), "./bin");
        assert_eq!(
            msys(r"node_modules\.bin;C:\tools\bin"),
            "node_modules/.bin:/c/tools/bin"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_single_entry() {
        assert_eq!(msys(r"C:\foo"), "/c/foo");
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_basic() {
        assert_eq!(cygwin(r"C:\foo;D:\bar"), "/cygdrive/c/foo:/cygdrive/d/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_forward_slash() {
        assert_eq!(cygwin("C:/foo;D:/bar"), "/cygdrive/c/foo:/cygdrive/d/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_drive_letter_case() {
        assert_eq!(cygwin(r"c:\foo"), "/cygdrive/c/foo");
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_program_files_with_spaces() {
        assert_eq!(
            cygwin(r"C:\Program Files\Git\bin"),
            "/cygdrive/c/Program Files/Git/bin"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_passthrough_unix_and_unc() {
        // The cygdrive prefix only affects canonical drive entries; already-Unix
        // and UNC entries are still passed through verbatim.
        assert_eq!(
            cygwin(r"/usr/bin;\\?\C:\x;C:\y"),
            r"/usr/bin:\\?\C:\x:/cygdrive/c/y"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_empty_entries() {
        assert_eq!(cygwin("C:\\foo;"), "/cygdrive/c/foo:");
    }

    #[test]
    fn test_windows_path_list_to_unix_cygwin_relative_paths_unprefixed() {
        // Non-drive entries get no cygdrive prefix — only `\` → `/`.
        assert_eq!(cygwin(r"node_modules\.bin"), "node_modules/.bin");
    }

    #[test]
    fn test_windows_path_list_to_unix_custom_cygdrive_prefix() {
        // A custom prefix such as `MISE_CYGDRIVE_PREFIX=/mnt` (fstab-configured).
        assert_eq!(
            windows_path_list_to_unix(r"C:\foo;D:\bar", "/mnt"),
            "/mnt/c/foo:/mnt/d/bar"
        );
    }

    #[test]
    fn test_is_posix_shell_program() {
        assert!(is_posix_shell_program(Path::new("bash")));
        assert!(is_posix_shell_program(Path::new("bash.exe")));
        assert!(is_posix_shell_program(Path::new("BASH.EXE")));
        assert!(is_posix_shell_program(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(is_posix_shell_program(Path::new("/usr/bin/bash")));
        assert!(is_posix_shell_program(Path::new("sh")));
        assert!(is_posix_shell_program(Path::new("zsh")));
        assert!(is_posix_shell_program(Path::new("fish")));

        assert!(!is_posix_shell_program(Path::new("cmd")));
        assert!(!is_posix_shell_program(Path::new("cmd.exe")));
        assert!(!is_posix_shell_program(Path::new("powershell")));
        assert!(!is_posix_shell_program(Path::new("pwsh.exe")));
        assert!(!is_posix_shell_program(Path::new("rustc")));
        assert!(!is_posix_shell_program(Path::new("")));
    }

    #[test]
    fn test_is_cmd_shell_program() {
        assert!(is_cmd_shell_program(Path::new("cmd")));
        assert!(is_cmd_shell_program(Path::new("cmd.exe")));
        assert!(is_cmd_shell_program(Path::new("CMD.EXE")));
        assert!(is_cmd_shell_program(Path::new(
            r"C:\Windows\System32\cmd.exe"
        )));

        assert!(!is_cmd_shell_program(Path::new("bash")));
        assert!(!is_cmd_shell_program(Path::new("bash.exe")));
        assert!(!is_cmd_shell_program(Path::new("powershell")));
        assert!(!is_cmd_shell_program(Path::new("pwsh.exe")));
        // `cmd.com` is not the modern interpreter we target.
        assert!(!is_cmd_shell_program(Path::new("cmd.com")));
        assert!(!is_cmd_shell_program(Path::new("")));
    }

    #[test]
    fn test_cmd_verbatim_args() {
        let c = || "/c".to_string();

        // The reported case: inner double quotes must survive untouched inside
        // the single outer quote pair, with `/s` ensured. The caller passes
        // each element as a raw arg, so the resulting command line is
        // `cmd /s /c "uv run python -c "import x""` — cmd strips only the outer
        // pair. See discussion #9355.
        assert_eq!(
            cmd_verbatim_args(&[c()], r#"uv run python -c "import x""#, &[]),
            sv(&["/s", "/c", r#""uv run python -c "import x"""#])
        );

        // No inner quotes — still wrapped, still gets `/s`.
        assert_eq!(
            cmd_verbatim_args(&[c()], "echo hi", &[]),
            sv(&["/s", "/c", r#""echo hi""#])
        );

        // Forwarded args go inside the same outer quote pair, each MSVCRT-quoted
        // when it contains spaces so the program still sees them as one argument
        // (preserving the #6744 spaces-in-args fix). `c` stays bare.
        assert_eq!(
            cmd_verbatim_args(&[c()], "proxy", &["a b".to_string(), "c".to_string()]),
            sv(&["/s", "/c", r#""proxy "a b" c""#])
        );

        // A forwarded path with a space (mirrors e2e-win/task_args.Tests.ps1):
        // `type ".\test dir\file.txt"` must reach `type` as a single argument.
        assert_eq!(
            cmd_verbatim_args(&[c()], "type", &[r".\test dir\file.txt".to_string()]),
            sv(&["/s", "/c", r#""type ".\test dir\file.txt"""#])
        );

        // An explicit `/s` in the shell flags is not duplicated.
        assert_eq!(
            cmd_verbatim_args(&["/s".to_string(), c()], "echo hi", &[]),
            sv(&["/s", "/c", r#""echo hi""#])
        );
    }

    #[test]
    fn test_quote_arg_for_cmd_body() {
        // No special chars -> returned as-is (no quotes added).
        assert_eq!(quote_arg_for_cmd_body("plain"), "plain");
        // Space/tab -> wrapped.
        assert_eq!(quote_arg_for_cmd_body("a b"), r#""a b""#);
        // Empty -> quoted empty string.
        assert_eq!(quote_arg_for_cmd_body(""), r#""""#);
        // Inner quote -> escaped as \".
        assert_eq!(quote_arg_for_cmd_body(r#"a"b"#), r#""a\"b""#);
        // Trailing backslashes are doubled before the closing quote (only when
        // the arg is quoted because it also contains a space).
        assert_eq!(quote_arg_for_cmd_body(r"a b\"), r#""a b\\""#);
        // A backslash not adjacent to a quote is left alone.
        assert_eq!(quote_arg_for_cmd_body(r"a\b c"), r#""a\b c""#);
        // cmd metacharacters (no whitespace) are still quoted so cmd does not
        // interpret them as shell syntax after stripping the outer quote pair.
        assert_eq!(quote_arg_for_cmd_body("a&b"), r#""a&b""#);
        assert_eq!(quote_arg_for_cmd_body("foo|bar"), r#""foo|bar""#);
        assert_eq!(quote_arg_for_cmd_body("a>b"), r#""a>b""#);
    }

    #[test]
    #[cfg(windows)]
    fn test_cmd_verbatim_command() {
        // cmd + /c -> Some; the body is wrapped in one outer quote pair with `/s`
        // ensured, passed via raw_arg (which appears verbatim in get_args()).
        let c = cmd_verbatim_command("cmd", &["/c".to_string()], r#"echo "a b""#).unwrap();
        assert_eq!(c.get_program().to_str(), Some("cmd"));
        let args: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "/s".to_string(),
                "/c".to_string(),
                r#""echo "a b"""#.to_string()
            ]
        );
        // /k also runs a command string.
        assert!(cmd_verbatim_command("cmd", &["/k".to_string()], "echo hi").is_some());
        // Non-cmd shell, or cmd without /c|/k -> None (caller falls through).
        assert!(cmd_verbatim_command("bash", &["-c".to_string()], "echo hi").is_none());
        assert!(cmd_verbatim_command("cmd", &[], "echo hi").is_none());
    }

    #[test]
    fn test_split_shell_command_bare_names() {
        assert_eq!(split_shell_command("bash -c").unwrap(), sv(&["bash", "-c"]));
        assert_eq!(split_shell_command("sh -c").unwrap(), sv(&["sh", "-c"]));
        assert_eq!(
            split_shell_command("sh -c -o errexit").unwrap(),
            sv(&["sh", "-c", "-o", "errexit"])
        );
    }

    #[test]
    fn test_split_shell_command_empty() {
        assert_eq!(split_shell_command("").unwrap(), sv(&[]));
        assert_eq!(split_shell_command("   ").unwrap(), sv(&[]));
    }

    #[test]
    fn test_split_shell_command_quoted_path_with_spaces() {
        // A double-quoted path containing spaces is one token on both platforms.
        assert_eq!(
            split_shell_command("\"C:/Program Files/Git/bin/bash.exe\" -c").unwrap(),
            sv(&["C:/Program Files/Git/bin/bash.exe", "-c"])
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_split_shell_command_windows_backslash_is_literal() {
        // Backslash is a plain path char on Windows, not an escape.
        assert_eq!(
            split_shell_command(r"C:\msys64\usr\bin\bash.exe -c").unwrap(),
            sv(&[r"C:\msys64\usr\bin\bash.exe", "-c"])
        );
        assert_eq!(
            split_shell_command("\"C:\\Program Files\\Git\\bin\\bash.exe\" -c").unwrap(),
            sv(&[r"C:\Program Files\Git\bin\bash.exe", "-c"])
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_split_shell_command_windows_unquoted_space_splits() {
        // Documented ambiguity: an unquoted space splits even inside a path.
        assert_eq!(
            split_shell_command(r"C:/Program Files/Git/bin/bash.exe -c").unwrap(),
            sv(&["C:/Program", "Files/Git/bin/bash.exe", "-c"])
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_split_shell_command_windows_double_quote_is_literal() {
        // `""` inside a quoted span → a literal `"`.
        assert_eq!(
            split_shell_command("\"a\"\"b\" c").unwrap(),
            sv(&["a\"b", "c"])
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_split_shell_command_windows_unbalanced_quote_errs() {
        assert!(split_shell_command("\"unterminated").is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn test_split_shell_command_unix_posix_semantics() {
        // Unix keeps shell_words (POSIX) behavior: backslash escapes, single quotes group.
        assert_eq!(
            split_shell_command(r"bash\ script -c").unwrap(),
            sv(&["bash script", "-c"])
        );
        assert_eq!(split_shell_command("'a b' c").unwrap(), sv(&["a b", "c"]));
    }

    #[test]
    fn test_is_cygwin_shell_detects_cygwin_paths() {
        assert!(is_cygwin_shell(Path::new(r"C:\cygwin64\bin\bash.exe")));
        assert!(is_cygwin_shell(Path::new(r"C:\cygwin\bin\bash.exe")));
        assert!(is_cygwin_shell(Path::new(
            r"D:\tools\cygwin64\bin\bash.exe"
        )));
        assert!(is_cygwin_shell(Path::new("C:/cygwin64/bin/bash.exe")));
        // Case-insensitive in both the drive and the `cygwin` segment.
        assert!(is_cygwin_shell(Path::new(r"C:\CygWin64\bin\BASH.EXE")));
    }

    #[test]
    fn test_is_cygwin_shell_rejects_non_cygwin() {
        assert!(!is_cygwin_shell(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(!is_cygwin_shell(Path::new(r"C:\msys64\usr\bin\bash.exe")));
        assert!(!is_cygwin_shell(Path::new("bash")));
        assert!(!is_cygwin_shell(Path::new(
            r"C:\Users\me\scoop\apps\git\current\bin\bash.exe"
        )));
        // A substring that is not a whole path segment must not match.
        assert!(!is_cygwin_shell(Path::new(
            r"C:\my-cygwinish-tools\bash.exe"
        )));
    }

    #[test]
    fn test_unix_path_to_windows_msys_drive_paths() {
        assert_eq!(unix_path_to_windows("/c/foo").as_deref(), Some(r"C:\foo"));
        assert_eq!(unix_path_to_windows("/C/foo").as_deref(), Some(r"C:\foo"));
        assert_eq!(
            unix_path_to_windows("/c/Program Files/Git").as_deref(),
            Some(r"C:\Program Files\Git")
        );
        assert_eq!(unix_path_to_windows("/c").as_deref(), Some(r"C:\"));
        assert_eq!(unix_path_to_windows("/c/").as_deref(), Some(r"C:\"));
    }

    #[test]
    fn test_unix_path_to_windows_cygdrive_paths() {
        assert_eq!(
            unix_path_to_windows("/cygdrive/c/foo").as_deref(),
            Some(r"C:\foo")
        );
        assert_eq!(unix_path_to_windows("/cygdrive/c").as_deref(), Some(r"C:\"));
        assert_eq!(unix_path_to_windows("/cygdrive"), None);
        // Not the cygdrive prefix — just a dir that starts with the same letters.
        assert_eq!(unix_path_to_windows("/cygdrive2/c/x"), None);
    }

    #[test]
    fn test_unix_path_to_windows_already_windows() {
        assert_eq!(
            unix_path_to_windows("C:/already").as_deref(),
            Some(r"C:\already")
        );
        assert_eq!(
            unix_path_to_windows(r"C:\already").as_deref(),
            Some(r"C:\already")
        );
    }

    #[test]
    fn test_unix_path_to_windows_unc() {
        assert_eq!(
            unix_path_to_windows("//server/share/dir").as_deref(),
            Some(r"\\server\share\dir")
        );
        assert_eq!(unix_path_to_windows("//"), None);
    }

    #[test]
    fn test_unix_path_to_windows_no_windows_equivalent() {
        assert_eq!(unix_path_to_windows("/usr/bin"), None);
        assert_eq!(unix_path_to_windows("/mingw64/bin"), None);
        assert_eq!(unix_path_to_windows("relative/x"), None);
        assert_eq!(unix_path_to_windows(""), None);
        // `/cc/foo` — two-letter first segment is not a drive.
        assert_eq!(unix_path_to_windows("/cc/foo"), None);
    }

    #[test]
    #[cfg(windows)]
    fn test_is_bash_basename_accepts_bash_variants() {
        use std::ffi::OsStr;
        assert!(is_bash_basename(OsStr::new("bash")));
        assert!(is_bash_basename(OsStr::new("bash.exe")));
        assert!(is_bash_basename(OsStr::new("BASH.EXE")));
        assert!(is_bash_basename(OsStr::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(is_bash_basename(OsStr::new("/usr/bin/bash")));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_bash_basename_rejects_other_shells() {
        use std::ffi::OsStr;
        assert!(!is_bash_basename(OsStr::new("sh")));
        assert!(!is_bash_basename(OsStr::new("zsh.exe")));
        assert!(!is_bash_basename(OsStr::new("fish")));
        assert!(!is_bash_basename(OsStr::new("dash")));
        assert!(!is_bash_basename(OsStr::new("cmd.exe")));
        assert!(!is_bash_basename(OsStr::new("bashfoo")));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_wsl_launcher_bash_detects_system32() {
        assert!(is_wsl_launcher_bash(Path::new(
            r"C:\Windows\System32\bash.exe"
        )));
        assert!(is_wsl_launcher_bash(Path::new(
            r"C:\WINDOWS\system32\bash.exe"
        )));
        assert!(is_wsl_launcher_bash(Path::new(
            r"D:\Windows\System32\bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_wsl_launcher_bash_detects_windows_apps() {
        assert!(is_wsl_launcher_bash(Path::new(
            r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\bash.exe"
        )));
        // Forward slashes still match — `which::which_in` may produce them.
        assert!(is_wsl_launcher_bash(Path::new(
            "C:/Users/me/AppData/Local/Microsoft/WindowsApps/bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_wsl_launcher_bash_accepts_real_bash() {
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\Program Files\Git\usr\bin\bash.exe"
        )));
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\msys64\usr\bin\bash.exe"
        )));
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\Users\me\scoop\apps\git\current\bin\bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_bash_candidates_includes_program_files() {
        let env = std::collections::BTreeMap::new();
        let candidates = bash_candidates(&env);
        assert!(candidates.contains(&PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")));
        assert!(candidates.contains(&PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe")));
    }

    #[test]
    #[cfg(windows)]
    fn test_bash_candidates_includes_msys2() {
        let env = std::collections::BTreeMap::new();
        let candidates = bash_candidates(&env);
        assert!(candidates.contains(&PathBuf::from(r"C:\msys64\usr\bin\bash.exe")));
        assert!(candidates.contains(&PathBuf::from(r"C:\msys32\usr\bin\bash.exe")));
    }

    #[test]
    #[cfg(windows)]
    fn test_bash_candidates_uses_localappdata_from_env() {
        let mut env = std::collections::BTreeMap::new();
        env.insert(
            "LOCALAPPDATA".to_string(),
            r"C:\Users\me\AppData\Local".to_string(),
        );
        let candidates = bash_candidates(&env);
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\me\AppData\Local\Programs\Git\bin\bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_uses_mise_bash_path_override() {
        // SAFETY: tests in this module run sequentially within the cargo test runner;
        // env mutation is scoped via a guard.
        let tmp = tempfile::tempdir().expect("tempdir");
        let bash_path = tmp.path().join("custom-bash.exe");
        std::fs::write(&bash_path, b"").expect("write fake bash");

        let mut env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
        env.insert(
            "MISE_BASH_PATH".to_string(),
            bash_path.to_string_lossy().into_owned(),
        );

        let resolved = resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env)
            .expect("override should resolve");
        assert_eq!(PathBuf::from(&resolved), bash_path);
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_override_beats_unix_form_path_gate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bash_path = tmp.path().join("custom-bash.exe");
        std::fs::write(&bash_path, b"").expect("write fake bash");

        // A Unix-form PATH normally ends resolution early, but the explicit
        // override still wins (e.g. `_.source` under mise running in Git Bash).
        let mut env = env_with_path("/c/foo:/d/bar");
        env.insert(
            "MISE_BASH_PATH".to_string(),
            bash_path.to_string_lossy().into_owned(),
        );

        let resolved = resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env)
            .expect("override should resolve");
        assert_eq!(PathBuf::from(&resolved), bash_path);
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_skips_when_not_posix_shell() {
        let env = env_with_path(r"C:\Windows\System32");
        assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("cmd.exe"), &env).is_none());
        assert!(
            resolve_posix_shell_program_path(std::ffi::OsStr::new("notepad.exe"), &env).is_none()
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_skips_when_path_already_unix() {
        let env = env_with_path("/c/foo:/d/bar");
        assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env).is_none());
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_honors_explicit_forward_slash_path() {
        // #9932: an explicit absolute bash path must be kept verbatim (None here,
        // so the caller keeps the original), NOT re-resolved to Git Bash via the
        // candidate list — even with a Windows-form PATH that would otherwise
        // trigger resolution.
        let env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
        assert!(
            resolve_posix_shell_program_path(
                std::ffi::OsStr::new("C:/msys64/usr/bin/bash.exe"),
                &env
            )
            .is_none()
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_honors_explicit_path_backslashes() {
        let env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
        assert!(
            resolve_posix_shell_program_path(
                std::ffi::OsStr::new(r"C:\msys64\usr\bin\bash.exe"),
                &env
            )
            .is_none()
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_honors_explicit_relative_path() {
        // A relative path with a separator is still an explicit choice, not a
        // bare name to look up on PATH.
        let env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
        assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("bin/bash"), &env).is_none());
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_honors_explicit_non_bash_shell_path() {
        // An explicit path to a non-bash POSIX shell is honored verbatim too.
        let env = env_with_path(r"C:\Windows\System32;C:\msys64\usr\bin");
        assert!(
            resolve_posix_shell_program_path(
                std::ffi::OsStr::new(r"C:\msys64\usr\bin\zsh.exe"),
                &env
            )
            .is_none()
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_program_has_directory_component_detects_explicit_paths() {
        use std::ffi::OsStr;
        assert!(program_has_directory_component(OsStr::new(
            "C:/msys64/usr/bin/bash.exe"
        )));
        assert!(program_has_directory_component(OsStr::new(
            r"C:\msys64\usr\bin\bash.exe"
        )));
        assert!(program_has_directory_component(OsStr::new("./bash")));
        assert!(program_has_directory_component(OsStr::new("bin/bash")));
        assert!(program_has_directory_component(OsStr::new("/usr/bin/bash")));
    }

    #[test]
    #[cfg(windows)]
    fn test_program_has_directory_component_rejects_bare_names() {
        use std::ffi::OsStr;
        assert!(!program_has_directory_component(OsStr::new("bash")));
        assert!(!program_has_directory_component(OsStr::new("bash.exe")));
        assert!(!program_has_directory_component(OsStr::new("BASH.EXE")));
    }
}
