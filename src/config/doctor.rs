use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Relative to the configuration file's project root.
    pub dir: Option<PathBuf>,
    /// Shell executable and arguments, including the command flag.
    pub shell: Option<Vec<String>>,
    /// Operating systems on which this check applies.
    pub os: Option<Vec<OperatingSystem>>,
}

#[derive(Debug, Clone, Deserialize, strum::AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub(crate) enum OperatingSystem {
    Linux,
    Macos,
    Windows,
    Freebsd,
    Openbsd,
    Netbsd,
    Dragonfly,
    Android,
    Illumos,
    Solaris,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_check_options_and_operating_systems() {
        for body in [
            "[checks.db]\nrun = 'true'\ntimeuot = '1s'",
            "[checks.db]\nrun = 'true'\nos = ['linxu']",
            "[checks.db]\nhint = 'missing command'",
        ] {
            assert!(toml::from_str::<DoctorConfig>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn accepts_platform_and_shell_overrides() {
        let config: DoctorConfig = toml::from_str(
            "[checks.db]\nrun = 'exit 0'\nos = ['windows']\nshell = ['pwsh', '-Command']",
        )
        .unwrap();
        assert_eq!(
            config.checks["db"].os.as_ref().unwrap()[0].as_ref(),
            "windows"
        );
        assert_eq!(
            config.checks["db"].shell.as_ref().unwrap(),
            &["pwsh", "-Command"]
        );
    }
}
