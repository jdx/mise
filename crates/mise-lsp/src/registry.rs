//! The tools mise's registry knows, as the host listed them: names to complete under `[tools]`
//! and descriptions to show on hover. The server does not look anything up; what the host passes
//! in `Options` is all there is.

use std::collections::HashMap;

use crate::RegistryEntry;

#[derive(Default)]
pub(crate) struct Registry {
    entries: Vec<RegistryEntry>,
    by_name: HashMap<String, usize>,
}

impl Registry {
    pub(crate) fn new(entries: Vec<RegistryEntry>) -> Self {
        let mut by_name = HashMap::new();
        for (i, entry) in entries.iter().enumerate() {
            for name in std::iter::once(&entry.name).chain(&entry.aliases) {
                by_name.entry(name.clone()).or_insert(i);
            }
        }
        Self { entries, by_name }
    }

    /// The tool that goes by `name`, as its own name or an alias.
    pub(crate) fn get(&self, name: &str) -> Option<&RegistryEntry> {
        self.by_name.get(name).map(|i| &self.entries[*i])
    }

    /// The tools to offer, without the ones that are deprecated.
    pub(crate) fn offered(&self) -> impl Iterator<Item = &RegistryEntry> {
        self.entries.iter().filter(|e| e.deprecated.is_none())
    }

    /// Whether the table at `path` is keyed by tool name: `[tools]`, or the `tools` of a task or
    /// a task template.
    pub(crate) fn is_tools_table(path: &[String]) -> bool {
        match path {
            [tools] => tools == "tools",
            [root, _, tools] => (root == "tasks" || root == "task_templates") && tools == "tools",
            _ => false,
        }
    }

    /// Whether the key at `path` is the name of a tool.
    pub(crate) fn is_tool_key(path: &[String]) -> bool {
        path.split_last()
            .is_some_and(|(_, table)| Self::is_tools_table(table))
    }
}

/// What hover says about a tool.
pub(crate) fn describe(entry: &RegistryEntry) -> String {
    let mut markdown = format!("**{}** _tool_", entry.name);
    if let Some(message) = &entry.deprecated {
        markdown.push_str("\n\n**Deprecated.**");
        if !message.is_empty() {
            markdown.push(' ');
            markdown.push_str(message);
        }
    }
    if let Some(description) = &entry.description {
        markdown.push_str("\n\n");
        markdown.push_str(description);
    }
    if !entry.backends.is_empty() {
        let backends: Vec<String> = entry.backends.iter().map(|b| format!("`{b}`")).collect();
        markdown.push_str(&format!("\n\nBackends: {}", backends.join(", ")));
    }
    markdown
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Registry {
        Registry::new(sample_entries())
    }

    pub(crate) fn sample_entries() -> Vec<RegistryEntry> {
        vec![
            RegistryEntry {
                name: "node".to_string(),
                description: Some("Node.js runtime\nmore".to_string()),
                aliases: vec!["nodejs".to_string()],
                backends: vec!["core:node".to_string()],
                deprecated: None,
            },
            RegistryEntry {
                name: "oldtool".to_string(),
                description: None,
                aliases: vec![],
                backends: vec![],
                deprecated: Some("use newtool".to_string()),
            },
        ]
    }

    #[test]
    fn tools_are_found_by_name_or_alias() {
        let registry = sample();
        assert_eq!(registry.get("nodejs").unwrap().name, "node");
        assert!(registry.get("deno").is_none());
        assert_eq!(registry.offered().count(), 1);
    }

    #[test]
    fn tool_tables_are_tools_and_task_tools() {
        let path = |s: &[&str]| s.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        assert!(Registry::is_tools_table(&path(&["tools"])));
        assert!(Registry::is_tools_table(&path(&[
            "tasks", "build", "tools"
        ])));
        assert!(!Registry::is_tools_table(&path(&["settings"])));
        assert!(Registry::is_tool_key(&path(&["tools", "node"])));
        assert!(!Registry::is_tool_key(&path(&["tools", "node", "version"])));
    }

    #[test]
    fn hover_says_what_the_tool_is() {
        let registry = sample();
        let text = describe(registry.get("node").unwrap());
        assert!(text.starts_with("**node** _tool_"));
        assert!(text.contains("Node.js runtime"));
        assert!(text.contains("Backends: `core:node`"));
        assert!(describe(registry.get("oldtool").unwrap()).contains("**Deprecated.** use newtool"));
    }
}
