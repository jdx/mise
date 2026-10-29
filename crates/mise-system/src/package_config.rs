//! Behavior for `[bootstrap.packages]` entries that needs the package managers or the host.
//!
//! `PackageTomlConfig` is plain data in `mise-config`, which depends on neither. These methods
//! read it here instead, where both are available.

use mise_config::packages::{PackageDesiredStateTomlConfig, PackageTomlConfig};
use mise_system_packages::PackageDesiredState;
use mise_system_packages::snap::SnapOptions;

pub(crate) trait PackageTomlConfigExt {
    /// The `channel` and `classic` options on this entry, if it sets either.
    fn snap_options(&self) -> Option<SnapOptions>;
    fn desired(&self) -> PackageDesiredState;
    /// Whether the entry's `os` selectors include this host. An entry without them applies
    /// everywhere.
    fn is_os_supported(&self) -> bool;
}

impl PackageTomlConfigExt for PackageTomlConfig {
    fn snap_options(&self) -> Option<SnapOptions> {
        match self {
            Self::Options(options) if options.channel.is_some() || options.classic.is_some() => {
                Some(SnapOptions {
                    channel: options.channel.clone(),
                    classic: options.classic.unwrap_or(false),
                })
            }
            _ => None,
        }
    }

    fn desired(&self) -> PackageDesiredState {
        match self {
            Self::Version(_) => PackageDesiredState::Present,
            Self::Options(options) => match options.state {
                PackageDesiredStateTomlConfig::Present => PackageDesiredState::Present,
                PackageDesiredStateTomlConfig::Absent => PackageDesiredState::Absent,
            },
        }
    }

    fn is_os_supported(&self) -> bool {
        let Self::Options(options) = self else {
            return true;
        };
        options.os.is_empty()
            || options
                .os
                .iter()
                .any(|entry| mise_util::platform::os_selector_matches(entry))
    }
}
