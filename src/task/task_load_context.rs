/// Context for loading tasks with optional filtering hints
#[derive(Debug, Clone, Default, Hash, Eq, PartialEq)]
pub struct TaskLoadContext {
    /// Specific paths to load tasks from
    /// e.g., ["foo/bar", "baz/qux"] from patterns "//foo/bar:task" and "//baz/qux:task"
    pub path_hints: Vec<String>,

    /// If true, load all tasks from the entire monorepo (for `mise tasks ls --all`)
    /// If false (default), only load tasks from current directory hierarchy
    pub load_all: bool,
}

impl TaskLoadContext {
    /// Create a new context that loads all tasks
    pub fn all() -> Self {
        Self {
            path_hints: vec![],
            load_all: true,
        }
    }

    /// Create a context from a task pattern like "//foo/bar:task" or "//foo/bar/..."
    pub fn from_pattern(pattern: &str) -> Self {
        // Extract path hint from pattern
        let path_hints = if let Some(hint) = Self::extract_path_hint(pattern) {
            vec![hint]
        } else {
            vec![]
        };

        Self {
            path_hints,
            load_all: false,
        }
    }

    /// Create a context from multiple patterns, merging their path hints
    pub fn from_patterns<'a>(patterns: impl Iterator<Item = &'a str>) -> Self {
        let mut path_hints = Vec::new();
        let mut load_all = false;

        for pattern in patterns {
            if let Some(hint) = Self::extract_path_hint(pattern) {
                if !path_hints.contains(&hint) {
                    path_hints.push(hint);
                }
            } else {
                // If any pattern has no hint, we need to load all
                load_all = true;
            }
        }

        Self {
            path_hints,
            load_all,
        }
    }

    /// Extract path hint from a monorepo pattern
    /// Returns None if the pattern doesn't provide useful filtering information
    fn extract_path_hint(pattern: &str) -> Option<String> {
        const MONOREPO_PREFIX: &str = "//";
        const TASK_SEPARATOR: &str = ":";
        const ELLIPSIS: &str = "...";

        if !pattern.starts_with(MONOREPO_PREFIX) {
            return None;
        }

        // Remove the // prefix
        let without_prefix = pattern.strip_prefix(MONOREPO_PREFIX)?;

        // Split on : to separate path from task name
        let parts: Vec<&str> = without_prefix.splitn(2, TASK_SEPARATOR).collect();
        let path_part = parts.first()?;

        // If it's just "//..." or "//" we need everything
        if path_part.is_empty() || *path_part == ELLIPSIS {
            return None;
        }

        // Remove trailing ellipsis if present (e.g., "foo/bar/...")
        let path_part = path_part.strip_suffix('/').unwrap_or(path_part);
        let path_part = path_part.strip_suffix(ELLIPSIS).unwrap_or(path_part);
        let path_part = path_part.strip_suffix('/').unwrap_or(path_part);

        // If we have a non-empty path hint, return it
        if !path_part.is_empty() {
            Some(path_part.to_string())
        } else {
            None
        }
    }

    /// Check if a subdirectory should be loaded based on the context
    pub fn should_load_subdir(&self, subdir: &str, _monorepo_root: &str) -> bool {
        // If load_all is true, load everything
        if self.load_all {
            return true;
        }

        // If no path hints, don't load anything (unless load_all is true)
        if self.path_hints.is_empty() {
            return false;
        }

        // Normalize subdir for comparison (remove leading/trailing slashes)
        let subdir = subdir.trim_matches('/');

        // Check if subdir matches or is a parent/child of any hint
        for hint in &self.path_hints {
            let hint = hint.trim_matches('/');

            // Check if subdir matches or is a parent/child of this hint
            // e.g., hint "foo/bar" should match:
            // - "foo/bar" (exact match)
            // - "foo/bar/baz" (child)
            // - "foo" (parent, might contain the target)
            if subdir == hint
                || subdir.starts_with(&format!("{}/", hint))
                || hint.starts_with(&format!("{}/", subdir))
            {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_path_hint() {
        assert_eq!(
            TaskLoadContext::extract_path_hint("//foo/bar:task"),
            Some("foo/bar".to_string())
        );
        assert_eq!(
            TaskLoadContext::extract_path_hint("//foo/bar/...:task"),
            Some("foo/bar".to_string())
        );
        assert_eq!(
            TaskLoadContext::extract_path_hint("//foo:task"),
            Some("foo".to_string())
        );
        assert_eq!(TaskLoadContext::extract_path_hint("//:task"), None);
        assert_eq!(TaskLoadContext::extract_path_hint("//...:task"), None);
        assert_eq!(TaskLoadContext::extract_path_hint("foo:task"), None);
    }

    #[test]
    fn test_should_load_subdir() {
        let ctx = TaskLoadContext::from_pattern("//foo/bar:task");

        // Should load exact match
        assert!(ctx.should_load_subdir("foo/bar", "/root"));

        // Should load children
        assert!(ctx.should_load_subdir("foo/bar/baz", "/root"));

        // Should load parent (might contain target)
        assert!(ctx.should_load_subdir("foo", "/root"));

        // Should not load unrelated paths
        assert!(!ctx.should_load_subdir("baz/qux", "/root"));
    }

    #[test]
    fn test_should_load_subdir_multiple_hints() {
        let ctx =
            TaskLoadContext::from_patterns(["//foo/bar:task", "//baz/qux:task"].iter().map(|s| *s));

        // Should load exact matches for both hints
        assert!(ctx.should_load_subdir("foo/bar", "/root"));
        assert!(ctx.should_load_subdir("baz/qux", "/root"));

        // Should load children of both hints
        assert!(ctx.should_load_subdir("foo/bar/child", "/root"));
        assert!(ctx.should_load_subdir("baz/qux/child", "/root"));

        // Should load parents of both hints
        assert!(ctx.should_load_subdir("foo", "/root"));
        assert!(ctx.should_load_subdir("baz", "/root"));

        // Should not load unrelated paths
        assert!(!ctx.should_load_subdir("other/path", "/root"));
    }

    #[test]
    fn test_load_all_context() {
        let ctx = TaskLoadContext::all();
        assert!(ctx.load_all);
        assert!(ctx.should_load_subdir("any/path", "/root"));
    }
}
