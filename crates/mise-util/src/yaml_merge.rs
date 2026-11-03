//! Merge one YAML mapping into another without rewriting the parts of the
//! target the merge does not touch.
//!
//! Applications that own a YAML config file re-serialize it on their own
//! schedule, so a tool that only wants to set a few keys has to edit the text
//! in place: comments, key order, quoting and flow/block style on every other
//! key survive. [`merge_yaml`] does that on top of `yaml-edit`'s lossless
//! syntax tree.

use crate::yaml;
use eyre::{Result, bail, eyre};
use std::str::FromStr;
use yaml_edit::{Mapping, YamlFile, YamlNode};

/// Set every key in `source` on `target` and return the edited text.
///
/// - Keys that exist only in `target` are left alone, in their original place.
/// - A mapping that exists on both sides is merged recursively.
/// - Anything else, including sequences, is replaced by the source's value.
///   A value that already equals the source's is not rewritten, so its
///   formatting and comments stay.
///
/// An empty `target` (or one that holds only comments) is treated as an empty
/// mapping. Both roots must otherwise be a single mapping.
pub fn merge_yaml(target: &str, source: &str) -> Result<String> {
    let source_file =
        YamlFile::from_str(source).map_err(|e| eyre!("failed to parse the YAML source: {e}"))?;
    let source_map = root_mapping(&source_file, "source", false)?;
    let target_file =
        YamlFile::from_str(target).map_err(|e| eyre!("failed to parse the YAML target: {e}"))?;
    let target_map = root_mapping(&target_file, "target", true)?;
    let (Some(source_map), Some(target_map)) = (source_map, target_map) else {
        bail!("the YAML source and target must each hold a mapping at the top level");
    };
    merge_mapping(&target_map, &source_map);
    let merged = target_file.to_string();
    verify(target, &merged, source)?;
    Ok(merged)
}

/// Check the edited text parses to exactly the target with the source merged
/// in. The edits are surgical, so this guards against a syntax-tree edit that
/// corrupts the document instead of trusting it. It also catches edits whose
/// meaning reaches past their text: replacing an alias such as `kid: *b`
/// drops the values it pointed at, and editing an anchored value changes every
/// alias of it.
fn verify(target: &str, merged: &str, source: &str) -> Result<()> {
    let parse = |text: &str| match yaml::from_str::<yaml::Value>(text) {
        Ok(yaml::Value::Mapping(map)) => Some(map),
        Ok(yaml::Value::Null) => Some(yaml::Mapping::new()),
        _ => None,
    };
    let (Some(mut expected), Some(source), Some(merged)) =
        (parse(target), parse(source), parse(merged))
    else {
        bail!("editing the YAML in place did not produce a valid document");
    };
    merge_values(&mut expected, source);
    if merged != expected {
        bail!(
            "editing the YAML in place would change values the merge does not set, \
             for example through an anchor and its aliases; edit this file by hand"
        );
    }
    Ok(())
}

/// The merge's semantics on parsed values: mappings on both sides merge
/// recursively, anything else from `source` replaces the target's value.
fn merge_values(target: &mut yaml::Mapping, source: yaml::Mapping) {
    for (key, sv) in source {
        match (target.get_mut(&key), sv) {
            (Some(yaml::Value::Mapping(t)), yaml::Value::Mapping(s)) => merge_values(t, s),
            (_, sv) => {
                target.insert(key, sv);
            }
        }
    }
}

/// Whether every key of `source` is set to the same value in `target`, tables
/// compared recursively.
pub(crate) fn mapping_subset(source: &yaml::Mapping, target: &yaml::Mapping) -> bool {
    source.iter().all(|(key, sv)| match (sv, target.get(key)) {
        (yaml::Value::Mapping(s), Some(yaml::Value::Mapping(t))) => mapping_subset(s, t),
        (sv, Some(tv)) => sv == tv,
        (_, None) => false,
    })
}

/// The mapping at the root of a file's only document. An empty target gets a
/// document created so keys have somewhere to go.
pub(crate) fn root_mapping(file: &YamlFile, what: &str, create: bool) -> Result<Option<Mapping>> {
    if file.documents().count() > 1 {
        bail!("the YAML {what} holds several documents; only one is supported");
    }
    let document = match file.document() {
        Some(document) => document,
        None if create => file.ensure_document(),
        None => return Ok(None),
    };
    Ok(document.as_mapping())
}

fn merge_mapping(target: &Mapping, source: &Mapping) {
    for (key, value) in source.iter() {
        match (target.get(&key), value.as_mapping()) {
            (Some(existing), Some(source_child)) => match existing.as_mapping() {
                Some(target_child) => merge_mapping(target_child, source_child),
                None => replace_value(target, &key, &existing, &value),
            },
            (Some(existing), None) if existing.yaml_eq(&value) => {}
            (Some(existing), None) => replace_value(target, &key, &existing, &value),
            (None, _) => target.set(&key, &value),
        }
    }
}

/// Replace the value of an existing key. `yaml-edit` loses the line break
/// after a block scalar (`|`, `>`) when it replaces one in place and glues the
/// next key onto the same line, so such an entry is removed and re-inserted at
/// the same position instead.
fn replace_value(target: &Mapping, key: &YamlNode, existing: &YamlNode, value: &YamlNode) {
    let existing_text = existing.to_string();
    if existing_text.trim_start().starts_with(['|', '>'])
        && let Some(index) = target.keys().position(|k| k.yaml_eq(key))
    {
        target.remove(key);
        target.insert_at_index(index, key, value);
        return;
    }
    target.set(key, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn edits_that_change_aliased_values_are_refused() {
        // replacing the alias would drop kid.x
        assert!(merge_yaml("base: &b {x: 1}\nkid: *b\n", "kid: {y: 2}\n").is_err());
        // editing the anchor would also change kid
        assert!(merge_yaml("base: &b {x: 1}\nkid: *b\n", "base: {x: 2}\n").is_err());
        // an alias the merge does not reach is fine
        assert_eq!(
            merge_yaml("base: &b {x: 1}\nkid: *b\nother: 1\n", "other: 2\n").unwrap(),
            "base: &b {x: 1}\nkid: *b\nother: 2\n"
        );
    }

    #[test]
    fn sets_keys_and_keeps_everything_else() {
        let target = "\
# managed by the app
settings:
  theme: dark   # picked in the UI
  app_written: 1
# trailing note
other: x
";
        let source = "settings:\n  theme: light\n  font: mono\nname: demo\n";
        assert_eq!(
            merge_yaml(target, source).unwrap(),
            "\
# managed by the app
settings:
  theme: light   # picked in the UI
  app_written: 1
  font: mono
# trailing note
other: x
name: demo
"
        );
    }

    #[test]
    fn an_equal_value_is_not_rewritten() {
        let target = "list: [1,   2]   # keep my spacing\nname: 'demo'\n";
        let source = "list: [1, 2]\nname: demo\n";
        assert_eq!(merge_yaml(target, source).unwrap(), target);
    }

    #[test]
    fn sequences_are_replaced() {
        let merged = merge_yaml("list: [9]\nkeep: 1\n", "list:\n  - a\n  - b\n").unwrap();
        assert_eq!(merged, "list:\n  - a\n  - b\nkeep: 1\n");
    }

    #[test]
    fn a_new_nested_mapping_is_created() {
        let merged = merge_yaml("a: 1\n", "b:\n  c:\n    d: 2\n").unwrap();
        assert_eq!(merged, "a: 1\nb:\n  c:\n    d: 2\n");
    }

    #[test]
    fn a_scalar_is_replaced_by_a_mapping_and_back() {
        let merged = merge_yaml("a: 1\nb:\n  c: 2\n", "a:\n  x: 1\nb: 3\n").unwrap();
        assert_eq!(merged, "a:\n  x: 1\nb: 3\n");
    }

    #[test]
    fn an_empty_or_comment_only_target_is_an_empty_mapping() {
        assert_eq!(merge_yaml("", "a: 1\n").unwrap(), "a: 1\n");
        assert_eq!(
            merge_yaml("# nothing yet\n", "a: 1\n").unwrap(),
            "# nothing yet\na: 1\n"
        );
    }

    #[test]
    fn a_flow_mapping_keeps_its_style_and_spacing() {
        let merged = merge_yaml("m: {a: 1,   b: 2}   # flow\nk: 1\n", "m:\n  b: 3\n").unwrap();
        assert_eq!(merged, "m: {a: 1,   b: 3}   # flow\nk: 1\n");
        let merged = merge_yaml("m: {a: 1}\n", "m:\n  c: 9\n").unwrap();
        assert_eq!(merged, "m: {a: 1, c: 9}\n");
    }

    #[test]
    fn a_block_scalar_is_kept_or_replaced_cleanly() {
        // untouched
        let merged = merge_yaml("text: |\n  keep this\n  block\nk: old\n", "k: new\n").unwrap();
        assert_eq!(merged, "text: |\n  keep this\n  block\nk: new\n");
        // replaced: the next key must stay on its own line, in place
        let merged = merge_yaml("text: |\n  old\nk: 1\n", "text: new\n").unwrap();
        assert_eq!(merged, "text: new\nk: 1\n");
        let merged = merge_yaml("a: 1\ntext: >\n  old\n  fold\nz: 2\n", "text: [x]\n").unwrap();
        assert_eq!(merged, "a: 1\ntext: [x]\nz: 2\n");
    }

    #[test]
    fn nested_inserts_follow_the_existing_indentation() {
        let merged = merge_yaml(
            "top:\n    inner:\n        a: 1\n",
            "top:\n  inner:\n    b: 2\n",
        )
        .unwrap();
        assert_eq!(merged, "top:\n    inner:\n        a: 1\n        b: 2\n");
        // a whole new sub-mapping still parses, with the right values
        let merged = merge_yaml("top:\n    a: 1\n", "top:\n  new:\n    deep: 2\n").unwrap();
        let value: yaml::Value = yaml::from_str(&merged).unwrap();
        let top = value.get("top").unwrap();
        assert_eq!(top.get("a"), Some(&yaml::Value::from(1)));
        assert_eq!(
            top.get("new").and_then(|v| v.get("deep")),
            Some(&yaml::Value::from(2))
        );
        assert!(merged.starts_with("top:\n    a: 1\n"));
    }

    #[test]
    fn merging_is_idempotent() {
        let source = "settings:\n  theme: light\nlist: [1, 2]\n";
        let once = merge_yaml("# c\nsettings:\n  x: 1\n", source).unwrap();
        assert_eq!(merge_yaml(&once, source).unwrap(), once);
    }

    #[test]
    fn unusable_roots_are_errors() {
        assert!(merge_yaml("- a\n- b\n", "a: 1\n").is_err());
        assert!(merge_yaml("a: 1\n", "- a\n").is_err());
        assert!(merge_yaml("a: 1\n---\nb: 2\n", "a: 1\n").is_err());
        assert!(merge_yaml("a: [1\n", "a: 1\n").is_err());
    }
}
