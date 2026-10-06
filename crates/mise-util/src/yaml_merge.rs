//! Merge one YAML mapping into another without rewriting the parts of the
//! target the merge does not touch.
//!
//! Applications that own a YAML config file re-serialize it on their own
//! schedule, so a tool that only wants to set a few keys has to edit the text
//! in place: comments, key order, quoting and flow/block style on every other
//! key survive. [`merge_yaml`] does that on top of `yaml-edit`'s lossless
//! syntax tree.

use eyre::{Result, bail, eyre};
use std::str::FromStr;
use yaml_edit::{Mapping, YamlFile};

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
    Ok(target_file.to_string())
}

/// The mapping at the root of a file's only document. An empty target gets a
/// document created so keys have somewhere to go.
fn root_mapping(file: &YamlFile, what: &str, create: bool) -> Result<Option<Mapping>> {
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
                None => target.set(&key, &value),
            },
            (Some(existing), None) if existing.yaml_eq(&value) => {}
            _ => target.set(&key, &value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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
