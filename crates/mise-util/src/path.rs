pub use std::path::*;

use crate::dirs;

/// `s` with `/` rewritten to `\` on Windows.
///
/// For text a person reads, and nothing else. `Path::join` appends a multi-segment literal
/// verbatim, so `root.join(".git/hooks")` resolves correctly through `components()` while
/// `Display` shows the `/` it was given — and roots handed back by libraries arrive
/// `/`-separated already, so one printed path could switch form three times:
/// `C:/Users/me\proj\.git/hooks\pre-commit`. `/` is a separator on Windows and never part of a
/// name, so both spellings address the same file.
///
/// Deliberately *not* applied in [`PathExt::display_user`], which is not only a display helper:
/// `ToolRequest::Path::version()` builds `path:<display_user>` and lockfile entries are matched
/// on that string, and `system::edits` compares a user-supplied filter against one. Rewriting
/// there would change identity, not presentation.
///
/// Off Windows this returns its input: `\` is an ordinary filename character there.
pub fn settle_display_separators(s: String) -> String {
    match cfg!(windows) {
        true => s.replace('/', "\\"),
        false => s,
    }
}

/// `\\?\UNC\server\share\x` shown as `\\server\share\x`.
///
/// The half of the extended-length prefix `dunce::simplified` leaves behind: its
/// `is_safe_to_strip_unc` accepts `Prefix::VerbatimDisk` and nothing else, so a UNC path keeps a
/// prefix mise itself rejects as input. Verified reachable on `\\wsl.localhost\<distro>\…`, which
/// resolves perfectly well without it.
///
/// Declines in the three cases where the plain form would not name the same file:
///
/// - **a `/` in the remainder.** Inside `\\?\` a `/` is an ordinary character, so `a/b` is one
///   component; `\\server\share\a/b` is two. `display_path`'s tests already pin the same trap for
///   the disk prefix.
/// - **past `MAX_PATH`.** There the prefix is load-bearing after all.
/// - **a component ending in `.` or a space.** Those only resolve through the verbatim form.
///
/// Reserved names are *not* checked, though `dunce` declines them. It is handing back paths to open;
/// this is text to read, and a directory named `con` is addressable by its plain path — measured,
/// along with a task whose `dir` was `con` running in the right place. The disk-prefix case is
/// unaffected either way, since `dunce` has already declined it before this sees it.
#[cfg(windows)]
fn simplify_verbatim_unc(shown: String) -> String {
    const VERBATIM_UNC: &str = r"\\?\UNC\";

    let Some(rest) = shown.strip_prefix(VERBATIM_UNC) else {
        return shown;
    };
    let plain_len = 2 + rest.encode_utf16().count();
    if rest.contains('/')
        || plain_len >= crate::file::MAX_PATH
        || rest
            .split('\\')
            .any(|c| c.ends_with('.') || c.ends_with(' '))
    {
        return shown;
    }
    format!(r"\\{rest}")
}

#[cfg(not(windows))]
fn simplify_verbatim_unc(shown: String) -> String {
    shown
}

pub trait PathExt {
    /// replaces $HOME with "~", and drops a Windows extended-length prefix
    fn display_user(&self) -> String;
    fn mount(&self, on: &Path) -> PathBuf;
    fn is_empty(&self) -> bool;
}

impl PathExt for Path {
    /// The one place mise turns a path into text for a person to read, so the extended-length
    /// prefix `std::fs::canonicalize` leaves on Windows is dropped here rather than at each
    /// caller. mise refuses `\\?\` as *input* — see `toolset::tool_request::validate_path_string`,
    /// which calls extended-length and device paths unsupported — so handing one back in a message
    /// offers a path mise would not accept.
    ///
    /// `dunce::simplified` only strips the prefix from `\\?\C:\…` — `Prefix::VerbatimDisk` is the
    /// one kind its `is_safe_to_strip_unc` accepts, and every other verbatim form comes back
    /// untouched. Device paths, reserved names and paths past `MAX_PATH` should come back untouched,
    /// because those genuinely do not resolve without the prefix. A verbatim **UNC** path should
    /// not: `\\?\UNC\server\share\x` and `\\server\share\x` name the same file, and only the second
    /// is one mise would accept back — so [`simplify_verbatim_unc`] finishes the job.
    ///
    /// Separators are deliberately left as they are here — see [`settle_display_separators`],
    /// which `file::display_path` applies. This function also feeds strings that are matched
    /// rather than merely shown.
    fn display_user(&self) -> String {
        let path = dunce::simplified(self);
        let home = dirs::HOME.to_string_lossy();
        let home_str: &str = home.as_ref();
        let shown = match cfg!(unix) && path.starts_with(home_str) && home != "/" {
            true => path.to_string_lossy().replacen(home_str, "~", 1),
            false => path.to_string_lossy().to_string(),
        };
        simplify_verbatim_unc(shown)
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

/// Returns true if `program` is the path or basename of a POSIX-style shell.
/// Used on Windows to decide how a task's command line is built and which program
/// to resolve to an absolute path before spawning.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_posix_shell_program(program: &Path) -> bool {
    // `ash` is here because it is what `/bin/sh` is on Alpine, which mise ships musl builds
    // for — a task written against it reaches this by name, not through the `sh` symlink.
    const POSIX_SHELLS: &[&str] = &["bash", "sh", "zsh", "fish", "ksh", "dash", "ash"];
    let Some(stem) = program_stem(program) else {
        return false;
    };
    POSIX_SHELLS.iter().any(|name| *name == stem)
}

/// The `-c` payload that makes a shell *run* the path which follows it, treating the
/// arguments after that as the script's own.
///
/// A file task hands its shell a script path, and a shell left in `-c` mode reads whatever
/// follows as a command string instead. Without this in front of the path, the path *is* that
/// string: the task's own arguments land on `$0` onward and never reach the script, and on
/// Windows the backslashes are eaten as escapes before the path is even looked up.
///
/// `None` for the shells this does not apply to — `cmd`, whose `/c` already takes a program and
/// forwards its arguments, and PowerShell, which has no `$0`/`$@` and already works.
pub fn command_mode_script_payload(program: &Path) -> Option<&'static str> {
    // fish counts as POSIX for [`is_posix_shell_program`]'s question (it wants a Unix-style
    // PATH) but not for this one: it has no `$0`/`$@` and rejects `$@` outright. `$argv` is the
    // whole argument list, and running it runs its first element with the rest as arguments.
    if program_stem(program).as_deref() == Some("fish") {
        return Some("$argv");
    }
    is_posix_shell_program(program).then_some(r#""$0" "$@""#)
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

/// Returns true if `program` is PowerShell (`pwsh` / PowerShell Core) or Windows
/// PowerShell (`powershell`), with or without a directory prefix or `.exe`
/// extension.
pub fn is_powershell_program(program: &Path) -> bool {
    matches!(
        program_stem(program).as_deref(),
        Some("pwsh" | "powershell")
    )
}

/// If `shell` invokes PowerShell and does not already suppress startup profiles,
/// insert `-NoProfile` immediately after the program.
///
/// Unlike `zsh -c` / `sh -c`, `pwsh -Command` loads the user's PowerShell
/// profile even for a non-interactive one-liner. A profile that mutates `PATH`
/// (e.g. mise activation prepending the shims dir) can shadow a task's own
/// installed tools, producing confusing "cannot find binary path" failures
/// (discussion #10956). Skipping the profile makes mise-spawned PowerShell
/// behave like the POSIX shells it spawns elsewhere.
///
/// `-NoProfile` must precede `-Command`/`-File`, since everything after those is
/// treated as the script/args rather than as pwsh options — hence insertion at
/// index 1, right after the program.
///
/// Detection is idempotent and covers PowerShell's case-insensitive prefix
/// abbreviations (`-nop`, `-NoProfile`, `/noprofile`, …). `-NoProfileLoadTime`
/// is deliberately *not* treated as suppressing the profile — it only affects
/// startup timing output — so it does not block injection.
pub fn inject_powershell_no_profile(shell: &mut Vec<String>) {
    let Some(program) = shell.first() else {
        return;
    };
    if !is_powershell_program(Path::new(program)) {
        return;
    }
    let already_present = shell[1..]
        .iter()
        .take_while(|arg| {
            let token = arg.trim_start_matches(['-', '/']).to_ascii_lowercase();
            // PowerShell treats everything after -Command/-File (and their
            // abbreviations) as payload, so a payload argument such as `-nop`
            // must not suppress injection.
            !(!token.is_empty()
                && ("command".starts_with(&token)
                    || "commandwithargs".starts_with(&token)
                    || "file".starts_with(&token)))
        })
        .any(|arg| {
            let token = arg.trim_start_matches(['-', '/']).to_ascii_lowercase();
            // `-nop`, `-nopro`, …, `-noprofile` are all abbreviations of NoProfile.
            // Require at least "nop" to avoid matching unrelated `-no*` flags, and
            // stop at "noprofile" so longer names like NoProfileLoadTime don't match.
            token.len() >= 3 && "noprofile".starts_with(&token)
        });
    if !already_present {
        shell.insert(1, "-NoProfile".to_string());
    }
}

/// Assemble the args (everything after the `cmd.exe` program) for running
/// `script` — plus any forwarded `args` — through cmd.exe *verbatim*.
///
/// Returns the cmd switches from `shell_flags` (with `/s` ensured at the front),
/// followed by the whole command wrapped in a single outer double-quote pair.
/// The caller must append these to the command line as *raw* args (e.g.
/// mise's `CmdLineRunner::raw_arg` / `Command::raw_arg`) so std does not
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
pub fn quote_arg_for_cmd_body(arg: &str) -> String {
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

/// Convert a single MSYS2/Git Bash (`/c/foo`) or Cygwin (`/cygdrive/c/foo`) style
/// absolute path entry back to Windows form (`C:\foo`), used when reading paths
/// *back* from a POSIX shell (e.g. PATH entries a sourced `[env] _.source`
/// script prepended).
///
/// Returns `None` when the entry has no recognizable Windows equivalent
/// (`/usr/bin`, `/mingw64/bin`, relative paths, empty strings, ...). A custom
/// fstab cygdrive mount root (e.g. `/mnt`) is not recognized either — callers
/// skip such entries.
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

/// On Windows, when about to spawn a POSIX shell — for a task, or to source an
/// `[env] _.source` script — resolve the program to its absolute path using the
/// PATH from the child env.
///
/// Why: `Command::spawn` on Windows uses the *child* env's PATH (when set via
/// `.envs(...)`) to locate the program, so which `bash` runs would otherwise
/// depend on how Win32 happens to search that PATH. Resolving here pins the
/// choice and hands the child an absolute path instead. See discussion #6513.
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
/// already in Unix form (no `;` and no `\`, meaning mise is itself running
/// inside a POSIX shell, whose own lookup is the one to use), `which`
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
mod tests;
