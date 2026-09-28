use super::*;

#[test]
fn test_set_by_comma_empty_string() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), BTreeSet::new());
}

#[test]
fn test_set_by_comma_whitespace_only() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("  ");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), BTreeSet::new());
}

#[test]
fn test_set_by_comma_single_value() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("foo");
    assert!(result.is_ok());
    let expected: BTreeSet<String> = ["foo".to_string()].into_iter().collect();
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_set_by_comma_multiple_values() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,baz");
    assert!(result.is_ok());
    let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string(), "baz".to_string()]
        .into_iter()
        .collect();
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_set_by_comma_with_whitespace() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("foo, bar, baz");
    assert!(result.is_ok());
    let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string(), "baz".to_string()]
        .into_iter()
        .collect();
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_set_by_comma_trailing_comma() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,");
    assert!(result.is_ok());
    let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string()].into_iter().collect();
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_set_by_comma_duplicate_values() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,foo");
    assert!(result.is_ok());
    let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string()].into_iter().collect();
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_set_by_comma_empty_elements() {
    let result: Result<BTreeSet<String>, _> = set_by_comma("foo,,bar");
    assert!(result.is_ok());
    let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string()].into_iter().collect();
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_normalize_tool_names() {
    let tools = BTreeSet::from([
        " node ".to_string(),
        "  ".to_string(),
        "ruby".to_string(),
        "".to_string(),
    ]);
    let expected = BTreeSet::from(["node".to_string(), "ruby".to_string()]);
    assert_eq!(normalize_tool_names(&tools), expected);
}

#[test]
fn test_list_by_os_path_separator_empty() {
    let result: Result<Vec<PathBuf>, _> = list_by_os_path_separator("");
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_list_by_os_path_separator_single() {
    #[cfg(not(windows))]
    let (input, expected) = ("/foo/bar", PathBuf::from("/foo/bar"));
    #[cfg(windows)]
    let (input, expected) = (r"C:\foo\bar", PathBuf::from(r"C:\foo\bar"));
    let result: Vec<PathBuf> = list_by_os_path_separator(input).unwrap();
    assert_eq!(result, vec![expected]);
}

#[test]
#[cfg(unix)]
fn test_list_by_os_path_separator_multiple_unix() {
    let result: Vec<PathBuf> = list_by_os_path_separator("/foo:/bar").unwrap();
    assert_eq!(result, vec![PathBuf::from("/foo"), PathBuf::from("/bar")]);
}

#[test]
#[cfg(windows)]
fn test_list_by_os_path_separator_multiple_windows() {
    let result: Vec<PathBuf> = list_by_os_path_separator(r"C:\foo;D:\bar").unwrap();
    assert_eq!(
        result,
        vec![PathBuf::from(r"C:\foo"), PathBuf::from(r"D:\bar")]
    );
}

#[test]
fn test_list_by_os_path_separator_as_btreeset() {
    // Verify the function works with BTreeSet as the collection type,
    // matching the field types used in Settings (e.g. trusted_config_paths).
    #[cfg(not(windows))]
    let (input, a, b) = ("/foo:/bar", PathBuf::from("/foo"), PathBuf::from("/bar"));
    #[cfg(windows)]
    let (input, a, b) = (
        r"C:\foo;D:\bar",
        PathBuf::from(r"C:\foo"),
        PathBuf::from(r"D:\bar"),
    );
    let result: BTreeSet<PathBuf> = list_by_os_path_separator(input).unwrap();
    assert_eq!(result, [a, b].into_iter().collect());
}

/// The workspace-root settings.toml, or the copy a published package carries. Same
/// precedence as `build.rs`.
fn settings_toml_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    let in_workspace = std::fs::canonicalize(root.join("crates/mise-settings"))
        .ok()
        .zip(std::fs::canonicalize(&manifest_dir).ok())
        .is_some_and(|(a, b)| a == b)
        && std::fs::read_to_string(root.join("Cargo.toml"))
            .ok()
            .and_then(|manifest| manifest.parse::<toml::Table>().ok())
            .is_some_and(|manifest| {
                manifest
                    .get("package")
                    .and_then(|package| package.get("name"))
                    .and_then(toml::Value::as_str)
                    == Some("mise")
            });
    let workspace = root.join("settings.toml");
    if in_workspace && workspace.exists() {
        workspace
    } else {
        manifest_dir.join("settings.toml")
    }
}

#[test]
fn test_settings_toml_is_sorted() {
    let content =
        std::fs::read_to_string(settings_toml_path()).expect("failed to read settings.toml");
    let table: toml::Table = content.parse().expect("failed to parse settings.toml");

    fn collect_keys(table: &toml::Table, prefix: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for (key, value) in table {
            let full_key = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if let toml::Value::Table(sub) = value {
                // A nested table that has no "type" or "description" is a grouping table
                // (e.g., [aqua], [node]), not a setting itself.
                if !sub.contains_key("type") && !sub.contains_key("description") {
                    keys.extend(collect_keys(sub, &full_key));
                    continue;
                }
            }
            keys.push(full_key);
        }
        keys
    }

    let keys = collect_keys(&table, "");
    let mut sorted = keys.clone();
    sorted.sort();

    for (i, (got, expected)) in keys.iter().zip(sorted.iter()).enumerate() {
        assert_eq!(
            got, expected,
            "settings.toml is not alphabetically sorted at index {i}: found \"{got}\", expected \"{expected}\". Run the sort script or reorder manually."
        );
    }
}

/// Every scalar setting type in settings.toml. Anything else is a collection.
///
/// Spelled as "not a scalar" rather than by listing the collections on purpose. A new
/// collection type — `SetPath`, say — has the same failure mode and is caught here
/// automatically, whereas enumerating `List*`/`SetString`/`IndexMap<…>` would let it through
/// silently, which is the exact failure this guard exists to prevent. A new *scalar* type
/// instead fails this test loudly and is fixed by adding one entry here, which is the cheaper
/// mistake to make.
const SCALAR_SETTING_TYPES: &[&str] = &[
    "Bool",
    "BoolOrString",
    "Duration",
    "Integer",
    "Path",
    "String",
    "Url",
];

/// Collect settings that are readable from the environment, hold a collection, and declare no
/// `parse_env`.
fn collect_settings_missing_parse_env(
    table: &toml::Table,
    prefix: &str,
    missing: &mut Vec<String>,
) {
    for (key, value) in table {
        let toml::Value::Table(setting) = value else {
            continue;
        };
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        // A nested table that has no "type" or "description" is a grouping table
        // (e.g., [aqua], [node]), not a setting itself.
        if !setting.contains_key("type") && !setting.contains_key("description") {
            collect_settings_missing_parse_env(setting, &full_key, missing);
            continue;
        }
        let is_collection = setting
            .get("type")
            .and_then(|type_| type_.as_str())
            .is_some_and(|type_| !SCALAR_SETTING_TYPES.contains(&type_));
        if is_collection && setting.contains_key("env") && !setting.contains_key("parse_env") {
            missing.push(full_key);
        }
    }
}

#[test]
fn test_settings_toml_collection_settings_declare_parse_env() {
    // A collection setting that can be set from the environment needs `parse_env`. Without it
    // confique hands the raw string to a `Vec`, set or map deserializer and mise aborts before
    // doing anything: "failed to deserialize value `SettingsAge::identity_files` from
    // environment variable `MISE_AGE_IDENTITY_FILES`: invalid type: string "...", expected a
    // sequence".
    let content =
        std::fs::read_to_string(settings_toml_path()).expect("failed to read settings.toml");
    let table: toml::Table = content.parse().expect("failed to parse settings.toml");

    let mut missing = vec![];
    collect_settings_missing_parse_env(&table, "", &mut missing);

    assert!(
        missing.is_empty(),
        "these collection settings are readable from the environment but declare no \
         parse_env, so mise panics as soon as one is set: {missing:?}"
    );
}

/// The guard above only earns its place if it catches every collection type, not just `List*`.
/// settings.toml also carries `SetString` and `IndexMap<String, String>`, which fail the same
/// way, so a violation of each is checked against a fixture here rather than waiting for one
/// to be committed.
#[test]
fn test_parse_env_guard_covers_sets_and_maps_too() {
    let table: toml::Table = r#"
        [bad_list]
        description = "x"
        type = "ListString"
        env = "MISE_BAD_LIST"

        [bad_set]
        description = "x"
        type = "SetString"
        env = "MISE_BAD_SET"

        [bad_map]
        description = "x"
        type = "IndexMap<String, String>"
        env = "MISE_BAD_MAP"

        [group.bad_nested]
        description = "x"
        type = "ListPath"
        env = "MISE_BAD_NESTED"

        [ok_has_parse_env]
        description = "x"
        type = "SetString"
        env = "MISE_OK_PARSE"
        parse_env = "set_by_comma"

        [ok_no_env]
        description = "x"
        type = "SetString"

        [ok_scalar]
        description = "x"
        type = "String"
        env = "MISE_OK_SCALAR"
    "#
    .parse()
    .expect("fixture parses");

    let mut missing = vec![];
    collect_settings_missing_parse_env(&table, "", &mut missing);
    missing.sort();

    assert_eq!(
        missing,
        vec!["bad_list", "bad_map", "bad_set", "group.bad_nested"]
    );
}
