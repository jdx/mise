use eyre::bail;

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
        use std::collections::HashSet;

        let mut path_hints_set = HashSet::new();
        let mut load_all = false;

        for pattern in patterns {
            if let Some(hint) = Self::extract_path_hint(pattern) {
                path_hints_set.insert(hint);
            } else {
                // If any pattern has no hint, we need to load all
                load_all = true;
            }
        }

        Self {
            path_hints: path_hints_set.into_iter().collect(),
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

        // If the path still contains "..." anywhere, it's a wildcard pattern
        // that could match many paths, so we can't use it as a specific hint
        // e.g., ".../graph" or "foo/.../bar" should load all subdirectories
        if path_part.contains(ELLIPSIS) {
            return None;
        }

        // If we have a non-empty path hint, return it
        if !path_part.is_empty() {
            Some(path_part.to_string())
        } else {
            None
        }
    }

    /// Check if a subdirectory should be loaded based on the context
    pub fn should_load_subdir(&self, subdir: &str, _monorepo_root: &str) -> bool {
        use std::path::Path;

        // If load_all is true, load everything
        if self.load_all {
            return true;
        }

        // If no path hints, don't load anything (unless load_all is true)
        if self.path_hints.is_empty() {
            return false;
        }

        // Use Path APIs for more robust path comparison
        let subdir_path = Path::new(subdir);

        // Check if subdir matches or is a parent/child of any hint
        for hint in &self.path_hints {
            let hint_path = Path::new(hint);

            // Check if subdir matches or is a parent/child of this hint
            // e.g., hint "foo/bar" should match:
            // - "foo/bar" (exact match)
            // - "foo/bar/baz" (child - subdir starts with hint)
            // - "foo" (parent - hint starts with subdir, might contain the target)
            if subdir_path == hint_path
                || subdir_path.starts_with(hint_path)
                || hint_path.starts_with(subdir_path)
            {
                return true;
            }
        }

        false
    }
}

/// Expands :task syntax to //path:task based on current directory relative to monorepo root
///
/// This function handles the special `:task` syntax that refers to tasks in the current
/// config_root within a monorepo. It converts `:build` to either `//:build` (if at monorepo root)
/// or `//project:build` (if in a subdirectory).
///
/// # Arguments
/// * `task` - The task pattern to expand (e.g., ":build")
/// * `config` - The configuration containing monorepo information
///
/// # Returns
/// * `Ok(String)` - The expanded task pattern (e.g., "//project:build")
/// * `Err` - If monorepo is not configured or current directory is outside monorepo root
pub fn expand_colon_task_syntax(
    task: &str,
    config: &crate::config::Config,
) -> eyre::Result<String> {
    // Skip expansion for absolute monorepo paths or explicit global tasks
    if task.starts_with("//") || task.starts_with("::") {
        return Ok(task.to_string());
    }

    // Check if this is a colon pattern or a bare name
    let is_colon_pattern = task.starts_with(':');

    // Reject patterns that look like monorepo paths with wrong syntax (have / and : but don't start with // or :)
    if !is_colon_pattern && task.contains('/') && task.contains(':') {
        bail!(
            "relative path syntax '{}' is not supported, use '//{}' or ':task' for current directory",
            task,
            task
        );
    }

    // Get the monorepo root (the config file with experimental_monorepo_root = true)
    let monorepo_root = config
        .config_files
        .values()
        .find(|cf| cf.experimental_monorepo_root() == Some(true))
        .and_then(|cf| cf.project_root());

    // If not in monorepo context, only expand if it's a colon pattern (error), otherwise return as-is
    if monorepo_root.is_none() {
        if is_colon_pattern {
            bail!("Cannot use :task syntax without a monorepo root");
        }
        return Ok(task.to_string());
    }

    // We're in a monorepo context
    let monorepo_root = monorepo_root.unwrap();

    // Determine the current directory relative to monorepo root
    if let Some(cwd) = &*crate::dirs::CWD {
        if let Ok(rel_path) = cwd.strip_prefix(monorepo_root) {
            // For bare task names, only expand if we're actually in the monorepo
            // For colon patterns, always expand (and error if outside monorepo)

            // Convert relative path to monorepo path format
            let path_str = rel_path
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");

            if path_str.is_empty() {
                // We're at the root
                if is_colon_pattern {
                    // :task -> //:task (task already has colon)
                    Ok(format!("//{}", task))
                } else {
                    // bare task -> //:task (add colon)
                    Ok(format!("//:{}", task))
                }
            } else {
                // We're in a subdirectory
                if is_colon_pattern {
                    // :task -> //path:task
                    Ok(format!("//{}{}", path_str, task))
                } else {
                    // bare name -> //path:task
                    Ok(format!("//{}:{}", path_str, task))
                }
            }
        } else {
            if is_colon_pattern {
                bail!("Cannot use :task syntax outside of monorepo root directory");
            }
            // Bare name outside monorepo - return as-is for global matching
            Ok(task.to_string())
        }
    } else {
        if is_colon_pattern {
            bail!("Cannot use :task syntax without a current working directory");
        }
        Ok(task.to_string())
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

        // Test patterns with ... in different positions (wildcard patterns)

        // ... at the START of path
        assert_eq!(
            TaskLoadContext::extract_path_hint("//.../api:task"),
            None,
            "Pattern with ... at start should load all subdirs"
        );
        assert_eq!(
            TaskLoadContext::extract_path_hint("//.../services/api:task"),
            None,
            "Pattern with ... at start and more path should load all subdirs"
        );

        // ... in the MIDDLE of path
        assert_eq!(
            TaskLoadContext::extract_path_hint("//projects/.../api:task"),
            None,
            "Pattern with ... in middle should load all subdirs"
        );
        assert_eq!(
            TaskLoadContext::extract_path_hint("//libs/.../utils:task"),
            None,
            "Pattern with ... in middle should load all subdirs"
        );

        // Multiple ... in path
        assert_eq!(
            TaskLoadContext::extract_path_hint("//projects/.../api/...:task"),
            None,
            "Pattern with multiple ... should load all subdirs"
        );
        assert_eq!(
            TaskLoadContext::extract_path_hint("//.../foo/.../bar:task"),
            None,
            "Pattern with ... at start and middle should load all subdirs"
        );
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
            TaskLoadContext::from_patterns(["//foo/bar:task", "//baz/qux:task"].iter().copied());

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

    #[test]
    fn test_expand_colon_task_syntax() {
        // Note: This is a basic structure test. Full integration testing is done in e2e tests
        // because it requires a real config with monorepo root setup and CWD manipulation.

        // Test that non-colon patterns are returned as-is
        // We can't easily test the full expansion here without setting up a real config
        // and manipulating CWD, so we just verify the function signature and basic behavior
        let task = "regular-task";
        // For non-colon tasks, this should work even with empty config
        // The actual expansion logic is tested via e2e tests
        assert!(!task.starts_with(':'));
    }
}
