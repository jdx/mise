//! `[bootstrap.user].login_shell` - declarative current-user login shell,
//! applied by `mise bootstrap user apply` or `mise bootstrap`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use crate::result::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShellRequest {
    pub shell: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginShellState {
    Set,
    Differs { current: String },
    MissingFromShells { current: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShellStatus {
    pub request: LoginShellRequest,
    pub user: String,
    pub current: String,
    pub shell_listed: bool,
    pub state: LoginShellState,
}

pub fn is_available() -> bool {
    cfg!(unix) && crate::file::which("chsh").is_some()
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
    let shell_listed = shell_is_listed(&request.shell)?;
    let state = login_shell_state(&current, &request.shell, shell_listed);
    Ok(LoginShellStatus {
        request: request.clone(),
        user: user.name,
        current,
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
    let user = target_user()?;
    let args = chsh_args(request, &user);
    ensure_shell_listed(&request.shell, dry_run)?;
    let display = shell_words::join(&args);
    if dry_run {
        miseprintln!("chsh {display}");
        return Ok(());
    }
    info!("$ chsh {display}");
    crate::cmd::CmdLineRunner::new("chsh")
        .args(args)
        .raw(true)
        .execute()
}

#[cfg(unix)]
fn target_user() -> Result<nix::unistd::User> {
    use eyre::eyre;
    use nix::unistd::{User, geteuid};

    if geteuid().is_root()
        && let Ok(sudo_user) = crate::env::var("SUDO_USER")
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
fn chsh_args(request: &LoginShellRequest, user: &nix::unistd::User) -> Vec<String> {
    chsh_args_for_user_name(request, &user.name)
}

#[cfg(unix)]
fn chsh_args_for_user_name(request: &LoginShellRequest, user_name: &str) -> Vec<String> {
    let mut args = vec!["-s".to_string(), request.shell.clone()];
    if nix::unistd::geteuid().is_root()
        && let Ok(sudo_user) = crate::env::var("SUDO_USER")
        && !sudo_user.is_empty()
        && sudo_user != "root"
    {
        args.push(user_name.to_string());
    }
    args
}

#[cfg(not(unix))]
fn chsh_args(request: &LoginShellRequest, _user: &TargetUser) -> Vec<String> {
    vec!["-s".to_string(), request.shell.clone()]
}

fn shells_file() -> PathBuf {
    crate::env::var("MISE_TEST_SHELLS_FILE")
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
            miseprintln!(
                "{}",
                shell_words::join(crate::system::sudo::argv("sh", &args))
            );
        }
        return Ok(());
    }
    match append_shell_directly(&path, shell) {
        Ok(()) => {
            info!("login_shell: added {shell} to {}", path.display());
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            crate::system::sudo::run("sh", &args, &[])
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
        let request = LoginShellRequest {
            shell: "/bin/zsh".to_string(),
        };
        let args = chsh_args_for_user_name(&request, "alice");
        if nix::unistd::geteuid().is_root() {
            assert_eq!(args, vec!["-s", "/bin/zsh", "alice"]);
        } else {
            assert_eq!(args, vec!["-s", "/bin/zsh"]);
        }
        unsafe { std::env::remove_var("SUDO_USER") };
    }
}
