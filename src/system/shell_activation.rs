//! `[bootstrap.mise_shell_activate]` — declarative mise shell activation
//! snippets, applied by `mise bootstrap shell apply` or `mise bootstrap`.

use std::path::PathBuf;

use crate::file;
use crate::system::edits::{BlockSource, EditOp, EditRequest};

#[derive(Debug, Clone)]
pub struct ShellActivationRequest {
    pub shell: ShellActivationShell,
    pub edit: EditRequest,
}

impl ShellActivationRequest {
    pub fn new(shell: ShellActivationShell) -> Self {
        let target_raw = shell.target_raw().to_string();
        Self {
            shell,
            edit: EditRequest {
                path_raw: target_raw.clone(),
                path: file::replace_path(&target_raw),
                id: "activate".to_string(),
                op: EditOp::Block {
                    source: BlockSource::Inline(shell.block().to_string()),
                    template: false,
                    comment: "#".to_string(),
                },
                base: PathBuf::from("."),
                config_path: PathBuf::from("[bootstrap.mise_shell_activate]"),
            },
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

    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        }
    }

    fn target_raw(self) -> &'static str {
        match self {
            Self::Bash => "~/.bashrc",
            Self::Zsh => "~/.zshrc",
            Self::Fish => "~/.config/fish/config.fish",
        }
    }

    fn block(self) -> &'static str {
        match self {
            Self::Bash => r#"eval "$(mise activate bash)""#,
            Self::Zsh => r#"eval "$(mise activate zsh)""#,
            Self::Fish => "mise activate fish | source",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_for_bash() {
        let request = ShellActivationRequest::new(ShellActivationShell::Bash);
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
    fn request_for_zsh() {
        let request = ShellActivationRequest::new(ShellActivationShell::Zsh);
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
    fn request_for_fish() {
        let request = ShellActivationRequest::new(ShellActivationShell::Fish);
        assert_eq!(request.edit.path_raw, "~/.config/fish/config.fish");
        match request.edit.op {
            EditOp::Block {
                source: BlockSource::Inline(block),
                ..
            } => assert_eq!(block, "mise activate fish | source"),
            _ => panic!("expected block edit"),
        }
    }
}
