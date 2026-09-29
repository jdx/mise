use super::*;

#[test]
fn test_version_override_non_version_string_matches_semver() {
    // Non-version strings like "brew" are parsed as valid General versions
    // by the versions crate, and can match semver constraints unexpectedly.
    // This documents the root cause of the linked-version panic.
    let pkg = AquaPackage {
        version_constraint: "false".to_string(),
        version_overrides: vec![
            AquaPackage {
                version_constraint: "semver(\"<= 0.2.13\")".to_string(),
                error_message: Some("too old".to_string()),
                ..Default::default()
            },
            AquaPackage {
                version_constraint: "true".to_string(),
                asset: "tool-{{.Version}}.tar.gz".to_string(),
                format: "tar.gz".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let result = pkg.version_override(&["brew"]).unwrap();
    // "brew" matches semver("<= 0.2.13") instead of "true",
    // because Versioning::new("brew") parses as General(Alphanum("brew"))
    // which sorts before numeric versions.
    assert!(result.error_message.is_some());
    assert!(result.asset.is_empty());
}

/// Two overrides that differ only in which branch is picked, so `version_override`
/// reports the answer: `Some("first")` when the constraint held, `Some("fallback")` otherwise.
fn constraint_probe(constraint: &str) -> AquaPackage {
    AquaPackage {
        version_constraint: "false".to_string(),
        version_overrides: vec![
            AquaPackage {
                version_constraint: constraint.to_string(),
                asset: "first".to_string(),
                ..Default::default()
            },
            AquaPackage {
                version_constraint: "true".to_string(),
                asset: "fallback".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

fn constraint_holds(constraint: &str, version: &str) -> bool {
    constraint_probe(constraint)
        .version_override(&[version])
        .map(|pkg| pkg.asset == "first")
        .expect("a version_override always matches, since the last one is unconditional")
}

#[test]
fn test_semver_constraint_accepts_four_component_bounds() {
    // https://github.com/jdx/mise/discussions/4813: relocatable-perl selects the linux asset
    // name for old releases with semver("<= 5.34.1.0"). Rejecting that bound sent 5.32.1.0
    // to the fallback branch, which asks for an asset the release does not have.
    assert!(constraint_holds("semver(\"<= 5.34.1.0\")", "5.32.1.0"));
    assert!(constraint_holds("semver(\"<= 5.34.1.0\")", "5.34.1.0"));
    assert!(!constraint_holds("semver(\"<= 5.34.1.0\")", "5.36.0.0"));

    // Both directions, so "the bound is rejected and everything is false" cannot pass.
    assert!(constraint_holds("semver(\">= 1.0.0.0\")", "5.32.1.0"));
    assert!(!constraint_holds("semver(\"< 1.0.0.0\")", "5.32.1.0"));
}

#[test]
fn test_semver_constraint_compares_bounds_against_shorter_versions() {
    // haskell/cabal pins 4-component bounds while its releases are 3-component.
    assert!(constraint_holds("semver(\"<= 3.16.1.0\")", "3.16.1"));
    assert!(!constraint_holds("semver(\"<= 3.16.1.0\")", "3.17.0"));
}

#[test]
fn test_semver_constraint_supports_not_equal() {
    // mattn/efm-langserver and sheepla/qiitaz exclude a single bad release this way.
    assert!(constraint_holds("semver(\"!= 0.0.45\")", "0.0.46"));
    assert!(!constraint_holds("semver(\"!= 0.0.45\")", "0.0.45"));
}

#[test]
fn test_semver_constraint_still_ands_comma_separated_terms() {
    assert!(constraint_holds(
        "semver(\">= 1.0.0, <= 9.0.0\")",
        "5.32.1.0"
    ));
    assert!(!constraint_holds(
        "semver(\">= 1.0.0, <= 5.0.0\")",
        "5.32.1.0"
    ));
}

#[test]
fn test_semver_constraint_without_an_operator_is_not_satisfied() {
    // aqua requires an operator; an unparseable term errors, and the caller reads that
    // as "does not apply" rather than propagating it.
    assert!(!constraint_holds("semver(\"5.32.1.0\")", "5.32.1.0"));
    assert!(!constraint_holds("semver(\"<= \")", "5.32.1.0"));
}

#[test]
fn test_version_override_matches_version_prefix() {
    let pkg = AquaPackage {
        version_constraint: "false".to_string(),
        version_overrides: vec![
            AquaPackage {
                version_constraint: "semver(\">= 1.17.0\")".to_string(),
                version_prefix: Some("oxlint_v".to_string()),
                error_message: Some("unavailable".to_string()),
                ..Default::default()
            },
            AquaPackage {
                version_constraint: "true".to_string(),
                version_prefix: Some("apps_v".to_string()),
                asset: "oxlint.tar.gz".to_string(),
                format: "tar.gz".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let apps = pkg.version_override(&["apps_v1.76.0"]).unwrap();
    assert_eq!(apps.version_prefix.as_deref(), Some("apps_v"));
    assert!(apps.error_message.is_none());
    assert_eq!(apps.asset, "oxlint.tar.gz");

    let oxlint = pkg.version_override(&["oxlint_v1.76.0"]).unwrap();
    assert_eq!(oxlint.version_prefix.as_deref(), Some("oxlint_v"));
    assert_eq!(oxlint.error_message.as_deref(), Some("unavailable"));
}

#[test]
fn test_unconditional_version_override_matches_version_prefix() {
    let pkg = AquaPackage {
        version_constraint: "false".to_string(),
        version_overrides: vec![
            AquaPackage {
                version_prefix: Some("old_v".to_string()),
                error_message: Some("unavailable".to_string()),
                ..Default::default()
            },
            AquaPackage {
                version_prefix: Some("new_v".to_string()),
                asset: "tool.tar.gz".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let old = pkg.version_override(&["old_v1.0.0"]).unwrap();
    assert_eq!(old.version_prefix.as_deref(), Some("old_v"));
    assert_eq!(old.error_message.as_deref(), Some("unavailable"));

    let new = pkg.version_override(&["new_v1.0.0"]).unwrap();
    assert_eq!(new.version_prefix.as_deref(), Some("new_v"));
    assert_eq!(new.asset, "tool.tar.gz");
}

#[test]
fn test_unconditional_package_ignores_version_prefix() {
    let pkg = AquaPackage {
        version_prefix: Some("jq-".to_string()),
        asset: "jq-{{.OS}}-{{.Arch}}".to_string(),
        ..Default::default()
    };

    let resolved = pkg.version_override(&["1.8.1"]).unwrap();
    assert_eq!(resolved.version_prefix.as_deref(), Some("jq-"));
    assert_eq!(resolved.asset, "jq-{{.OS}}-{{.Arch}}");
}
