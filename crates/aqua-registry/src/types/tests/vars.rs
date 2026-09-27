use super::*;

#[test]
fn test_vars_default_value() {
    let pkg = AquaPackage {
        asset: "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string(),
        vars: vec![AquaVar {
            name: "channel".to_string(),
            default: default_str("stable"),
            required: false,
        }],
        ..Default::default()
    };
    let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();
    assert_eq!(asset, "tool-stable-1.0.0.tar.gz");
}

#[test]
fn test_vars_override_value() {
    let mut var_values = HashMap::new();
    var_values.insert("channel".to_string(), "beta".to_string());
    let pkg = AquaPackage {
        asset: "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string(),
        vars: vec![AquaVar {
            name: "channel".to_string(),
            default: default_str("stable"),
            required: false,
        }],
        ..Default::default()
    }
    .with_var_values(var_values)
    .unwrap();
    let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();
    assert_eq!(asset, "tool-beta-1.0.0.tar.gz");
}

#[test]
fn test_vars_default_unquoted_yaml_string() {
    let yml = r#"
packages:
  - asset: tool-{{.Vars.channel}}-{{.Version}}.tar.gz
    vars:
      - name: channel
        default: stable
"#;
    let pkg = first_registry_package(yml);
    let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();
    assert_eq!(asset, "tool-stable-1.0.0.tar.gz");
}

#[test]
fn test_vars_scalar_defaults_deserialize_as_strings() {
    for (yaml_default, expected) in [("true", "true"), ("123", "123")] {
        let yml = format!(
            r#"
packages:
  - vars:
      - name: channel
        default: {yaml_default}
"#
        );
        let pkg = first_registry_package(&yml);
        assert_eq!(pkg.vars[0].default.as_deref(), Some(expected));
    }
}

#[test]
fn test_vars_null_default_deserializes_as_none() {
    let yml = r#"
packages:
  - vars:
      - name: channel
        default: null
"#;
    let pkg = first_registry_package(yml);
    assert_eq!(pkg.vars[0].default, None);
}

#[test]
fn test_vars_sequence_and_mapping_defaults_fail_yaml_parse() {
    for yaml_default in ["[stable, beta]", "{channel: stable}"] {
        let yml = format!(
            r#"
packages:
  - vars:
      - name: channel
        default: {yaml_default}
"#
        );
        let err = serde_yaml::from_str::<RegistryYaml>(&yml).unwrap_err();
        assert!(
            err.to_string().contains("invalid type"),
            "unexpected error for {yaml_default}: {err}"
        );
    }
}

#[test]
fn test_vars_required_missing() {
    let pkg = AquaPackage {
        asset: "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string(),
        vars: vec![AquaVar {
            name: "channel".to_string(),
            default: None,
            required: true,
        }],
        ..Default::default()
    };
    let err = pkg.asset("1.0.0", "linux", "amd64").unwrap_err();
    assert!(
        err.to_string()
            .contains("required aqua var not set: channel"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_vars_required_missing_with_var_values() {
    let pkg = AquaPackage {
        vars: vec![AquaVar {
            name: "go_version".to_string(),
            default: None,
            required: true,
        }],
        ..Default::default()
    };
    let err = pkg.with_var_values(HashMap::new()).unwrap_err();
    assert!(
        err.to_string()
            .contains("required aqua var not set: go_version"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_vars_empty_name() {
    let pkg = AquaPackage {
        vars: vec![AquaVar {
            name: String::new(),
            default: None,
            required: false,
        }],
        ..Default::default()
    };
    let err = pkg.asset("1.0.0", "linux", "amd64").unwrap_err();
    assert!(
        err.to_string().contains("aqua var name is empty"),
        "unexpected error: {err}"
    );
}
