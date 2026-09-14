//! Domain model for the mise machine-bootstrap workflow.
//!
//! This crate owns the stable phase vocabulary and phase-selection semantics.
//! The `mise` binary supplies the configuration, resource implementations, and
//! CLI adapters that execute the selected phases.

use std::collections::HashSet;

/// One independently selectable part of a machine-bootstrap run.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, usage_rs::ValueEnum)]
pub enum Phase {
    Plugins,
    Packages,
    Accounts,
    Files,
    Services,
    Firewall,
    Compose,
    Repos,
    Dotfiles,
    #[usage(name = "mise-shell-activate", visible_alias = "shell")]
    Shell,
    #[usage(name = "macos-defaults", visible_alias = "defaults")]
    Defaults,
    #[usage(name = "macos-launchd-agents", visible_alias = "launchd")]
    Launchd,
    #[usage(name = "linux-systemd-units", visible_alias = "systemd")]
    Systemd,
    User,
    Tools,
    Task,
    FinalHook,
}

impl Phase {
    /// Every selectable phase, used to compute the complement of `--only`.
    pub const ALL: [Self; 17] = [
        Self::Plugins,
        Self::Packages,
        Self::Accounts,
        Self::Files,
        Self::Services,
        Self::Firewall,
        Self::Compose,
        Self::Repos,
        Self::Dotfiles,
        Self::Shell,
        Self::Defaults,
        Self::Launchd,
        Self::Systemd,
        Self::User,
        Self::Tools,
        Self::Task,
        Self::FinalHook,
    ];

    /// Canonical spelling used when forwarding phase filters.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plugins => "plugins",
            Self::Packages => "packages",
            Self::Accounts => "accounts",
            Self::Files => "files",
            Self::Services => "services",
            Self::Firewall => "firewall",
            Self::Compose => "compose",
            Self::Repos => "repos",
            Self::Dotfiles => "dotfiles",
            Self::Shell => "mise-shell-activate",
            Self::Defaults => "macos-defaults",
            Self::Launchd => "macos-launchd-agents",
            Self::Systemd => "linux-systemd-units",
            Self::User => "user",
            Self::Tools => "tools",
            Self::Task => "task",
            Self::FinalHook => "final-hook",
        }
    }

    /// Phase responsible for a declarative resource kind, when one exists.
    pub fn for_resource_kind(kind: &str) -> Option<Self> {
        match kind {
            "package" => Some(Self::Packages),
            "file" | "directory" => Some(Self::Files),
            "service" => Some(Self::Services),
            "firewall" | "firewall-rule" => Some(Self::Firewall),
            "user" | "group" => Some(Self::Accounts),
            _ => None,
        }
    }
}

/// The phases excluded from one bootstrap invocation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    skipped: HashSet<Phase>,
}

impl Selection {
    /// Build a selection from `--only` or `--skip` values.
    ///
    /// The CLI prevents the two filters from being combined. If another caller
    /// supplies both, `only` takes precedence to preserve mise's historical
    /// behavior.
    pub fn from_filters(only: &[Phase], skip: &[Phase]) -> Self {
        let skipped = if only.is_empty() {
            skip.iter().copied().collect()
        } else {
            let only = only.iter().copied().collect::<HashSet<_>>();
            Phase::ALL
                .into_iter()
                .filter(|phase| !only.contains(phase))
                .collect()
        };
        Self { skipped }
    }

    /// Whether the given phase is excluded from the invocation.
    pub fn skips(&self, phase: Phase) -> bool {
        self.skipped.contains(&phase)
    }
}

#[cfg(test)]
mod tests {
    use super::{Phase, Selection};

    #[test]
    fn skip_filter_excludes_only_requested_phases() {
        let selection = Selection::from_filters(&[], &[Phase::Tools, Phase::Task]);

        assert!(selection.skips(Phase::Tools));
        assert!(selection.skips(Phase::Task));
        assert!(!selection.skips(Phase::Packages));
    }

    #[test]
    fn only_filter_excludes_its_complement() {
        let selection = Selection::from_filters(&[Phase::Tools], &[]);

        assert!(!selection.skips(Phase::Tools));
        assert!(selection.skips(Phase::Packages));
        assert!(selection.skips(Phase::FinalHook));
    }

    #[test]
    fn resource_kinds_map_to_their_owning_phase() {
        assert_eq!(Phase::for_resource_kind("directory"), Some(Phase::Files));
        assert_eq!(
            Phase::for_resource_kind("firewall-rule"),
            Some(Phase::Firewall)
        );
        assert_eq!(Phase::for_resource_kind("compose"), None);
    }

    #[test]
    fn canonical_names_cover_every_phase() {
        let names = Phase::ALL
            .map(Phase::name)
            .into_iter()
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(names.len(), Phase::ALL.len());
    }
}
