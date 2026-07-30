use std::borrow::Cow;
use std::collections::HashSet;
use std::hash::Hash;

pub trait TreeItem: Clone {
    type Child: TreeItem;

    fn write_self(&self) -> std::io::Result<()>;

    fn children(&self) -> Cow<'_, [Self::Child]>;
}

struct TreeItemIndentChars {
    /// Character for pointing down and right (`├`).
    pub down_and_right: &'static str,
    /// Character for pointing straight down (`|`).
    pub down: &'static str,
    /// Character for turning from down to right (`└`).
    pub turn_right: &'static str,
    /// Character for pointing right (`─`).
    pub right: &'static str,
    /// Empty character (` `).
    pub empty: &'static str,
}

const TREE_ITEM_CHARS: TreeItemIndentChars = TreeItemIndentChars {
    down_and_right: "├",
    down: "│",
    turn_right: "└",
    right: "─",
    empty: " ",
};

struct TreeItemIndent {
    pub regular_prefix: String,
    pub child_prefix: String,
    pub last_regular_prefix: String,
    pub last_child_prefix: String,
}

impl TreeItemIndent {
    pub fn new(
        indent_size: usize,
        padding: usize,
        characters: &TreeItemIndentChars,
    ) -> TreeItemIndent {
        let m = 1 + padding;
        let n = indent_size.saturating_sub(m);

        let right_pad = characters.right.repeat(n);
        let empty_pad = characters.empty.repeat(n);
        let item_pad = characters.empty.repeat(padding);

        TreeItemIndent {
            regular_prefix: format!("{}{}{}", characters.down_and_right, right_pad, item_pad),
            child_prefix: format!("{}{}{}", characters.down, empty_pad, item_pad),
            last_regular_prefix: format!("{}{}{}", characters.turn_right, right_pad, item_pad),
            last_child_prefix: format!("{}{}{}", characters.empty, empty_pad, item_pad),
        }
    }
}

pub fn print_tree<T: TreeItem>(item: &T) -> std::io::Result<()> {
    let indent = TreeItemIndent::new(4, 1, &TREE_ITEM_CHARS);
    print_tree_item(item, String::from(""), String::from(""), &indent, 0)
}

pub fn print_tree_compact<T, K, F>(item: &T, key: F, seen: &mut HashSet<K>) -> std::io::Result<()>
where
    T: TreeItem<Child = T>,
    K: Eq + Hash,
    F: Fn(&T) -> K,
{
    let indent = TreeItemIndent::new(4, 1, &TREE_ITEM_CHARS);
    print_tree_item_compact(
        item,
        String::from(""),
        String::from(""),
        &indent,
        0,
        &key,
        seen,
    )
}

fn print_tree_item<T: TreeItem>(
    item: &T,
    prefix: String,
    child_prefix: String,
    indent: &TreeItemIndent,
    level: u32,
) -> std::io::Result<()> {
    miseprint!("{}", prefix)?;
    item.write_self()?;
    miseprintln!("");

    if level < u32::MAX {
        let children = item.children();
        if let Some((last_child, children)) = children.split_last() {
            let rp = child_prefix.clone() + &indent.regular_prefix;
            let cp = child_prefix.clone() + &indent.child_prefix;

            for c in children {
                print_tree_item(c, rp.clone(), cp.clone(), indent, level + 1)?;
            }

            let rp = child_prefix.clone() + &indent.last_regular_prefix;
            let cp = child_prefix.clone() + &indent.last_child_prefix;

            print_tree_item(last_child, rp, cp, indent, level + 1)?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_tree_item_compact<T, K, F>(
    item: &T,
    prefix: String,
    child_prefix: String,
    indent: &TreeItemIndent,
    level: u32,
    key: &F,
    seen: &mut HashSet<K>,
) -> std::io::Result<()>
where
    T: TreeItem<Child = T>,
    K: Eq + Hash,
    F: Fn(&T) -> K,
{
    miseprint!("{}", prefix)?;
    item.write_self()?;

    if !seen.insert(key(item)) {
        miseprintln!(" (already shown)");
        return Ok(());
    }
    miseprintln!("");

    if level < u32::MAX {
        let children = item.children();
        if let Some((last_child, children)) = children.split_last() {
            let rp = child_prefix.clone() + &indent.regular_prefix;
            let cp = child_prefix.clone() + &indent.child_prefix;

            for child in children {
                print_tree_item_compact(
                    child,
                    rp.clone(),
                    cp.clone(),
                    indent,
                    level + 1,
                    key,
                    seen,
                )?;
            }

            let rp = child_prefix.clone() + &indent.last_regular_prefix;
            let cp = child_prefix.clone() + &indent.last_child_prefix;

            print_tree_item_compact(last_child, rp, cp, indent, level + 1, key, seen)?;
        }
    }

    Ok(())
}
