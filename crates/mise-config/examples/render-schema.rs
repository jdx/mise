//! Prints the generated schema fragments as one JSON object, keyed by JSON pointer.
//!
//! `mise run render:schema` runs this and writes each fragment into `schema/mise.json`.

fn main() {
    let fragments: serde_json::Map<String, serde_json::Value> = mise_config::schema::fragments()
        .into_iter()
        .map(|(pointer, schema)| (pointer.to_string(), schema))
        .collect();
    println!("{}", serde_json::to_string_pretty(&fragments).unwrap());
}
