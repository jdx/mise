/// Context for loading tasks with optional filtering hints
#[derive(Debug, Clone, Default, Hash, Eq, PartialEq)]
pub struct TaskLoadContext {
    /// If Some, only load tasks from this specific path
    /// e.g., "foo/bar" from pattern "//foo/bar:task"
    pub path_hint: Option<String>,

    /// If true, load all tasks from the entire monorepo (for `mise tasks ls --all`)
    /// If false (default), only load tasks from current directory hierarchy
    pub load_all: bool,
}

impl TaskLoadContext {
    /// Create a new context that loads all tasks
    pub fn all() -> Self {
        Self {
            path_hint: None,
            load_all: true,
        }
    }

    /// Create a context from a task pattern like "//foo/bar:task" or "//foo/bar/..."
    pub fn from_pattern(pattern: &str) -> Self {
        // Extract path hint from pattern
        let path_hint = Self::extract_path_hint(pattern);

        Self {
            path_hint,
            load_all: false,
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

        // If no path hint, don't load anything (unless load_all is true)
        let Some(ref hint) = self.path_hint else {
            return false;
        };

        // Normalize paths for comparison (remove leading/trailing slashes)
        let hint = hint.trim_matches('/');
        let subdir = subdir.trim_matches('/');

        // Check if subdir matches or is a parent/child of the hint
        // e.g., hint "foo/bar" should match:
        // - "foo/bar" (exact match)
        // - "foo/bar/baz" (child)
        // - "foo" (parent, might contain the target)

        subdir == hint
            || subdir.starts_with(&format!("{}/", hint))
            || hint.starts_with(&format!("{}/", subdir))
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
    fn test_load_all_context() {
        let ctx = TaskLoadContext::all();
        assert!(ctx.load_all);
        assert!(ctx.should_load_subdir("any/path", "/root"));
    }
}
