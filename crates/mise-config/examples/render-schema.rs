//! Writes `schema/mise.json`, `schema/mise-task.json` and `schema/miserc.json`.
//!
//! `mise run render:schema` runs this, then formats the files.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let settings = std::fs::read_to_string(root.join("settings.toml")).unwrap();
    for (path, schema) in mise_config::schema::render(&settings) {
        let json = serde_json::to_string_pretty(&schema).unwrap();
        std::fs::write(root.join(path), json + "\n").unwrap();
    }
}
