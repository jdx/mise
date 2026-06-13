//! `[system].login_shell` - declarative current-user login shell, applied by
//! `mise system install` or `mise bootstrap`.

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShellStatus {
    pub request: LoginShellRequest,
    pub current: String,
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
    let current = current_login_shell()?;
    let state = if current == request.shell {
        LoginShellState::Set
    } else {
        LoginShellState::Differs {
            current: current.clone(),
        }
    };
    Ok(LoginShellStatus {
        request: request.clone(),
        current,
        state,
    })
}

pub fn apply(request: &LoginShellRequest, dry_run: bool) -> Result<()> {
    let args = vec!["-s".to_string(), request.shell.clone()];
    let display = shell_words::join(&args);
    if dry_run {
        miseprintln!("chsh {display}");
        return Ok(());
    }
    info!("$ chsh {display}");
    crate::cmd::CmdLineRunner::new("chsh")
        .arg("-s")
        .arg(&request.shell)
        .raw(true)
        .execute()
}

#[cfg(unix)]
fn current_login_shell() -> Result<String> {
    use eyre::eyre;
    use nix::unistd::{User, geteuid};

    let uid = geteuid();
    let user = User::from_uid(uid)?.ok_or_else(|| eyre!("failed to find user for uid {uid}"))?;
    Ok(display_shell(user.shell))
}

#[cfg(not(unix))]
fn current_login_shell() -> Result<String> {
    eyre::bail!("{}", unavailable_reason())
}

fn display_shell(shell: PathBuf) -> String {
    shell.to_string_lossy().to_string()
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
        let request = LoginShellRequest {
            shell: "/bin/zsh".to_string(),
        };
        let set = if "/bin/zsh" == request.shell {
            LoginShellState::Set
        } else {
            LoginShellState::Differs {
                current: "/bin/zsh".to_string(),
            }
        };
        assert_eq!(set, LoginShellState::Set);

        let differs = if "/bin/bash" == request.shell {
            LoginShellState::Set
        } else {
            LoginShellState::Differs {
                current: "/bin/bash".to_string(),
            }
        };
        assert_eq!(
            differs,
            LoginShellState::Differs {
                current: "/bin/bash".to_string()
            }
        );
    }
}
