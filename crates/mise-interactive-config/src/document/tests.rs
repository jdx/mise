use super::*;

#[test]
fn test_new_document() {
    let doc = TomlDocument::new();
    assert_eq!(doc.sections.len(), 4);
    assert_eq!(doc.sections[0].name, "tools");
    assert!(doc.sections[0].expanded);
}

#[test]
fn test_parse_simple() {
    let content = r#"
[tools]
node = "22"
python = "3.12"

[env]
NODE_ENV = "development"
"#;
    let doc = TomlDocument::parse(content).unwrap();
    assert_eq!(doc.sections[0].name, "tools");
    assert_eq!(doc.sections[0].entries.len(), 2);
    assert_eq!(doc.sections[0].entries[0].key, "node");
}

#[test]
fn test_parse_array() {
    let content = r#"
[env]
paths = ["./bin", "./node_modules/.bin"]
"#;
    let doc = TomlDocument::parse(content).unwrap();
    let env_section = doc.sections.iter().find(|s| s.name == "env").unwrap();
    let entry = &env_section.entries[0];
    assert_eq!(entry.key, "paths");
    assert!(matches!(entry.value, EntryValue::Array(_)));
    if let EntryValue::Array(items) = &entry.value {
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], "./bin");
    }
}

#[test]
fn test_to_toml() {
    let mut doc = TomlDocument::new();
    doc.add_entry(0, "node".to_string(), "22".to_string());
    let toml = doc.to_toml();
    assert!(toml.contains("[tools]"));
    assert!(toml.contains("node = \"22\""));
}

#[test]
fn test_roundtrip_keeps_comments() {
    // Everything here came back stripped before: the banner, the comment
    // above the section, the comment above an entry, and the same-line
    // comment. See discussion #10650.
    let content = r#"# managed by the platform team

[tools]
# language runtimes
node = "22"

[env]
FOO = "bar" # why this is set
"#;
    let doc = TomlDocument::parse(content).unwrap();
    let output = doc.to_toml();
    assert!(
        output.contains("# managed by the platform team"),
        "banner lost: {output}"
    );
    assert!(
        output.contains("# language runtimes"),
        "section-level comment lost: {output}"
    );
    assert!(
        output.contains("# why this is set"),
        "trailing comment lost: {output}"
    );
    // The trailing comment has to stay on its own line, not become a leading
    // one for the next entry.
    assert!(
        output.contains(r#"FOO = "bar" # why this is set"#),
        "trailing comment moved: {output}"
    );
}

#[test]
fn test_roundtrip_without_comments_adds_nothing() {
    let content = r#"[tools]
node = "22"
"#;
    let doc = TomlDocument::parse(content).unwrap();
    let output = doc.to_toml();
    assert!(!output.contains('#'), "invented a comment: {output}");
}

#[test]
fn test_roundtrip() {
    let content = r#"[tools]
node = "22"
python = "3.12"

[env]
NODE_ENV = "development"
"#;
    let doc = TomlDocument::parse(content).unwrap();
    let output = doc.to_toml();
    assert!(output.contains("node = \"22\""));
    assert!(output.contains("python = \"3.12\""));
    assert!(output.contains("NODE_ENV = \"development\""));
}

#[test]
fn test_parse_top_level_entries() {
    let content = r#"min_version = "2024.1.0"

[tools]
node = "22"
"#;
    let doc = TomlDocument::parse(content).unwrap();
    // Root section (empty name) should be first
    let root_section = doc.sections.iter().find(|s| s.name.is_empty()).unwrap();
    assert_eq!(root_section.entries.len(), 1);
    assert_eq!(root_section.entries[0].key, "min_version");
}

#[test]
fn test_top_level_entries_roundtrip() {
    let content = r#"min_version = "2024.1.0"

[tools]
node = "22"
"#;
    let doc = TomlDocument::parse(content).unwrap();
    let output = doc.to_toml();
    assert!(output.contains("min_version = \"2024.1.0\""));
    assert!(output.contains("[tools]"));
    assert!(output.contains("node = \"22\""));
}

#[test]
fn test_env_dotted_key_serialization() {
    // Create a document with _.path in the env section
    let mut doc = TomlDocument::new();
    let env_idx = doc.sections.iter().position(|s| s.name == "env").unwrap();

    // Add _.path as an array
    doc.sections[env_idx].entries.push(Entry {
        key: "_.path".to_string(),
        value: EntryValue::Array(vec!["./bin".to_string(), "./node_modules/.bin".to_string()]),
        expanded: false,
        comments: Vec::new(),
        trailing_comment: None,
    });

    let output = doc.to_toml();
    // Should output as dotted key, not quoted key
    // _.path = [...] means _: { path: [...] }
    assert!(
        output.contains("_.path") || output.contains("[env._]"),
        "Output should contain dotted key notation: {}",
        output
    );
    // Should NOT contain quoted key
    assert!(
        !output.contains("\"_.path\""),
        "Output should not contain quoted key: {}",
        output
    );
}

#[test]
fn test_save_creates_missing_directories() {
    // `mise generate config --global` on a fresh install points here at
    // ~/.config/mise/config.toml, and neither the file nor its directory exists yet.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mise").join("config.toml");
    assert!(!path.parent().unwrap().exists());

    TomlDocument::new().save(&path).unwrap();

    assert!(path.is_file());
}

#[test]
fn test_save_still_writes_into_an_existing_directory() {
    // Control for the case above: without it, that test would also pass if `save` had
    // started creating a directory *at* `path` and writing nothing.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    TomlDocument::new().save(&path).unwrap();

    assert!(path.is_file());
}

#[test]
fn test_a_bare_relative_name_has_an_empty_parent_that_is_safe_to_create() {
    // What `save` relies on for `mise edit foo.toml`, pinned here rather than assumed:
    // the parent is the empty path, and creating that is a no-op rather than an error.
    // Asserted without touching the process's current directory, which the test harness
    // shares across threads.
    assert_eq!(Path::new("foo.toml").parent(), Some(Path::new("")));
    std::fs::create_dir_all(Path::new("")).unwrap();
}
