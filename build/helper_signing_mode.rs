/// Describes the signing behavior for the embedded notification helper.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum HelperSigningMode {
    /// Skip codesigning and omit signature resources.
    Disabled,
    /// Use an ad-hoc signature without marking the helper release-signed.
    AdHoc,
    /// Use a Developer ID signature and mark the helper release-signed.
    Release,
}

/// Selects helper signing behavior from the setting and signing identity.
pub(crate) fn select(signing: Option<&str>, identity: &str) -> HelperSigningMode {
    if signing == Some("disabled") {
        HelperSigningMode::Disabled
    } else if identity == "-" {
        HelperSigningMode::AdHoc
    } else {
        HelperSigningMode::Release
    }
}

#[cfg(test)]
mod tests {
    use super::{HelperSigningMode, select};

    /// Covers the default ad-hoc, disabled, and unset-setting Developer ID modes.
    #[test]
    fn selects_expected_helper_signing_modes() {
        let cases = [
            ("default ad-hoc", None, "-", HelperSigningMode::AdHoc),
            (
                "disabled Developer ID",
                Some("disabled"),
                "Developer ID Application: mise",
                HelperSigningMode::Disabled,
            ),
            (
                "unset Developer ID",
                None,
                "Developer ID Application: mise",
                HelperSigningMode::Release,
            ),
        ];

        for (name, signing, identity, expected) in cases {
            assert_eq!(select(signing, identity), expected, "{name}");
        }
    }
}
