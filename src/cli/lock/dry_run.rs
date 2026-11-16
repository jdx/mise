//! What `mise lock --dry-run` reports: the difference between a lockfile as
//! read and the same lockfile after this run resolved into it.

use console::style;
use mise_util::redactions::redact_output;

use crate::args::BackendArg;
use crate::lockfile::{Lockfile, LockfileTool, PlatformInfo};
use crate::platform::Platform;
use crate::toolset::ToolVersion;

/// What resolving would do to one tool version's entry on one platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EntryChange {
    /// The lockfile has no entry for the tool.
    NewEntry,
    /// The tool is locked at other versions, which this one replaces.
    VersionChange { from: Vec<String> },
    /// The version is locked, but not for this platform.
    NewPlatform,
    /// The platform entry gains, changes or loses these fields.
    Updated(Vec<String>),
    /// The platform entry would be dropped.
    Removed,
    /// Resolution leaves the entry as it is.
    Unchanged,
    /// Nothing was resolved for this platform, and nothing is locked for it.
    Unresolved,
}

impl EntryChange {
    fn changes(&self) -> bool {
        !matches!(self, Self::Unchanged | Self::Unresolved)
    }
}

impl std::fmt::Display for EntryChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewEntry => write!(f, "new entry"),
            Self::VersionChange { from } => write!(f, "version change from {}", from.join(", ")),
            Self::NewPlatform => write!(f, "new platform"),
            Self::Updated(fields) => write!(f, "{}", fields.join(", ")),
            Self::Removed => write!(f, "entry removed"),
            Self::Unchanged => write!(f, "unchanged"),
            Self::Unresolved => write!(f, "nothing resolved"),
        }
    }
}

/// One line of the dry-run report.
#[derive(Debug)]
pub(super) struct EntryLine {
    short: String,
    version: String,
    platform: String,
    change: EntryChange,
}

/// The change to each platform entry `tasks` resolves, comparing `before`
/// (the lockfile as read) with `after` (the lockfile once resolved).
pub(super) fn entry_changes(
    before: &Lockfile,
    after: &Lockfile,
    tasks: &[(BackendArg, ToolVersion, Platform)],
) -> Vec<EntryLine> {
    let mut lines = Vec::new();
    for (ba, tv, platform) in tasks {
        // The versions this run locks for the tool, which do not replace each other.
        let locked_here: Vec<&str> = tasks
            .iter()
            .filter(|(other, _, _)| other.short == ba.short)
            .map(|(_, tv, _)| tv.version.as_str())
            .collect();
        let platform = platform.to_key();
        for change in changes_for(
            before,
            after,
            &ba.short,
            &tv.version,
            &locked_here,
            &platform,
        ) {
            lines.push(EntryLine {
                short: ba.short.clone(),
                version: tv.version.clone(),
                platform: platform.clone(),
                change,
            });
        }
    }
    lines
}

/// The change to `short@version` on `platform`, one per locked option
/// variant of that version.
fn changes_for(
    before: &Lockfile,
    after: &Lockfile,
    short: &str,
    version: &str,
    locked_here: &[&str],
    platform: &str,
) -> Vec<EntryChange> {
    let before_entries: &[LockfileTool] = before.tools_for(short).map_or(&[], Vec::as_slice);
    let after_entries: Vec<&LockfileTool> = after
        .tools_for(short)
        .into_iter()
        .flatten()
        .filter(|entry| entry.version == version)
        .collect();
    if after_entries.is_empty() {
        return vec![EntryChange::Unresolved];
    }
    after_entries
        .into_iter()
        .map(|after_entry| {
            let before_entry = before_entries
                .iter()
                .find(|entry| entry.version == version && entry.options == after_entry.options);
            let Some(before_entry) = before_entry else {
                let mut from: Vec<String> = Vec::new();
                for entry in before_entries {
                    if !locked_here.contains(&entry.version.as_str())
                        && !from.contains(&entry.version)
                    {
                        from.push(entry.version.clone());
                    }
                }
                return if from.is_empty() {
                    EntryChange::NewEntry
                } else {
                    EntryChange::VersionChange { from }
                };
            };
            match (
                before_entry.platforms.get(platform),
                after_entry.platforms.get(platform),
            ) {
                (None, None) => EntryChange::Unresolved,
                (Some(_), None) => EntryChange::Removed,
                (None, Some(_)) => EntryChange::NewPlatform,
                (Some(old), Some(new)) if old.same_persisted_data(new) => EntryChange::Unchanged,
                (Some(old), Some(new)) => EntryChange::Updated(changed_fields(old, new)),
            }
        })
        .collect()
}

/// Describe how `new` differs from `old`, field by field.
fn changed_fields(old: &PlatformInfo, new: &PlatformInfo) -> Vec<String> {
    fn field(fields: &mut Vec<String>, name: &str, old: Option<String>, new: Option<String>) {
        match (old, new) {
            (None, Some(_)) => fields.push(format!("{name} filled")),
            (Some(_), None) => fields.push(format!("{name} removed")),
            (Some(old), Some(new)) if old != new => fields.push(format!("{name} changed")),
            _ => {}
        }
    }
    let mut fields = Vec::new();
    field(
        &mut fields,
        "checksum",
        old.checksum.clone(),
        new.checksum.clone(),
    );
    field(&mut fields, "url", old.url.clone(), new.url.clone());
    field(
        &mut fields,
        "provenance",
        old.provenance.as_ref().map(ToString::to_string),
        new.provenance.as_ref().map(ToString::to_string),
    );
    let artifacts = |info: &PlatformInfo| {
        info.additional_artifacts
            .iter()
            .map(|artifact| (artifact.url.clone(), artifact.checksum.clone()))
            .collect::<Vec<_>>()
    };
    if artifacts(old) != artifacts(new) {
        fields.push("additional artifacts changed".to_string());
    }
    if fields.is_empty() {
        fields.push("metadata changed".to_string());
    }
    fields
}

pub(super) fn show(lines: &[EntryLine]) -> eyre::Result<()> {
    let changed = lines.iter().filter(|line| line.change.changes()).count();
    if changed == 0 {
        miseprintln!("{} Dry run - already up to date:", style("→").yellow());
    } else {
        miseprintln!("{} Dry run - would update:", style("→").yellow());
    }
    for line in lines {
        let icon = match &line.change {
            EntryChange::NewEntry
            | EntryChange::VersionChange { .. }
            | EntryChange::NewPlatform => style("+").green(),
            EntryChange::Updated(_) => style("~").yellow(),
            EntryChange::Removed => style("-").red(),
            EntryChange::Unchanged | EntryChange::Unresolved => style("=").dim(),
        };
        miseprintln!(
            "  {icon} {}@{} for {} ({})",
            style(redact_output(&line.short)).bold(),
            line.version,
            style(&line.platform).blue(),
            line.change
        );
    }
    miseprintln!(
        "{} {changed} of {} platform entries would change",
        style("→").yellow(),
        lines.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn info(url: Option<&str>, checksum: Option<&str>) -> PlatformInfo {
        PlatformInfo {
            url: url.map(str::to_string),
            checksum: checksum.map(str::to_string),
            ..Default::default()
        }
    }

    fn lockfile(entries: &[(&str, &str, Option<PlatformInfo>)]) -> Lockfile {
        let mut lockfile = Lockfile::default();
        for (short, version, platform) in entries {
            lockfile.set_platform_info(
                short,
                version,
                Some("aqua:example/tool"),
                &BTreeMap::new(),
                "linux-x64",
                platform.clone().unwrap_or_default(),
            );
        }
        lockfile
    }

    fn change(before: &Lockfile, after: &Lockfile, version: &str) -> Vec<EntryChange> {
        changes_for(before, after, "tool", version, &[version], "linux-x64")
    }

    #[test]
    fn a_tool_missing_from_the_lockfile_is_a_new_entry() {
        let before = Lockfile::default();
        let after = lockfile(&[("tool", "1.0.0", Some(info(Some("u"), Some("c"))))]);
        assert_eq!(change(&before, &after, "1.0.0"), [EntryChange::NewEntry]);
    }

    #[test]
    fn a_replaced_version_is_a_version_change() {
        let before = lockfile(&[("tool", "1.0.0", Some(info(Some("u"), Some("c"))))]);
        let after = lockfile(&[
            ("tool", "1.0.0", Some(info(Some("u"), Some("c")))),
            ("tool", "1.1.0", Some(info(Some("v"), Some("d")))),
        ]);
        assert_eq!(
            change(&before, &after, "1.1.0"),
            [EntryChange::VersionChange {
                from: vec!["1.0.0".into()]
            }]
        );
    }

    #[test]
    fn a_filled_checksum_is_reported() {
        let before = lockfile(&[("tool", "1.0.0", Some(info(Some("u"), None)))]);
        let after = lockfile(&[("tool", "1.0.0", Some(info(Some("u"), Some("c"))))]);
        assert_eq!(
            change(&before, &after, "1.0.0"),
            [EntryChange::Updated(vec!["checksum filled".into()])]
        );
    }

    #[test]
    fn a_version_only_entry_gains_a_platform() {
        let before = lockfile(&[("tool", "1.0.0", None)]);
        let after = lockfile(&[("tool", "1.0.0", Some(info(Some("u"), Some("c"))))]);
        assert_eq!(change(&before, &after, "1.0.0"), [EntryChange::NewPlatform]);
    }

    #[test]
    fn a_complete_entry_is_unchanged() {
        let before = lockfile(&[("tool", "1.0.0", Some(info(Some("u"), Some("c"))))]);
        let after = before.clone();
        assert_eq!(change(&before, &after, "1.0.0"), [EntryChange::Unchanged]);
    }
}
