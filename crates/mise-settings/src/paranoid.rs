//! The `paranoid` setting: a boolean, or the list of paranoid categories to turn on.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

/// One independently selectable part of paranoid mode.
///
/// Any new behavior gated on paranoid mode belongs to one of these, or to a new
/// category added here, so it can be selected on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParanoidCategory {
    /// Content-hashed trust for every config outside global and system config, no
    /// automatic trust, no CI or `--yes` answer to the trust prompt, and no trust
    /// shared between worktrees or through a monorepo marker.
    Config,
    /// Content-bound trust for files that `env._.file`, `env._.source` and the
    /// `env_file` setting load.
    EnvFiles,
    /// Refuse to install an unregistered community plugin by short name.
    Plugins,
    /// Re-verify provenance on every install and confirm a mise-versions
    /// "no attestations" answer with GitHub.
    Provenance,
}

impl ParanoidCategory {
    pub const ALL: [Self; 4] = [
        Self::Config,
        Self::EnvFiles,
        Self::Plugins,
        Self::Provenance,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::EnvFiles => "env_files",
            Self::Plugins => "plugins",
            Self::Provenance => "provenance",
        }
    }

    /// The valid names, as `config, env_files, plugins, provenance`.
    pub fn names() -> String {
        Self::ALL.map(Self::as_str).join(", ")
    }
}

impl Display for ParanoidCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ParanoidCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|category| category.as_str() == s)
            .ok_or_else(|| {
                format!(
                    "unknown paranoid category {s:?}; valid categories are {}",
                    Self::names()
                )
            })
    }
}

/// Which paranoid categories are on. `true` selects every category, `false` none.
///
/// Read it through [`Paranoid::config`], [`Paranoid::env_files`],
/// [`Paranoid::plugins`] and [`Paranoid::provenance`], never as a plain bool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paranoid(BTreeSet<ParanoidCategory>);

impl Paranoid {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn all() -> Self {
        Self(ParanoidCategory::ALL.into())
    }

    pub fn only(categories: impl IntoIterator<Item = ParanoidCategory>) -> Self {
        Self(categories.into_iter().collect())
    }

    pub fn has(&self, category: ParanoidCategory) -> bool {
        self.0.contains(&category)
    }

    pub fn config(&self) -> bool {
        self.has(ParanoidCategory::Config)
    }

    pub fn env_files(&self) -> bool {
        self.has(ParanoidCategory::EnvFiles)
    }

    pub fn plugins(&self) -> bool {
        self.has(ParanoidCategory::Plugins)
    }

    pub fn provenance(&self) -> bool {
        self.has(ParanoidCategory::Provenance)
    }

    pub fn is_any(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn categories(&self) -> impl Iterator<Item = ParanoidCategory> + '_ {
        self.0.iter().copied()
    }

    /// A stable string for the selection, for cache keys: differing selections never match.
    pub fn fingerprint(&self) -> String {
        self.categories()
            .map(ParanoidCategory::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl Paranoid {
    /// `Some(true)` or `Some(false)` when `s` is a spelling of a boolean that [`Paranoid::from_str`]
    /// accepts, `None` when it should be read as a list of categories.
    pub fn bool_spelling(s: &str) -> Option<bool> {
        match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "on" | "1" => Some(true),
            "false" | "no" | "n" | "off" | "0" | "" => Some(false),
            _ => None,
        }
    }
}

impl From<bool> for Paranoid {
    fn from(value: bool) -> Self {
        if value { Self::all() } else { Self::none() }
    }
}

impl FromStr for Paranoid {
    type Err = String;

    /// Parses `true`, `false` and their usual spellings, or a comma separated list of
    /// category names.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(value) = Self::bool_spelling(s) {
            return Ok(value.into());
        }
        s.split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(ParanoidCategory::from_str)
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

impl Display for Paranoid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.0.len() == ParanoidCategory::ALL.len() {
            f.write_str("true")
        } else if self.0.is_empty() {
            f.write_str("false")
        } else {
            f.write_str(&self.fingerprint())
        }
    }
}

impl<'de> Deserialize<'de> for Paranoid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, SeqAccess, Visitor};

        struct ParanoidVisitor;

        impl<'de> Visitor<'de> for ParanoidVisitor {
            type Value = Paranoid;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                write!(
                    formatter,
                    "a boolean or a list of paranoid categories ({})",
                    ParanoidCategory::names()
                )
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Paranoid, E> {
                Ok(value.into())
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Paranoid, E> {
                value.parse().map_err(E::custom)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Paranoid, A::Error> {
                let mut categories = BTreeSet::new();
                while let Some(name) = seq.next_element::<String>()? {
                    categories.insert(name.trim().parse().map_err(de::Error::custom)?);
                }
                Ok(Paranoid(categories))
            }
        }

        deserializer.deserialize_any(ParanoidVisitor)
    }
}

impl Serialize for Paranoid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.len() == ParanoidCategory::ALL.len() {
            serializer.serialize_bool(true)
        } else if self.0.is_empty() {
            serializer.serialize_bool(false)
        } else {
            serializer.collect_seq(self.categories().map(ParanoidCategory::as_str))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Paranoid, String> {
        s.parse()
    }

    #[test]
    fn bool_spellings_select_all_or_none() {
        for on in ["true", "TRUE", "1", "yes", "on", " true "] {
            assert_eq!(parse(on).unwrap(), Paranoid::all(), "{on}");
        }
        for off in ["false", "0", "no", "off", ""] {
            assert_eq!(parse(off).unwrap(), Paranoid::none(), "{off}");
        }
    }

    #[test]
    fn bool_spelling_agrees_with_parsing() {
        for s in ["TRUE", " On ", "y", "off", "No", "", "0"] {
            assert_eq!(
                Paranoid::from(Paranoid::bool_spelling(s).unwrap()),
                parse(s).unwrap(),
                "{s}"
            );
        }
        assert_eq!(Paranoid::bool_spelling("config"), None);
    }

    #[test]
    fn comma_list_selects_categories() {
        let p = parse("config, env_files").unwrap();
        assert!(p.config() && p.env_files() && !p.plugins() && !p.provenance());
        assert_eq!(parse("plugins,plugins").unwrap(), parse("plugins").unwrap());
    }

    #[test]
    fn unknown_category_names_the_valid_ones() {
        let err = parse("config,bogus").unwrap_err();
        assert!(err.contains("bogus"), "{err}");
        assert!(
            err.contains("config, env_files, plugins, provenance"),
            "{err}"
        );
    }

    #[test]
    fn accessors_map_to_one_category_each() {
        for category in ParanoidCategory::ALL {
            let p = Paranoid::only([category]);
            let on = [p.config(), p.env_files(), p.plugins(), p.provenance()];
            assert_eq!(on.iter().filter(|b| **b).count(), 1, "{category}");
            assert!(p.has(category));
            assert!(p.is_any());
        }
        let none = Paranoid::none();
        assert!(!none.is_any() && !none.config() && !none.env_files());
        let all = Paranoid::all();
        assert!(all.config() && all.env_files() && all.plugins() && all.provenance());
    }

    #[test]
    fn toml_accepts_bool_or_list() {
        #[derive(Deserialize)]
        struct T {
            paranoid: Paranoid,
        }
        let t = |s: &str| toml::from_str::<T>(s).map(|t| t.paranoid);
        assert_eq!(t("paranoid = true").unwrap(), Paranoid::all());
        assert_eq!(t("paranoid = false").unwrap(), Paranoid::none());
        assert_eq!(
            t(r#"paranoid = ["env_files"]"#).unwrap(),
            Paranoid::only([ParanoidCategory::EnvFiles])
        );
        assert_eq!(t("paranoid = []").unwrap(), Paranoid::none());
        let err = t(r#"paranoid = ["nope"]"#).unwrap_err().to_string();
        assert!(
            err.contains("config, env_files, plugins, provenance"),
            "{err}"
        );
        assert!(t("paranoid = 3").is_err());
    }

    #[test]
    fn serializes_as_bool_or_list() {
        #[derive(Serialize)]
        struct T {
            paranoid: Paranoid,
        }
        let s = |p| toml::to_string(&T { paranoid: p }).unwrap();
        assert_eq!(s(Paranoid::all()), "paranoid = true\n");
        assert_eq!(s(Paranoid::none()), "paranoid = false\n");
        assert_eq!(
            s(Paranoid::only([
                ParanoidCategory::Config,
                ParanoidCategory::Plugins
            ])),
            "paranoid = [\"config\", \"plugins\"]\n"
        );
    }

    #[test]
    fn fingerprints_differ_per_selection() {
        let mut seen = BTreeSet::new();
        for mask in 0..16u8 {
            let p = Paranoid::only(
                ParanoidCategory::ALL
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| mask & (1 << i) != 0)
                    .map(|(_, c)| c),
            );
            assert!(seen.insert(p.fingerprint()));
        }
    }
}
