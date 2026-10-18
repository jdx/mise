//! The JSON schema for the `mise.toml` sections whose types live in this crate.
//!
//! `schema/mise.json` is still mostly written by hand. `mise run render:schema` replaces each
//! fragment listed here at its JSON pointer, the way it already fills in `$defs.settings` from
//! `settings.toml`. A section moves out of the hand-written part when its types gain a
//! `JsonSchema` derive here, and the test below fails while the committed file is stale.

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::Value;

use crate::history::HistoryTomlConfig;

/// Each generated fragment, keyed by the JSON pointer it replaces in `schema/mise.json`.
pub fn fragments() -> Vec<(&'static str, Value)> {
    let mut generator = generator();
    vec![(
        "/properties/history",
        fragment::<HistoryTomlConfig>(&mut generator),
    )]
}

fn generator() -> SchemaGenerator {
    SchemaSettings::draft2020_12()
        .with(|settings| {
            // Every fragment is written in place, so a nested type cannot point at a `$defs`
            // entry that the hand-written file does not have.
            settings.inline_subschemas = true;
        })
        .with_transform(RecursiveTransform(drop_null))
        .with_transform(RecursiveTransform(reject_unknown_keys))
        .with_transform(RecursiveTransform(describe_first))
        .into_generator()
}

fn fragment<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    let mut schema = generator.root_schema_for::<T>();
    let object = schema.ensure_object();
    // The root-only keys belong to the whole file, not to one of its sections.
    object.remove("$schema");
    object.remove("title");
    schema.to_value()
}

/// TOML has no null, so an `Option` field is optional but never nullable, and it has no
/// default to show.
fn drop_null(schema: &mut Schema) {
    if schema.get("default") == Some(&Value::Null) {
        schema.remove("default");
    }
    let Some(Value::Array(types)) = schema.get_mut("type") else {
        return;
    };
    types.retain(|t| t != "null");
    if let [only] = types.as_slice() {
        let only = only.clone();
        schema.insert("type".into(), only);
    }
}

/// mise warns about an unknown key, but the schema rejects it, so an editor flags the typo.
///
/// A type that accepts open keys says so with an explicit `additionalProperties`, which this
/// leaves alone.
fn reject_unknown_keys(schema: &mut Schema) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    if object.contains_key("properties")
        && !object.contains_key("additionalProperties")
        && !object.contains_key("unevaluatedProperties")
    {
        object.insert("additionalProperties".into(), false.into());
    }
}

/// Puts `type` and `description` first, the way the hand-written sections read.
fn describe_first(schema: &mut Schema) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let mut rest = std::mem::take(object);
    for key in ["type", "description"] {
        if let Some(value) = rest.shift_remove(key) {
            object.insert(key.into(), value);
        }
    }
    object.extend(rest);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_json_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/mise.json");
        let committed: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for (pointer, generated) in fragments() {
            assert_eq!(
                committed.pointer(pointer),
                Some(&generated),
                "schema/mise.json{pointer} differs from the mise-config types; run `mise run render:schema`"
            );
        }
    }
}
