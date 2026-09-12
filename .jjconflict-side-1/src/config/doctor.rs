use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct DoctorConfig {
    #[serde(default)]
    pub checks: BTreeMap<String, DoctorCheck>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DoctorCheck {
    pub run: String,
    pub description: Option<String>,
    pub hint: Option<String>,
    pub timeout: Option<String>,
    /// Relative paths use the configuration file's root (the invocation directory
    /// for global and system configuration); a leading `~/` and absolute paths are
    /// used as given.
    pub dir: Option<PathBuf>,
    /// Shell executable and arguments, including the command flag.
    pub shell: Option<String>,
    /// Operating systems on which this check applies.
    #[serde(default, deserialize_with = "deserialize_os")]
    pub os: Vec<String>,
}

// Omitted selectors run everywhere; an explicit empty list is almost always a
// mistake and is rejected, consistent with the shared os_filter schema.
fn deserialize_os<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let selectors: Vec<String> = crate::config::config_file::toml::deserialize_arr(deserializer)?;
    if selectors.is_empty()
        || selectors.iter().any(|selector| {
            let parts = selector.split('/').collect::<Vec<_>>();
            parts.len() > 2
                || parts.iter().any(|part| {
                    part.is_empty()
                        || !part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_.+-".contains(&c))
                })
        })
    {
        return Err(serde::de::Error::custom(
            "os must contain at least one valid OS or OS/arch selector",
        ));
    }
    Ok(selectors)
}

impl DoctorCheck {
    pub(crate) fn applies(&self) -> bool {
        self.os.is_empty()
            || self
                .os
                .iter()
                .any(|os| crate::cli::version::os_selector_matches(os))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_check_options() {
        for body in [
            "[checks.db]\nrun = 'true'\ntimeuot = '1s'",
            "[checks.db]\nrun = 'true'\nos = []",
            "[checks.db]\nhint = 'missing command'",
        ] {
            assert!(toml::from_str::<DoctorConfig>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn accepts_platform_and_shell_overrides() {
        for os in ["'darwin'", "['win', 'linux/arm64']", "'linux/x86_64'"] {
            let config: DoctorConfig = toml::from_str(&format!(
                "[checks.db]\nrun = 'exit 0'\nos = {os}\nshell = 'pwsh -Command'"
            ))
            .unwrap();
            assert!(!config.checks["db"].os.is_empty());
            assert_eq!(config.checks["db"].shell.as_deref(), Some("pwsh -Command"));
        }
    }

    #[test]
    fn accepts_future_container_options() {
        let config: DoctorConfig = toml::from_str("[settings]\nfuture = true").unwrap();
        assert!(config.checks.is_empty());
    }
}
