//! `[bootstrap.mise_shell_activate]` — declarative mise shell activation
//! snippets, applied by `mise bootstrap mise-shell-activate apply` or `mise bootstrap`.

use std::path::PathBuf;

use crate::file;
use crate::system::edits::{BlockSource, EditOp, EditRequest};

#[derive(Debug, Clone)]
pub struct ShellActivationRequest {
    pub target: ShellActivationTarget,
    pub shell: ShellActivationShell,
    pub mode: ShellActivationMode,
    pub edit: EditRequest,
}

impl ShellActivationRequest {
    pub fn new(target: ShellActivationTarget, mode: ShellActivationMode) -> Self {
        let shell = target.shell();
        let target_raw = target.target_raw().to_string();
        Self {
            target,
            shell,
            mode,
            edit: EditRequest {
                path_raw: target_raw.clone(),
                path: file::replace_path(&target_raw),
                id: "activate".to_string(),
                op: EditOp::Block {
                    source: BlockSource::Inline(target.block(mode).to_string()),
                    template: false,
                    comment: "#".to_string(),
                },
                base: PathBuf::from("."),
                config_path: PathBuf::from("[bootstrap.mise_shell_activate]"),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShellActivationMode {
    Activate,
    Shims,
}

impl ShellActivationMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "activate" => Some(Self::Activate),
            "shims" => Some(Self::Shims),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::Shims => "shims",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum ShellActivationShell {
    Bash,
    Zsh,
    Fish,
}

impl ShellActivationShell {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    pub fn default_targets(self) -> &'static [ShellActivationTarget] {
        match self {
            Self::Bash => &[
                ShellActivationTarget::BashProfile,
                ShellActivationTarget::Bashrc,
            ],
            Self::Zsh => &[
                ShellActivationTarget::Zprofile,
                ShellActivationTarget::Zshrc,
            ],
            Self::Fish => &[ShellActivationTarget::Fish],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum ShellActivationTarget {
    BashProfile,
    Bashrc,
    Zshenv,
    Zprofile,
    Zshrc,
    Fish,
}

impl ShellActivationTarget {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bash_profile" => Some(Self::BashProfile),
            "bashrc" => Some(Self::Bashrc),
            "zshenv" => Some(Self::Zshenv),
            "zprofile" => Some(Self::Zprofile),
            "zshrc" => Some(Self::Zshrc),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    pub fn expected_keys() -> &'static str {
        "bash, zsh, fish, bash_profile, bashrc, zshenv, zprofile, or zshrc"
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::BashProfile => "bash_profile",
            Self::Bashrc => "bashrc",
            Self::Zshenv => "zshenv",
            Self::Zprofile => "zprofile",
            Self::Zshrc => "zshrc",
            Self::Fish => "fish",
        }
    }

    pub fn shell(self) -> ShellActivationShell {
        match self {
            Self::BashProfile | Self::Bashrc => ShellActivationShell::Bash,
            Self::Zshenv | Self::Zprofile | Self::Zshrc => ShellActivationShell::Zsh,
            Self::Fish => ShellActivationShell::Fish,
        }
    }

    pub fn default_mode(self) -> ShellActivationMode {
        match self {
            Self::BashProfile | Self::Zshenv | Self::Zprofile => ShellActivationMode::Shims,
            Self::Bashrc | Self::Zshrc | Self::Fish => ShellActivationMode::Activate,
        }
    }

    fn target_raw(self) -> &'static str {
        match self {
            Self::BashProfile => "~/.bash_profile",
            Self::Bashrc => "~/.bashrc",
            Self::Zshenv => "~/.zshenv",
            Self::Zprofile => "~/.zprofile",
            Self::Zshrc => "~/.zshrc",
            Self::Fish => "~/.config/fish/config.fish",
        }
    }

    fn block(self, mode: ShellActivationMode) -> &'static str {
        match (self.shell(), mode) {
            (ShellActivationShell::Bash, ShellActivationMode::Activate) => {
                r#"eval "$(mise activate bash)""#
            }
            (ShellActivationShell::Bash, ShellActivationMode::Shims) => {
                r#"eval "$(mise activate bash --shims)""#
            }
            (ShellActivationShell::Zsh, ShellActivationMode::Activate) => {
                r#"eval "$(mise activate zsh)""#
            }
            (ShellActivationShell::Zsh, ShellActivationMode::Shims) => {
                r#"eval "$(mise activate zsh --shims)""#
            }
            (ShellActivationShell::Fish, ShellActivationMode::Activate) => {
                "mise activate fish | source"
            }
            (ShellActivationShell::Fish, ShellActivationMode::Shims) => {
                "mise activate fish --shims | source"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_for_bashrc_activate() {
        let request = ShellActivationRequest::new(
            ShellActivationTarget::Bashrc,
            ShellActivationMode::Activate,
        );
        assert_eq!(request.target, ShellActivationTarget::Bashrc);
        assert_eq!(request.shell, ShellActivationShell::Bash);
        assert_eq!(request.mode, ShellActivationMode::Activate);
        assert_eq!(request.edit.path_raw, "~/.bashrc");
        assert_eq!(request.edit.id, "activate");
        match request.edit.op {
            EditOp::Block {
                source: BlockSource::Inline(block),
                template,
                comment,
            } => {
                assert_eq!(block, r#"eval "$(mise activate bash)""#);
                assert!(!template);
                assert_eq!(comment, "#");
            }
            _ => panic!("expected block edit"),
        }
    }

    #[test]
    fn request_for_zprofile_shims() {
        let request = ShellActivationRequest::new(
            ShellActivationTarget::Zprofile,
            ShellActivationMode::Shims,
        );
        assert_eq!(request.edit.path_raw, "~/.zprofile");
        match request.edit.op {
            EditOp::Block {
                source: BlockSource::Inline(block),
                ..
            } => assert_eq!(block, r#"eval "$(mise activate zsh --shims)""#),
            _ => panic!("expected block edit"),
        }
    }

    #[test]
    fn request_for_zshrc_activate() {
        let request = ShellActivationRequest::new(
            ShellActivationTarget::Zshrc,
            ShellActivationMode::Activate,
        );
        assert_eq!(request.edit.path_raw, "~/.zshrc");
        match request.edit.op {
            EditOp::Block {
                source: BlockSource::Inline(block),
                ..
            } => assert_eq!(block, r#"eval "$(mise activate zsh)""#),
            _ => panic!("expected block edit"),
        }
    }

    #[test]
    fn request_for_fish_activate() {
        let request =
            ShellActivationRequest::new(ShellActivationTarget::Fish, ShellActivationMode::Activate);
        assert_eq!(request.edit.path_raw, "~/.config/fish/config.fish");
        match request.edit.op {
            EditOp::Block {
                source: BlockSource::Inline(block),
                ..
            } => assert_eq!(block, "mise activate fish | source"),
            _ => panic!("expected block edit"),
        }
    }

    #[test]
    fn request_for_fish_shims() {
        let request =
            ShellActivationRequest::new(ShellActivationTarget::Fish, ShellActivationMode::Shims);
        match request.edit.op {
            EditOp::Block {
                source: BlockSource::Inline(block),
                ..
            } => assert_eq!(block, "mise activate fish --shims | source"),
            _ => panic!("expected block edit"),
        }
    }
}
