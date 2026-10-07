//! Variant selection for tracked files: which shared stream a machine
//! belongs to, using the same conventions as bootstrap packages (`os`, with
//! an optional `/arch`) and mise environments (`profile`).

use serde::{Deserialize, Serialize};

/// One `variants = [{ … }]` element of a `[dotfiles]` track entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// `"macos"`, `"linux/arm64"`, the `"unix"` family, or a list; empty = any.
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub os: Vec<String>,
    /// Active when this mise environment is selected (`-E work`).
    #[serde(default)]
    pub profile: Option<String>,
    /// The explicit fallback stream for machines matching no other variant.
    #[serde(default)]
    pub default: bool,
    /// A separate stream on every machine, named after this machine (see
    /// [`super::store::machine_name`]). Written only when set, so a setup
    /// without it stays readable by older clients, which refuse it rather
    /// than share the file across machines.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub machine: bool,
}

/// The prefix of every per-machine stream name.
pub(crate) const MACHINE_STREAM_PREFIX: &str = "machine-";

impl Variant {
    /// The stream name recorded in checkpoints and used in the setup branch:
    /// [`Self::selector_name`], or `machine-<name>` for this machine's own
    /// stream. Naming this machine fails when its name cannot be read or
    /// kept, and the history operation stops: another name could be a
    /// stream some other machine also writes.
    pub(crate) fn name(&self) -> eyre::Result<String> {
        if self.machine {
            return Ok(format!(
                "{MACHINE_STREAM_PREFIX}{}",
                super::store::machine_name()?
            ));
        }
        Ok(self.selector_name())
    }

    /// The stream name of an `os`/`profile`/`default` variant: `macos`,
    /// `linux-arm64`, `macos+work`, `work`, or `default`. A machine variant
    /// names a different stream on every machine; see [`Self::name`].
    pub(crate) fn selector_name(&self) -> String {
        let mut parts = vec![];
        for os in &self.os {
            parts.push(os.replace('/', "-"));
        }
        let mut name = parts.join("-");
        if let Some(profile) = &self.profile {
            if name.is_empty() {
                name = profile.clone();
            } else {
                name = format!("{name}+{profile}");
            }
        }
        if name.is_empty() {
            "default".to_string()
        } else {
            name
        }
    }

    /// Whether `stream` belongs to this variant on some machine: its own
    /// name, or for a per-machine variant, the stream of any machine. A
    /// per-machine variant names a different stream on every machine, so
    /// code that reasons about every machine's streams (carrying the ones
    /// inactive here, owning their permissions) must ask this instead of
    /// comparing with [`Self::name`].
    pub(crate) fn declares_stream(&self, stream: &str) -> bool {
        if self.machine {
            stream
                .strip_prefix(MACHINE_STREAM_PREFIX)
                .is_some_and(super::store::is_valid_machine_name)
        } else {
            self.selector_name() == stream
        }
    }

    fn matches(&self, environments: &[String]) -> bool {
        let os_ok = self.os.is_empty()
            || self
                .os
                .iter()
                .any(|entry| crate::platform::os_selector_matches(entry));
        let profile_ok = self
            .profile
            .as_ref()
            .is_none_or(|profile| environments.iter().any(|env| env == profile));
        os_ok && profile_ok
    }

    /// `profile` +4, plus the best matching `os` entry: an os +2 (the `unix`
    /// family +1) and an arch qualifier +2 more.
    fn specificity(&self) -> u8 {
        let profile = if self.profile.is_some() { 4 } else { 0 };
        let os = self
            .os
            .iter()
            .filter(|entry| crate::platform::os_selector_matches(entry))
            .map(|entry| {
                let os = if crate::platform::is_os_family_selector(entry) {
                    1
                } else {
                    2
                };
                let arch = if entry.contains('/') { 2 } else { 0 };
                os + arch
            })
            .max()
            .unwrap_or(0);
        profile + os
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    /// The entry has no variants: one stream for every machine.
    Single,
    /// The variant this machine uses.
    Variant(Variant),
    /// No variant matches and none is the default: do not capture or apply.
    NoMatch,
    /// Two variants match with the same specificity.
    Ambiguous(Vec<Variant>),
}

/// Reject fallbacks whose selectors would be ignored during selection, and
/// a per-machine variant beside anything else: it already selects every
/// machine, so another selector could never win.
pub(crate) fn validate(variants: &[Variant]) -> eyre::Result<()> {
    if variants.iter().any(|variant| variant.machine)
        && !matches!(
            variants,
            [only] if only.os.is_empty() && only.profile.is_none() && !only.default
        )
    {
        eyre::bail!("a machine variant must be the only variant, without os, profile, or default");
    }
    let mut default_seen = false;
    for variant in variants {
        if variant.default {
            if default_seen || !variant.os.is_empty() || variant.profile.is_some() {
                eyre::bail!("variants allow only one default, without os or profile selectors");
            }
            default_seen = true;
        }
    }
    Ok(())
}

/// Picks the variant for this machine given the active mise environments.
pub(crate) fn select(variants: &[Variant], environments: &[String]) -> Selection {
    if variants.is_empty() {
        return Selection::Single;
    }
    let matching: Vec<&Variant> = variants
        .iter()
        .filter(|variant| !variant.default && variant.matches(environments))
        .collect();
    let best = matching.iter().map(|variant| variant.specificity()).max();
    match best {
        Some(best) => {
            let winners: Vec<&Variant> = matching
                .into_iter()
                .filter(|variant| variant.specificity() == best)
                .collect();
            match winners.as_slice() {
                [one] => Selection::Variant((*one).clone()),
                many => Selection::Ambiguous(many.iter().map(|v| (*v).clone()).collect()),
            }
        }
        None => match variants.iter().find(|variant| variant.default) {
            Some(fallback) => Selection::Variant(fallback.clone()),
            None => Selection::NoMatch,
        },
    }
}

/// The active mise environments (`-E` / `MISE_ENV`).
pub(crate) fn active_environments() -> Vec<String> {
    crate::env::MISE_ENV_WITH_AUTO.clone()
}

fn deserialize_one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(one) => vec![one],
        OneOrMany::Many(many) => many,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_must_be_unique_and_unqualified() {
        let fallback = Variant {
            default: true,
            ..Variant::default()
        };
        assert!(validate(std::slice::from_ref(&fallback)).is_ok());
        assert!(validate(&[fallback.clone(), fallback.clone()]).is_err());
        assert!(
            validate(&[Variant {
                os: vec!["macos".into()],
                ..fallback.clone()
            }])
            .is_err()
        );
        assert!(
            validate(&[Variant {
                profile: Some("work".into()),
                ..fallback
            }])
            .is_err()
        );
    }

    fn v(os: &[&str], profile: Option<&str>, default: bool) -> Variant {
        Variant {
            os: os.iter().map(|s| s.to_string()).collect(),
            profile: profile.map(str::to_string),
            default,
            machine: false,
        }
    }

    fn machine() -> Variant {
        Variant {
            machine: true,
            ..Variant::default()
        }
    }

    #[test]
    fn a_machine_variant_stands_alone() {
        assert!(validate(&[machine()]).is_ok());
        assert_eq!(select(&[machine()], &[]), Selection::Variant(machine()));
        for invalid in [
            vec![machine(), v(&["linux"], None, false)],
            vec![machine(), v(&[], None, true)],
            vec![Variant {
                os: vec!["linux".into()],
                ..machine()
            }],
            vec![Variant {
                profile: Some("work".into()),
                ..machine()
            }],
            vec![Variant {
                default: true,
                ..machine()
            }],
        ] {
            assert!(validate(&invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn a_machine_variant_declares_every_machines_stream() {
        let machine = machine();
        assert!(machine.declares_stream("machine-omarchy-3f2a9c1b"));
        assert!(machine.declares_stream("machine-desk"));
        assert!(!machine.declares_stream("machine-"));
        assert!(!machine.declares_stream("linux"));
        assert!(!machine.declares_stream("default"));
        let linux = v(&["linux"], None, false);
        assert!(linux.declares_stream("linux"));
        assert!(!linux.declares_stream("machine-desk"));
    }

    #[test]
    fn a_machine_variant_is_written_only_when_set() {
        let json = serde_json::to_string(&v(&["linux"], None, false)).unwrap();
        assert!(!json.contains("machine"), "{json}");
        let json = serde_json::to_string(&machine()).unwrap();
        assert!(json.contains(r#""machine":true"#), "{json}");
        let read: Variant = serde_json::from_str(&json).unwrap();
        assert_eq!(read, machine());
    }

    #[test]
    fn selection_prefers_the_most_specific_match() {
        let this_os = crate::platform::OS.to_string();
        let other_os = if this_os == "linux" { "macos" } else { "linux" };
        let variants = vec![v(&[&this_os], None, false), v(&[other_os], None, false)];
        assert_eq!(
            select(&variants, &[]),
            Selection::Variant(v(&[&this_os], None, false))
        );
        // a profile beats an os alone
        let variants = vec![v(&[&this_os], None, false), v(&[], Some("work"), false)];
        assert_eq!(
            select(&variants, &["work".into()]),
            Selection::Variant(v(&[], Some("work"), false))
        );
        // no match and no default: nothing shared
        let variants = vec![v(&[other_os], None, false)];
        assert_eq!(select(&variants, &[]), Selection::NoMatch);
        let variants = vec![v(&[other_os], None, false), v(&[], None, true)];
        assert_eq!(
            select(&variants, &[]),
            Selection::Variant(v(&[], None, true))
        );
        // equal specificity is ambiguous
        let variants = vec![v(&[&this_os], None, false), v(&[&this_os], None, false)];
        assert!(matches!(select(&variants, &[]), Selection::Ambiguous(_)));
        assert_eq!(select(&[], &[]), Selection::Single);
    }

    #[cfg(unix)]
    #[test]
    fn unix_family_selects_below_a_specific_os() {
        let this_os = crate::platform::OS.to_string();
        let this_arch = crate::platform::ARCH.to_string();
        let variants = vec![v(&["unix"], None, false), v(&["windows"], None, false)];
        assert_eq!(
            select(&variants, &[]),
            Selection::Variant(v(&["unix"], None, false))
        );
        // the os itself beats its family
        let variants = vec![v(&["unix"], None, false), v(&[&this_os], None, false)];
        assert_eq!(
            select(&variants, &[]),
            Selection::Variant(v(&[&this_os], None, false))
        );
        // an arch qualifier on the family beats the os alone
        let family_arch = format!("unix/{this_arch}");
        let variants = vec![v(&[&family_arch], None, false), v(&[&this_os], None, false)];
        assert_eq!(
            select(&variants, &[]),
            Selection::Variant(v(&[&family_arch], None, false))
        );
        // a list scores by the entry that matched, not its most specific entry
        let variants = vec![
            v(&["unix", "windows/arm64"], None, false),
            v(&[&this_os], None, false),
        ];
        assert_eq!(
            select(&variants, &[]),
            Selection::Variant(v(&[&this_os], None, false))
        );
    }

    #[cfg(windows)]
    #[test]
    fn unix_family_does_not_select_windows() {
        let variants = vec![v(&["unix"], None, false)];
        assert_eq!(select(&variants, &[]), Selection::NoMatch);
    }

    #[test]
    fn stream_names() {
        assert_ne!(
            v(&["macos", "linux"], None, false).selector_name(),
            v(&["macos", "windows"], None, false).selector_name()
        );
        assert_eq!(v(&["macos"], None, false).selector_name(), "macos");
        assert_eq!(
            v(&["linux/arm64"], Some("work"), false).selector_name(),
            "linux-arm64+work"
        );
        assert_eq!(v(&[], Some("work"), false).selector_name(), "work");
        assert_eq!(v(&[], None, true).selector_name(), "default");
    }
}
