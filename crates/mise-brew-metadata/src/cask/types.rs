use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppArtifact {
    pub source: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryArtifact {
    pub source: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandWrapperArtifact {
    pub name: String,
    pub target: Option<String>,
    pub content: Option<String>,
    pub executable: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgArtifact {
    pub source: String,
    /// Choice changes for `installer -applyChoiceChangesXML`, e.g. deselecting
    /// a bundled updater. Empty installs the package's default choices.
    pub choices: Vec<PkgChoice>,
}

/// One entry of `installer`'s choice changes: an attribute change for the
/// choice `identifier`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgChoice {
    pub identifier: String,
    pub change: PkgChoiceChange,
}

/// The attributes `installer(8)` documents for `-applyChoiceChangesXML`, each
/// paired with the setting it takes: 0/1 for the flags, a path for
/// `customLocation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PkgChoiceChange {
    Selected(bool),
    Enabled(bool),
    Visible(bool),
    CustomLocation(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerArtifact {
    pub executable: String,
    pub args: Vec<String>,
    pub sudo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericArtifact {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontArtifact {
    pub source: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionShell {
    Bash,
    Fish,
    Zsh,
    Pwsh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionArtifact {
    pub shell: CompletionShell,
    pub source: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedCompletionArtifact {
    pub executable: String,
    pub args: Vec<String>,
    pub base_name: Option<String>,
    pub shell_parameter_format: Option<String>,
    pub shells: Vec<CompletionShell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlightStep {
    Move {
        source: FlightPath,
        target: FlightPath,
        source_glob: bool,
    },
    Remove {
        paths: Vec<FlightPath>,
        recursive: bool,
    },
    SetPermissions {
        paths: Vec<FlightPath>,
        permissions: String,
        recursive: bool,
    },
    SetOwnership {
        paths: Vec<FlightPath>,
        /// `None` is the user running mise (the invoking user under sudo), as
        /// in Homebrew.
        user: Option<String>,
        group: String,
        recursive: bool,
    },
    Copy {
        source: FlightPath,
        target: FlightPath,
        recursive: bool,
        overwrite: bool,
        source_glob: bool,
        guards: Vec<FlightGuard>,
    },
    Symlink {
        source: FlightPath,
        target: FlightPath,
        force: bool,
        uninstall: bool,
        source_glob: bool,
        sudo: FlightSudo,
        guards: Vec<FlightGuard>,
    },
    Run {
        must_succeed: bool,
        command: FlightPath,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        sudo: bool,
        guards: Vec<FlightGuard>,
    },
    TerminateProcess {
        name: String,
        match_mode: ProcessMatch,
        sudo: bool,
        attempts: usize,
        must_succeed: bool,
        notices: Vec<String>,
        failure_message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMatch {
    Name,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightPathBase {
    StagedPath,
    AppDir,
    HomebrewPrefix,
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightSudo {
    Never,
    Always,
    IfNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightPath {
    pub base: FlightPathBase,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlightGuard {
    OnMacos,
    OnLinux,
    IfExists(FlightPath),
    UnlessExists(FlightPath),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaskArtifacts {
    pub apps: Vec<AppArtifact>,
    pub binaries: Vec<BinaryArtifact>,
    pub command_wrappers: Vec<CommandWrapperArtifact>,
    pub pkgs: Vec<PkgArtifact>,
    pub installers: Vec<InstallerArtifact>,
    pub generic: Vec<GenericArtifact>,
    pub fonts: Vec<FontArtifact>,
    pub completions: Vec<CompletionArtifact>,
    pub generated_completions: Vec<GeneratedCompletionArtifact>,
    pub preflight_steps: Vec<FlightStep>,
    pub postflight_steps: Vec<FlightStep>,
    pub pkg_ids: Vec<String>,
}

impl CompletionShell {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            "zsh" => Some(Self::Zsh),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Fish => "fish",
            Self::Zsh => "zsh",
            Self::Pwsh => "pwsh",
        }
    }

    pub fn parameter_name(self) -> &'static str {
        match self {
            Self::Pwsh => "powershell",
            _ => self.name(),
        }
    }
}

impl FlightStep {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::SetPermissions { .. } => "set_permissions",
            Self::SetOwnership { .. } => "set_ownership",
            Self::Copy { .. } => "copy",
            Self::Symlink { .. } => "symlink",
            Self::Run { .. } => "run",
            Self::TerminateProcess { .. } => "terminate_process",
        }
    }
}
