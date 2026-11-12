use super::*;
use crate::cursor::{AddButtonKind, CursorTarget};

/// Run "Load .env" on the `[env]` section of `editor`, entering `file`.
fn load_dotenv(editor: &mut InteractiveConfig, file: &str) {
    let env_idx = editor
        .doc
        .sections
        .iter()
        .position(|s| s.name == "env")
        .unwrap();
    editor.doc.sections[env_idx].expanded = true;
    editor.cursor.goto(
        &editor.doc,
        &CursorTarget::AddButton(AddButtonKind::EnvDotenv(env_idx)),
    );
    editor.apply_new_key(file.to_string());
}

#[test]
fn load_dotenv_writes_underscore_file() {
    let mut editor = InteractiveConfig::new(PathBuf::from("mise.toml"));
    load_dotenv(&mut editor, ".env");

    let output = editor.doc.to_toml();
    assert!(
        output.contains("_.file") || output.contains("[env._]"),
        "Load .env should write the `_` key: {output}"
    );
    assert!(output.contains(".env"), "{output}");
    assert!(
        !output.contains("mise"),
        "Load .env should not write the deprecated env.mise table: {output}"
    );
}

#[test]
fn load_dotenv_keeps_existing_underscore_entry_and_undoes() {
    let mut editor = InteractiveConfig::new(PathBuf::from("mise.toml"));
    editor.doc = TomlDocument::parse("[env]\n_.source = \"a.sh\"\n").unwrap();
    load_dotenv(&mut editor, ".env");

    let output = editor.doc.to_toml();
    assert!(
        output.contains(".env") && output.contains("a.sh"),
        "Load .env should keep the existing source: {output}"
    );

    editor.undo();
    let output = editor.doc.to_toml();
    assert!(
        output.contains("a.sh") && !output.contains(".env"),
        "Undo should remove only the loaded file: {output}"
    );
}
