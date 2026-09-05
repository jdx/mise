//! Variant selection for tracked files: which shared stream a machine
//! belongs to, using the same conventions as bootstrap packages (`os`, with
//! an optional `/arch`) and mise environments (`profile`).

use serde::Deserialize;

/// One `variants = [{ … }]` element of a `[dotfiles]` track entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Variant {
    /// `"macos"`, `"linux/arm64"`, or a list; empty = any.
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub os: Vec<String>,
    /// Active when this mise environment is selected (`-E work`).
    #[serde(default)]
    pub profile: Option<String>,
    /// The explicit fallback stream for machines matching no other variant.
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub share: Option<bool>,
}

impl Variant {
    /// The stream name recorded in checkpoints and used in the setup branch:
    /// `macos`, `linux-arm64`, `macos+work`, `work`, or `default`.
    pub(crate) fn name(&self) -> String {
        let mut parts = vec![];
        if let Some(os) = self.os.first() {
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

    fn matches(&self, environments: &[String]) -> bool {
        let os_ok = self.os.is_empty()
            || self
                .os
                .iter()
                .any(|entry| crate::cli::version::os_selector_matches(entry));
        let profile_ok = self
            .profile
            .as_ref()
            .is_none_or(|profile| environments.iter().any(|env| env == profile));
        os_ok && profile_ok
    }

    /// `profile` +2, an arch qualifier +2, an os alone +1.
    fn specificity(&self) -> u8 {
        let mut score = 0;
        if self.profile.is_some() {
            score += 2;
        }
        if !self.os.is_empty() {
            score += 1;
            if self.os.iter().any(|entry| entry.contains('/')) {
                score += 1;
            }
        }
        score
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    /// The entry has no variants: one stream for every machine.
    Single,
    /// The variant this machine uses.
    Variant(Variant),
    /// No variant matches and none is the default: local protection only.
    NoMatch,
    /// Two variants match with the same specificity.
    Ambiguous(Vec<Variant>),
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

    fn v(os: &[&str], profile: Option<&str>, default: bool) -> Variant {
        Variant {
            os: os.iter().map(|s| s.to_string()).collect(),
            profile: profile.map(str::to_string),
            default,
            share: None,
        }
    }

    #[test]
    fn selection_prefers_the_most_specific_match() {
        let this_os = crate::cli::version::OS.to_string();
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

    #[test]
    fn stream_names() {
        assert_eq!(v(&["macos"], None, false).name(), "macos");
        assert_eq!(
            v(&["linux/arm64"], Some("work"), false).name(),
            "linux-arm64+work"
        );
        assert_eq!(v(&[], Some("work"), false).name(), "work");
        assert_eq!(v(&[], None, true).name(), "default");
    }
}
