//! `[doctor]`: project diagnostic checks.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::refs;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// Project diagnostic checks, run explicitly with mise doctor project.
// Unknown container keys are accepted so newer options do not break older mise versions.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[schemars(extend("additionalProperties" = true))]
pub struct DoctorConfig {
    /// Checks run by mise doctor project, keyed by check name.
    #[serde(default)]
    pub checks: BTreeMap<String, DoctorCheck>,
}

/// A check: a command that passes when it exits zero.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoctorCheck {
    /// Diagnostic command. Exit zero to pass; other exit codes fail.
    #[schemars(length(min = 1))]
    pub run: String,
    /// Human-readable requirement checked by the command.
    pub description: Option<String>,
    /// Remediation guidance displayed on failure; never executed.
    pub hint: Option<String>,
    /// Positive command timeout, such as 5s. Defaults to 10s.
    pub timeout: Option<String>,
    /// Working directory. Relative paths resolve from the declaring config's root (the invocation directory for global and system config); a leading ~/ and absolute paths are used as given.
    pub dir: Option<PathBuf>,
    /// Shell command, including the command flag, as in tasks.
    #[schemars(extend("examples" = ["bash -c", "pwsh -Command"]))]
    pub shell: Option<String>,
    /// OS or OS/arch selectors; omit to run everywhere.
    #[serde(default, deserialize_with = "deserialize_os")]
    #[schemars(schema_with = "refs::os_filter")]
    pub os: Vec<String>,
}

// Omitted selectors run everywhere; an explicit empty list is almost always a
// mistake and is rejected, consistent with the shared os_filter schema.
fn deserialize_os<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let selectors: Vec<String> = crate::de::deserialize_arr(deserializer)?;
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
