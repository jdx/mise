//! Detect active AI coding agents from environment variables.
//!
//! This crate is intentionally small and conservative. It detects agent-driven
//! command execution, not merely an interactive terminal inside an editor that
//! happens to offer AI features.

#![forbid(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;

/// An AI coding agent known to the detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Agent {
    Amp,
    AmazonQ,
    Antigravity,
    Augment,
    ClaudeCode,
    ClaudeCowork,
    Cline,
    CodeBuddy,
    Codex,
    Crush,
    Cursor,
    Firebender,
    GeminiCli,
    GitHubCopilot,
    Goose,
    Grok,
    IflowCli,
    Kiro,
    OpenCode,
    OpenHands,
    Pi,
    QwenCode,
    Replit,
    RooCode,
    Trae,
    VeCli,
    Warp,
    /// An agent identified only through the vendor-neutral `AI_AGENT`
    /// variable.
    Other,
}

impl Agent {
    /// A stable, lowercase identifier suitable for logs and serialized output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Amp => "amp",
            Self::AmazonQ => "amazon-q",
            Self::Antigravity => "antigravity",
            Self::Augment => "augment",
            Self::ClaudeCode => "claude-code",
            Self::ClaudeCowork => "claude-cowork",
            Self::Cline => "cline",
            Self::CodeBuddy => "codebuddy",
            Self::Codex => "codex",
            Self::Crush => "crush",
            Self::Cursor => "cursor",
            Self::Firebender => "firebender",
            Self::GeminiCli => "gemini-cli",
            Self::GitHubCopilot => "github-copilot",
            Self::Goose => "goose",
            Self::Grok => "grok",
            Self::IflowCli => "iflow-cli",
            Self::Kiro => "kiro",
            Self::OpenCode => "opencode",
            Self::OpenHands => "openhands",
            Self::Pi => "pi",
            Self::QwenCode => "qwen-code",
            Self::Replit => "replit",
            Self::RooCode => "roo-code",
            Self::Trae => "trae",
            Self::VeCli => "vecli",
            Self::Warp => "warp",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The result of agent detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Detection {
    pub agent: Agent,
    /// The environment variable that established the detection.
    ///
    /// Its value is intentionally not retained because some agent markers can
    /// contain session identifiers or credentials.
    pub signal: &'static str,
}

/// Detect an agent in the current process environment.
pub fn detect() -> Option<Detection> {
    detect_with_env(|name| env::var_os(name))
}

/// Return whether the current process appears to be driven by an AI agent.
pub fn is_agent() -> bool {
    detect().is_some()
}

/// Detect an agent using a caller-supplied environment lookup.
///
/// This is useful for deterministic tests and for programs that already hold
/// an environment snapshot.
pub fn detect_with_env<F>(env: F) -> Option<Detection>
where
    F: Fn(&str) -> Option<OsString>,
{
    let value = |name| env(name).filter(|value| !value.is_empty());
    let present = |name| value(name).is_some();
    let enabled = |name| value(name).is_some_and(|value| !is_false(&value));
    let equals = |name, expected| value(name).is_some_and(|value| value == OsStr::new(expected));
    let contains =
        |name, needle| value(name).is_some_and(|value| value.to_string_lossy().contains(needle));

    // A named generic value is authoritative. A flag-like value proves an
    // agent is present but lets a specific marker below identify it first.
    let mut generic = None;
    for signal in ["AI_AGENT"] {
        if let Some(raw) = value(signal)
            && !is_false(&raw)
        {
            let mut agent = classify_generic(&raw);
            if agent == Agent::ClaudeCode && present("CLAUDE_CODE_IS_COWORK") {
                agent = Agent::ClaudeCowork;
            }
            let detection = Detection { agent, signal };
            if agent != Agent::Other {
                return Some(detection);
            }
            generic.get_or_insert(detection);
        }
    }

    // More-specific derivatives precede the agents whose compatibility
    // variables they inherit.
    const SIGNALS: &[(&str, Agent)] = &[
        ("AMP_CURRENT_THREAD_ID", Agent::Amp),
        ("CODEBUDDY", Agent::CodeBuddy),
        ("CODEBUDDY_SESSION_ID", Agent::CodeBuddy),
        ("CODEBUDDY_PROJECT_DIR", Agent::CodeBuddy),
        ("CLAUDE_CODE_IS_COWORK", Agent::ClaudeCowork),
        ("CLAUDECODE", Agent::ClaudeCode),
        ("CLAUDE_CODE", Agent::ClaudeCode),
        ("CLAUDE_CODE_ENTRYPOINT", Agent::ClaudeCode),
        ("CLAUDE_CODE_SESSION_ID", Agent::ClaudeCode),
        ("CLAUDE_CODE_EXECPATH", Agent::ClaudeCode),
        ("CURSOR_AGENT", Agent::Cursor),
        ("CURSOR_SANDBOX", Agent::Cursor),
        ("QWEN_CODE", Agent::QwenCode),
        ("VECLI_SANDBOX", Agent::VeCli),
        ("VECLI_DIR", Agent::VeCli),
        ("GEMINI_CLI", Agent::GeminiCli),
        ("CODEX_THREAD_ID", Agent::Codex),
        ("CODEX_SANDBOX", Agent::Codex),
        ("CODEX_CI", Agent::Codex),
        ("CODEX_SANDBOX_NETWORK_DISABLED", Agent::Codex),
        ("ANTIGRAVITY_AGENT", Agent::Antigravity),
        ("AUGMENT_AGENT", Agent::Augment),
        ("CLINE_ACTIVE", Agent::Cline),
        ("CLINE_TASK_ID", Agent::Cline),
        ("ROO_CODE_TASK_ID", Agent::RooCode),
        ("CRUSH", Agent::Crush),
        ("IFLOW_CLI", Agent::IflowCli),
        ("GROK_AGENT", Agent::Grok),
        ("OZ_RUN_ID", Agent::Warp),
        ("PI_CODING_AGENT", Agent::Pi),
        ("KIRO_AGENT_PATH", Agent::Kiro),
        ("FIREBENDER_TERMINAL", Agent::Firebender),
        ("OPENCODE", Agent::OpenCode),
        ("OPENCODE_PID", Agent::OpenCode),
        ("OPENCODE_BIN_PATH", Agent::OpenCode),
        ("OPENCODE_SERVER", Agent::OpenCode),
        ("OPENCODE_APP_INFO", Agent::OpenCode),
        ("OPENCODE_MODES", Agent::OpenCode),
        ("OPENCODE_CLIENT", Agent::OpenCode),
        ("TRAE_AI_SHELL_ID", Agent::Trae),
        ("GOOSE_TERMINAL", Agent::Goose),
        ("COPILOT_AGENT_SESSION_ID", Agent::GitHubCopilot),
        ("COPILOT_AGENT", Agent::GitHubCopilot),
        ("COPILOT_CLI", Agent::GitHubCopilot),
        ("COPILOT_AGENT_JOB_ID", Agent::GitHubCopilot),
    ];
    if let Some(&(signal, agent)) = SIGNALS.iter().find(|(signal, _)| enabled(signal)) {
        return Some(Detection { agent, signal });
    }

    if equals("CURSOR_EXTENSION_HOST_ROLE", "agent-exec") {
        return Some(Detection {
            agent: Agent::Cursor,
            signal: "CURSOR_EXTENSION_HOST_ROLE",
        });
    }
    if present("CURSOR_TRACE_ID") && equals("PAGER", "head -n 10000 | cat") {
        return Some(Detection {
            agent: Agent::Cursor,
            signal: "CURSOR_TRACE_ID",
        });
    }
    if contains("AWS_EXECUTION_ENV", "AmazonQ-For-CLI") {
        return Some(Detection {
            agent: Agent::AmazonQ,
            signal: "AWS_EXECUTION_ENV",
        });
    }
    if present("AGENT_CONTEXT_OUT") && present("AGENT_DISPLAY_OUT") {
        return Some(Detection {
            agent: Agent::Kiro,
            signal: "AGENT_CONTEXT_OUT",
        });
    }
    if equals("REPLIT_MODE", "assistant") && present("REPL_ID") {
        return Some(Detection {
            agent: Agent::Replit,
            signal: "REPLIT_MODE",
        });
    }
    for signal in ["PS1", "PROMPT_COMMAND"] {
        if contains(signal, "###PS1JSON###") {
            return Some(Detection {
                agent: Agent::OpenHands,
                signal,
            });
        }
    }

    generic
}

fn is_false(value: &OsStr) -> bool {
    matches!(
        value.to_string_lossy().trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn classify_generic(value: &OsStr) -> Agent {
    let value = value.to_string_lossy();
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "github_copilot_vscode_agent" {
        return Agent::GitHubCopilot;
    }
    let name = normalized
        .split('@')
        .next()
        .unwrap_or(&normalized)
        .split('_')
        .next()
        .unwrap_or(&normalized);
    match name {
        "amp" => Agent::Amp,
        "amazonq" | "amazon-q" | "amazon-q-cli" => Agent::AmazonQ,
        "antigravity" => Agent::Antigravity,
        "augment" | "augment-cli" => Agent::Augment,
        "claude" | "claude-code" | "claudecode" => Agent::ClaudeCode,
        "cowork" | "claude-cowork" => Agent::ClaudeCowork,
        "cline" => Agent::Cline,
        "codebuddy" => Agent::CodeBuddy,
        "codex" | "codex-cli" => Agent::Codex,
        "crush" => Agent::Crush,
        "cursor" | "cursor-cli" => Agent::Cursor,
        "firebender" => Agent::Firebender,
        "gemini" | "gemini-cli" => Agent::GeminiCli,
        "github-copilot" | "github-copilot-cli" => Agent::GitHubCopilot,
        "goose" => Agent::Goose,
        "grok" | "grok-cli" => Agent::Grok,
        "iflow" | "iflow-cli" => Agent::IflowCli,
        "kiro" | "kiro-cli" => Agent::Kiro,
        "opencode" => Agent::OpenCode,
        "openhands" => Agent::OpenHands,
        "pi" => Agent::Pi,
        "qwen" | "qwen-code" | "qwencode" => Agent::QwenCode,
        "replit" => Agent::Replit,
        "roo" | "roo-code" | "roocode" => Agent::RooCode,
        "trae" => Agent::Trae,
        "vecli" => Agent::VeCli,
        "warp" | "oz" => Agent::Warp,
        _ => Agent::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn detect(values: &[(&str, &str)]) -> Option<Detection> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), OsString::from(value)))
            .collect::<HashMap<_, _>>();
        detect_with_env(|name| values.get(name).cloned())
    }

    #[test]
    fn detects_representative_agents() {
        for (signal, value, expected) in [
            ("CLAUDECODE", "1", Agent::ClaudeCode),
            ("CODEX_THREAD_ID", "thread-1", Agent::Codex),
            ("GEMINI_CLI", "1", Agent::GeminiCli),
            ("OPENCODE", "1", Agent::OpenCode),
            (
                "COPILOT_AGENT_SESSION_ID",
                "session-1",
                Agent::GitHubCopilot,
            ),
        ] {
            assert_eq!(detect(&[(signal, value)]).unwrap().agent, expected);
        }
    }

    #[test]
    fn generic_name_is_authoritative() {
        let detected = detect(&[("AI_AGENT", "codex@1.2.3"), ("CLAUDECODE", "1")]).unwrap();
        assert_eq!(detected.agent, Agent::Codex);
        assert_eq!(detected.signal, "AI_AGENT");

        let detected = detect(&[("AI_AGENT", "github_copilot_vscode_agent")]).unwrap();
        assert_eq!(detected.agent, Agent::GitHubCopilot);

        let detected = detect(&[
            ("AI_AGENT", "claude-code_2.1.0_cli"),
            ("CLAUDE_CODE_IS_COWORK", "1"),
        ])
        .unwrap();
        assert_eq!(detected.agent, Agent::ClaudeCowork);
    }

    #[test]
    fn generic_flag_yields_to_a_specific_signal() {
        let detected = detect(&[("AI_AGENT", "1"), ("OPENCODE", "1")]).unwrap();
        assert_eq!(detected.agent, Agent::OpenCode);
        assert_eq!(detected.signal, "OPENCODE");
    }

    #[test]
    fn ignores_ambiguous_agent_variable() {
        assert_eq!(detect(&[("AGENT", "1")]), None);
        assert_eq!(detect(&[("AGENT", "build-runner")]), None);
    }

    #[test]
    fn false_generic_values_are_ignored() {
        for value in ["", "0", "false", "no", "off"] {
            assert_eq!(detect(&[("AI_AGENT", value)]), None);
        }
    }

    #[test]
    fn false_provider_values_are_ignored() {
        for value in ["0", "false", "no", "off"] {
            assert_eq!(detect(&[("CLAUDECODE", value)]), None);
            assert_eq!(detect(&[("OPENCODE", value)]), None);
        }
    }

    #[test]
    fn avoids_interactive_environment_false_positives() {
        assert_eq!(detect(&[("CURSOR_TRACE_ID", "trace-1")]), None);
        assert_eq!(detect(&[("REPL_ID", "workspace-1")]), None);
        assert_eq!(detect(&[("TERM_PROGRAM", "WarpTerminal")]), None);
    }

    #[test]
    fn detects_combined_agent_signals() {
        assert_eq!(
            detect(&[
                ("CURSOR_TRACE_ID", "trace-1"),
                ("PAGER", "head -n 10000 | cat")
            ])
            .unwrap()
            .agent,
            Agent::Cursor
        );
        assert_eq!(
            detect(&[("REPL_ID", "workspace-1"), ("REPLIT_MODE", "assistant")])
                .unwrap()
                .agent,
            Agent::Replit
        );
        assert_eq!(
            detect(&[
                ("AGENT_CONTEXT_OUT", "fifo-1"),
                ("AGENT_DISPLAY_OUT", "fifo-2")
            ])
            .unwrap()
            .agent,
            Agent::Kiro
        );
    }

    #[test]
    fn signal_does_not_retain_its_value() {
        let detection = detect(&[("CODEX_THREAD_ID", "sensitive-session-id")]).unwrap();
        assert_eq!(
            format!("{detection:?}"),
            "Detection { agent: Codex, signal: \"CODEX_THREAD_ID\" }"
        );
    }
}
