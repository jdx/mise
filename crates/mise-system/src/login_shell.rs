//! `[bootstrap.user].login_shell` - declarative current-user login shell,
//! applied by `mise bootstrap user apply` or `mise bootstrap`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use eyre::Result;

/// The shell as written in config: an absolute path, or a bare name such as
/// `fish` that is resolved to an executable when the shell is checked or applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShellRequest {
    pub shell: String,
}

impl LoginShellRequest {
    /// Absolute path to apply, or `None` when a bare name is not installed yet.
    pub fn resolve(&self) -> Option<String> {
        if Path::new(&self.shell).is_absolute() {
            return Some(self.shell.clone());
        }
        resolve_shell_name(&self.shell, &search_dirs()).map(|p| p.to_string_lossy().to_string())
    }
}

/// Directories package managers install into that may not be in the invoking
/// process's `PATH`, such as right after Homebrew was installed.
const EXTRA_SEARCH_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/home/linuxbrew/.linuxbrew/bin",
    "/usr/bin",
    "/bin",
];

fn search_dirs() -> Vec<PathBuf> {
    search_dirs_from(
        &mise_util::env::PATH,
        mise_util::file::is_mise_shims_dir,
        target_home().as_deref(),
    )
}

/// Home of the account `chsh` changes, which is not `$HOME` under `sudo`.
fn target_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        target_user().ok().map(|user| user.dir)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// `path` without mise shim directories, followed by the package-manager locations.
fn search_dirs_from(
    path: &[PathBuf],
    is_shims_dir: impl Fn(&Path) -> bool,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = path
        .iter()
        .filter(|dir| dir.is_absolute())
        .cloned()
        .collect();
    if let Some(home) = home {
        dirs.push(home.join(".linuxbrew/bin"));
    }
    dirs.extend(EXTRA_SEARCH_DIRS.iter().map(PathBuf::from));
    dirs.retain(|dir| !is_shims_dir(dir));
    dirs
}

/// First executable regular file called `name` in `dirs`. Never a mise shim.
fn resolve_shell_name(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginShellState {
    /// A bare shell name that no installed executable matches yet
    Unresolved,
    Set,
    Differs {
        current: String,
    },
    MissingFromShells {
        current: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShellStatus {
    pub request: LoginShellRequest,
    pub user: String,
    pub current: String,
    /// Absolute path the request resolves to, if any
    pub resolved: Option<String>,
    pub shell_listed: bool,
    pub state: LoginShellState,
}

pub fn is_available() -> bool {
    cfg!(unix) && mise_util::file::which("chsh").is_some()
}

pub fn unavailable_reason() -> String {
    if cfg!(unix) {
        "`chsh` not found".to_string()
    } else {
        "only available on unix".to_string()
    }
}

pub fn status(request: &LoginShellRequest) -> Result<LoginShellStatus> {
    let user = target_user()?;
    let current = display_shell(user.shell);
    let resolved = request.resolve();
    let (shell_listed, state) = match &resolved {
        Some(shell) => {
            let listed = shell_is_listed(shell)?;
            (listed, login_shell_state(&current, shell, listed))
        }
        None => (false, LoginShellState::Unresolved),
    };
    Ok(LoginShellStatus {
        request: request.clone(),
        user: user.name,
        current,
        resolved,
        shell_listed,
        state,
    })
}

fn login_shell_state(current: &str, requested: &str, shell_listed: bool) -> LoginShellState {
    if current == requested && shell_listed {
        LoginShellState::Set
    } else if current == requested {
        LoginShellState::MissingFromShells {
            current: current.to_string(),
        }
    } else {
        LoginShellState::Differs {
            current: current.to_string(),
        }
    }
}

pub fn apply(request: &LoginShellRequest, dry_run: bool) -> Result<()> {
    let Some(shell) = request.resolve() else {
        if dry_run {
            miseprintln!(
                "# login_shell: `{}` is not installed yet, its path is resolved after installation",
                request.shell
            );
            return Ok(());
        }
        eyre::bail!(
            "login_shell: could not find an executable named `{}`, leaving the login shell unchanged",
            request.shell
        );
    };
    let user = target_user()?;
    let args = chsh_args(&shell, &user);
    ensure_shell_listed(&shell, dry_run)?;
    let display = shell_words::join(&args);
    if dry_run {
        miseprintln!("chsh {display}");
        return Ok(());
    }
    info!("$ chsh {display}");
    mise_util::cmd::CmdLineRunner::new("chsh")
        .args(args)
        .raw(true)
        .execute()
}

#[cfg(unix)]
fn target_user() -> Result<nix::unistd::User> {
    use eyre::eyre;
    use nix::unistd::{User, geteuid};

    if geteuid().is_root()
        && let Ok(sudo_user) = mise_util::env::var("SUDO_USER")
        && !sudo_user.is_empty()
        && sudo_user != "root"
    {
        return User::from_name(&sudo_user)?
            .ok_or_else(|| eyre!("failed to find user from SUDO_USER={sudo_user}"));
    }
    let uid = geteuid();
    User::from_uid(uid)?.ok_or_else(|| eyre!("failed to find user for uid {uid}"))
}

#[cfg(not(unix))]
fn target_user() -> Result<TargetUser> {
    eyre::bail!("{}", unavailable_reason())
}

#[cfg(not(unix))]
struct TargetUser {
    name: String,
    shell: PathBuf,
}

fn display_shell(shell: PathBuf) -> String {
    shell.to_string_lossy().to_string()
}

#[cfg(unix)]
fn chsh_args(shell: &str, user: &nix::unistd::User) -> Vec<String> {
    chsh_args_for_user_name(shell, &user.name)
}

#[cfg(unix)]
fn chsh_args_for_user_name(shell: &str, user_name: &str) -> Vec<String> {
    let mut args = vec!["-s".to_string(), shell.to_string()];
    if nix::unistd::geteuid().is_root()
        && let Ok(sudo_user) = mise_util::env::var("SUDO_USER")
        && !sudo_user.is_empty()
        && sudo_user != "root"
    {
        args.push(user_name.to_string());
    }
    args
}

#[cfg(not(unix))]
fn chsh_args(shell: &str, _user: &TargetUser) -> Vec<String> {
    vec!["-s".to_string(), shell.to_string()]
}

fn shells_file() -> PathBuf {
    mise_util::env::var("MISE_TEST_SHELLS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/shells"))
}

fn shell_is_listed(shell: &str) -> Result<bool> {
    Ok(shells_file_contents()?
        .lines()
        .filter_map(shells_line_entry)
        .any(|entry| entry == shell))
}

fn shells_file_contents() -> Result<String> {
    match std::fs::read_to_string(shells_file()) {
        Ok(contents) => Ok(contents),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.into()),
    }
}

fn shells_line_entry(line: &str) -> Option<&str> {
    let line = line.split_once('#').map_or(line, |(entry, _)| entry).trim();
    (!line.is_empty()).then_some(line)
}

fn ensure_shell_listed(shell: &str, dry_run: bool) -> Result<()> {
    if shell_is_listed(shell)? {
        return Ok(());
    }
    let path = shells_file();
    let args = vec![
        "-c".to_string(),
        r#"printf '%s\n' "$1" >> "$2""#.to_string(),
        "sh".to_string(),
        shell.to_string(),
        path.to_string_lossy().to_string(),
    ];
    if dry_run {
        if can_append_shell_directly(&path) {
            miseprintln!("sh {}", shell_words::join(&args));
        } else {
            miseprintln!("{}", shell_words::join(crate::sudo::argv("sh", &args)));
        }
        return Ok(());
    }
    match append_shell_directly(&path, shell) {
        Ok(()) => {
            info!("login_shell: added {shell} to {}", path.display());
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            crate::sudo::run("sh", &args, &[])
        }
        Err(err) => Err(err.into()),
    }
}

fn append_shell_directly(path: &PathBuf, shell: &str) -> std::io::Result<()> {
    let needs_leading_newline = std::fs::metadata(path).is_ok_and(|m| m.len() > 0)
        && !shells_file_contents()
            .unwrap_or_default()
            .ends_with(['\n', '\r']);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if needs_leading_newline {
        writeln!(file)?;
    }
    writeln!(file, "{shell}")
}

fn can_append_shell_directly(path: &PathBuf) -> bool {
    OpenOptions::new().append(true).open(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_shell() {
        assert_eq!(display_shell(PathBuf::from("/bin/zsh")), "/bin/zsh");
    }

    #[test]
    fn test_status_state_set_and_differs() {
        assert_eq!(
            login_shell_state("/bin/zsh", "/bin/zsh", true),
            LoginShellState::Set
        );
        assert_eq!(
            login_shell_state("/bin/zsh", "/bin/zsh", false),
            LoginShellState::MissingFromShells {
                current: "/bin/zsh".to_string()
            }
        );
        assert_eq!(
            login_shell_state("/bin/bash", "/bin/zsh", true),
            LoginShellState::Differs {
                current: "/bin/bash".to_string()
            }
        );
    }

    #[test]
    fn test_resolve_shell_name() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().to_path_buf();
        assert_eq!(resolve_shell_name("fish", std::slice::from_ref(&bin)), None);
        let fish = bin.join("fish");
        std::fs::write(&fish, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // not executable yet
            assert_eq!(resolve_shell_name("fish", std::slice::from_ref(&bin)), None);
            std::fs::set_permissions(&fish, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            resolve_shell_name("fish", std::slice::from_ref(&bin)),
            Some(fish)
        );
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_shell_name("fish", &[other.path().to_path_buf(), bin.clone()]),
            Some(bin.join("fish"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_search_dirs_skip_shims_and_keep_order() {
        let tmp = tempfile::tempdir().unwrap();
        let shims = tmp.path().join("shims");
        let link = tmp.path().join("shims-link");
        let other = tmp.path().join("bin");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&shims, &link).unwrap();
        let path = vec![link, other.clone(), shims.clone()];
        let canon = |d: &Path| d.canonicalize().unwrap_or_else(|_| d.to_path_buf());
        let shims_canon = canon(&shims);
        let dirs = search_dirs_from(
            &path,
            |d| canon(d) == shims_canon,
            Some(Path::new("/home/u")),
        );
        assert_eq!(dirs[0], other);
        assert_eq!(dirs[1], PathBuf::from("/home/u/.linuxbrew/bin"));
        assert_eq!(dirs[2], PathBuf::from(EXTRA_SEARCH_DIRS[0]));
        assert!(
            !dirs
                .iter()
                .any(|d| d.ends_with("shims") || d.ends_with("shims-link"))
        );
    }

    #[test]
    fn test_absolute_request_resolves_to_itself() {
        let request = LoginShellRequest {
            shell: "/opt/homebrew/bin/fish".to_string(),
        };
        assert_eq!(request.resolve().as_deref(), Some("/opt/homebrew/bin/fish"));
    }

    #[test]
    fn test_shells_line_entry() {
        assert_eq!(shells_line_entry("/bin/zsh"), Some("/bin/zsh"));
        assert_eq!(shells_line_entry(" /bin/fish # comment"), Some("/bin/fish"));
        assert_eq!(shells_line_entry("# comment"), None);
        assert_eq!(shells_line_entry("   "), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_chsh_args_for_root_sudo_user() {
        unsafe { std::env::set_var("SUDO_USER", "alice") };
        let args = chsh_args_for_user_name("/bin/zsh", "alice");
        if nix::unistd::geteuid().is_root() {
            assert_eq!(args, vec!["-s", "/bin/zsh", "alice"]);
        } else {
            assert_eq!(args, vec!["-s", "/bin/zsh"]);
        }
        unsafe { std::env::remove_var("SUDO_USER") };
    }
}
