//! What `mise lock --dry-run` reports: the difference between a lockfile as
//! read and the same lockfile after this run resolved into it.

use std::collections::BTreeMap;

use console::style;
use mise_util::redactions::redact_output;

use super::{LockTool, platform_tasks};
use crate::lockfile::{Lockfile, LockfileTool, PlatformInfo, ProvenanceType};
use crate::platform::Platform;

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

/// The change to each platform entry locking `tools` on `platforms` makes,
/// comparing `before` (the lockfile as read) with `after` (the lockfile
/// once resolved). Each stored entry is reported once, including entries
/// the run drops; versions in `stale_versions` are left to the prune
/// message.
pub(super) fn entry_changes(
    before: &Lockfile,
    after: &Lockfile,
    tools: &[LockTool],
    platforms: &[Platform],
    stale_versions: &BTreeMap<String, Vec<String>>,
) -> Vec<EntryLine> {
    let mut lines = Vec::new();
    let mut shorts: Vec<&str> = Vec::new();
    for (ba, _) in tools {
        if !shorts.contains(&ba.short.as_str()) {
            shorts.push(&ba.short);
        }
    }
    let tasks = platform_tasks(tools, platforms);
    for short in shorts {
        let tools: Vec<&LockTool> = tools.iter().filter(|(ba, _)| ba.short == short).collect();
        // The versions this run locks for the tool, which do not replace each other.
        let locked_here: Vec<&str> = tools.iter().map(|(_, tv)| tv.version.as_str()).collect();
        let keys = platform_keys(&tools, platforms);
        let stale: &[String] = stale_versions.get(short).map_or(&[], Vec::as_slice);
        let mut changes = tool_changes(before, after, short, &locked_here, &keys, stale);
        // A platform this run tried to lock that neither lockfile has.
        for (_, tv, platform) in tasks.iter().filter(|(ba, _, _)| ba.short == short) {
            let key = platform.to_key();
            if !changes
                .iter()
                .any(|(version, k, _)| *version == tv.version && *k == key)
            {
                changes.push((tv.version.clone(), key, EntryChange::Unresolved));
            }
        }
        lines.extend(
            changes
                .into_iter()
                .map(|(version, platform, change)| EntryLine {
                    short: short.to_string(),
                    version,
                    platform,
                    change,
                }),
        );
    }
    lines
}

/// The platform keys an entry of `tools` can be stored under on
/// `platforms`, whether or not a tool's `os` option still allows them, so
/// that entries the run drops are seen too.
fn platform_keys(tools: &[&LockTool], platforms: &[Platform]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for (ba, tv) in tools {
        let backend = crate::backend::get(ba);
        for platform in platforms {
            let variants = backend
                .as_ref()
                .map(|backend| backend.platform_variants_for(tv, platform))
                .unwrap_or_default();
            for key in
                std::iter::once(platform.to_key()).chain(variants.iter().map(Platform::to_key))
            {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
    }
    keys
}

/// The change to each of `short`'s entries on each of `keys`, as
/// `(version, platform key, change)`: one per stored option variant, and
/// only where either lockfile has the platform.
fn tool_changes(
    before: &Lockfile,
    after: &Lockfile,
    short: &str,
    locked_here: &[&str],
    keys: &[String],
    stale: &[String],
) -> Vec<(String, String, EntryChange)> {
    let before_entries: &[LockfileTool] = before.tools_for(short).map_or(&[], Vec::as_slice);
    let after_entries: &[LockfileTool] = after.tools_for(short).map_or(&[], Vec::as_slice);
    let same =
        |a: &LockfileTool, b: &LockfileTool| a.version == b.version && a.options == b.options;
    let mut changes = Vec::new();
    for (i, after_entry) in after_entries.iter().enumerate() {
        if after_entries[..i]
            .iter()
            .any(|seen| same(seen, after_entry))
        {
            continue;
        }
        let before_entry = before_entries.iter().find(|entry| same(entry, after_entry));
        for key in keys {
            let old = before_entry.and_then(|entry| entry.platforms.get(key));
            let new = after_entry.platforms.get(key);
            let change = match (before_entry, old, new) {
                (_, None, None) => continue,
                (None, _, Some(_)) => {
                    let mut from: Vec<String> = Vec::new();
                    for entry in before_entries {
                        if !locked_here.contains(&entry.version.as_str())
                            && !from.contains(&entry.version)
                        {
                            from.push(entry.version.clone());
                        }
                    }
                    if from.is_empty() {
                        EntryChange::NewEntry
                    } else {
                        EntryChange::VersionChange { from }
                    }
                }
                (_, Some(_), None) => EntryChange::Removed,
                (_, None, Some(_)) => EntryChange::NewPlatform,
                (_, Some(old), Some(new)) if same_entry(old, new) => EntryChange::Unchanged,
                (_, Some(old), Some(new)) => EntryChange::Updated(changed_fields(old, new)),
            };
            changes.push((after_entry.version.clone(), key.clone(), change));
        }
    }
    // Entries the run drops, other than stale versions the prune message names.
    for (i, before_entry) in before_entries.iter().enumerate() {
        if stale.contains(&before_entry.version)
            || before_entries[..i]
                .iter()
                .any(|seen| same(seen, before_entry))
            || after_entries.iter().any(|entry| same(entry, before_entry))
        {
            continue;
        }
        for key in keys
            .iter()
            .filter(|key| before_entry.platforms.contains_key(*key))
        {
            changes.push((
                before_entry.version.clone(),
                key.clone(),
                EntryChange::Removed,
            ));
        }
    }
    changes
}

/// Whether `old` and `new` would be written the same. Unlike
/// `ProvenanceType`'s equality, this compares the SLSA provenance URL too.
fn same_entry(old: &PlatformInfo, new: &PlatformInfo) -> bool {
    old.same_persisted_data(new) && slsa_url(old) == slsa_url(new)
}

fn slsa_url(info: &PlatformInfo) -> Option<&str> {
    match &info.provenance {
        Some(ProvenanceType::Slsa { url }) => url.as_deref(),
        _ => None,
    }
}

/// How `info`'s provenance is written: its type, and the URL for SLSA.
fn provenance(info: &PlatformInfo) -> Option<String> {
    info.provenance
        .as_ref()
        .map(|provenance| match slsa_url(info) {
            Some(url) => format!("{provenance} {url}"),
            None => provenance.to_string(),
        })
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
    field(&mut fields, "provenance", provenance(old), provenance(new));
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
    use super::*;

    fn info(url: Option<&str>, checksum: Option<&str>) -> PlatformInfo {
        PlatformInfo {
            url: url.map(str::to_string),
            checksum: checksum.map(str::to_string),
            ..Default::default()
        }
    }

    /// `(version, options, platform, entry)`; no entry stores the version alone.
    type Entry<'a> = (
        &'a str,
        &'a [(&'a str, &'a str)],
        &'a str,
        Option<PlatformInfo>,
    );

    fn lockfile(entries: &[Entry<'_>]) -> Lockfile {
        let mut lockfile = Lockfile::default();
        for (version, options, platform, info) in entries {
            let options: BTreeMap<String, String> = options
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            lockfile.set_platform_info(
                "tool",
                version,
                Some("aqua:example/tool"),
                &options,
                platform,
                info.clone().unwrap_or_default(),
            );
        }
        lockfile
    }

    fn linux(version: &str, info: PlatformInfo) -> Lockfile {
        lockfile(&[(version, &[], "linux-x64", Some(info))])
    }

    fn changes(
        before: &Lockfile,
        after: &Lockfile,
        locked_here: &[&str],
        keys: &[&str],
        stale: &[&str],
    ) -> Vec<(String, String, EntryChange)> {
        let keys: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
        let stale: Vec<String> = stale.iter().map(|v| v.to_string()).collect();
        tool_changes(before, after, "tool", locked_here, &keys, &stale)
    }

    fn change(before: &Lockfile, after: &Lockfile, version: &str) -> Vec<EntryChange> {
        changes(before, after, &[version], &["linux-x64"], &[])
            .into_iter()
            .map(|(_, _, change)| change)
            .collect()
    }

    fn line(version: &str, platform: &str, change: EntryChange) -> (String, String, EntryChange) {
        (version.to_string(), platform.to_string(), change)
    }

    #[test]
    fn a_tool_missing_from_the_lockfile_is_a_new_entry() {
        let before = Lockfile::default();
        let after = linux("1.0.0", info(Some("u"), Some("c")));
        assert_eq!(change(&before, &after, "1.0.0"), [EntryChange::NewEntry]);
    }

    #[test]
    fn a_replaced_version_is_a_version_change() {
        let before = linux("1.0.0", info(Some("u"), Some("c")));
        let after = linux("1.1.0", info(Some("v"), Some("d")));
        assert_eq!(
            changes(&before, &after, &["1.1.0"], &["linux-x64"], &["1.0.0"]),
            [line(
                "1.1.0",
                "linux-x64",
                EntryChange::VersionChange {
                    from: vec!["1.0.0".into()]
                }
            )]
        );
    }

    #[test]
    fn a_filled_checksum_is_reported() {
        let before = linux("1.0.0", info(Some("u"), None));
        let after = linux("1.0.0", info(Some("u"), Some("c")));
        assert_eq!(
            change(&before, &after, "1.0.0"),
            [EntryChange::Updated(vec!["checksum filled".into()])]
        );
    }

    #[test]
    fn a_version_only_entry_gains_a_platform() {
        let before = linux("1.0.0", PlatformInfo::default());
        let after = linux("1.0.0", info(Some("u"), Some("c")));
        assert_eq!(change(&before, &after, "1.0.0"), [EntryChange::NewPlatform]);
    }

    #[test]
    fn a_complete_entry_is_unchanged() {
        let before = linux("1.0.0", info(Some("u"), Some("c")));
        let after = before.clone();
        assert_eq!(change(&before, &after, "1.0.0"), [EntryChange::Unchanged]);
    }

    #[test]
    fn each_option_variant_is_reported_once() {
        let before = Lockfile::default();
        let after = lockfile(&[
            (
                "1.0.0",
                &[("flavor", "a")],
                "linux-x64",
                Some(info(Some("a"), Some("c"))),
            ),
            (
                "1.0.0",
                &[("flavor", "b")],
                "linux-x64",
                Some(info(Some("b"), Some("d"))),
            ),
        ]);
        assert_eq!(
            changes(&before, &after, &["1.0.0", "1.0.0"], &["linux-x64"], &[]),
            [
                line("1.0.0", "linux-x64", EntryChange::NewEntry),
                line("1.0.0", "linux-x64", EntryChange::NewEntry),
            ]
        );
    }

    #[test]
    fn a_new_entry_counts_only_the_platforms_it_resolved() {
        let before = Lockfile::default();
        let after = linux("1.0.0", info(Some("u"), Some("c")));
        assert_eq!(
            changes(
                &before,
                &after,
                &["1.0.0"],
                &["linux-x64", "macos-arm64"],
                &[]
            ),
            [line("1.0.0", "linux-x64", EntryChange::NewEntry)]
        );
    }

    #[test]
    fn a_dropped_platform_is_removed() {
        let before = lockfile(&[
            ("1.0.0", &[], "linux-x64", Some(info(Some("u"), Some("c")))),
            (
                "1.0.0",
                &[],
                "macos-arm64",
                Some(info(Some("v"), Some("d"))),
            ),
        ]);
        let after = lockfile(&[(
            "1.0.0",
            &[],
            "macos-arm64",
            Some(info(Some("v"), Some("d"))),
        )]);
        assert_eq!(
            changes(
                &before,
                &after,
                &["1.0.0"],
                &["linux-x64", "macos-arm64"],
                &[]
            ),
            [
                line("1.0.0", "linux-x64", EntryChange::Removed),
                line("1.0.0", "macos-arm64", EntryChange::Unchanged),
            ]
        );
    }

    #[test]
    fn a_dropped_entry_is_removed_unless_the_prune_message_names_it() {
        let before = linux("1.0.0", info(Some("u"), Some("c")));
        let after = Lockfile::default();
        assert_eq!(
            changes(&before, &after, &["1.0.0"], &["linux-x64"], &[]),
            [line("1.0.0", "linux-x64", EntryChange::Removed)]
        );
        assert_eq!(
            changes(&before, &after, &["1.1.0"], &["linux-x64"], &["1.0.0"]),
            []
        );
    }

    #[test]
    fn a_changed_slsa_url_is_reported() {
        let slsa = |url: &str| PlatformInfo {
            provenance: Some(ProvenanceType::Slsa {
                url: Some(url.to_string()),
            }),
            ..info(Some("u"), Some("c"))
        };
        let before = linux("1.0.0", slsa("https://example.com/a.intoto.jsonl"));
        let after = linux("1.0.0", slsa("https://example.com/b.intoto.jsonl"));
        assert_eq!(
            change(&before, &after, "1.0.0"),
            [EntryChange::Updated(vec!["provenance changed".into()])]
        );
    }
}
