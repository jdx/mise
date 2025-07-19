# Task Dependencies with Environment Variables

## Overview

This feature adds support for specifying environment variables when declaring task dependencies in mise. This allows you to run the same task multiple times with different environment variable values as dependencies.

## Syntax

### Object Syntax (New)

The preferred syntax uses objects to specify task dependencies with environment variables:

```toml
[tasks.test]
run = "test.sh"

[tasks.test-all]
depends = [
  {task = "test", env = {PROP = "a"}},
  {task = "test", env = {PROP = "b"}},
]
```

You can also combine args and env:

```toml
[tasks.test]
run = "test.sh"

[tasks.test-all]
depends = [
  {task = "test", args = ["arg1"], env = {PROP = "a"}},
  {task = "test", args = ["arg2"], env = {PROP = "b"}},
]
```

### Mixed Syntax

You can mix object and string syntax in the same depends array:

```toml
[tasks.test]
run = "test.sh"

[tasks.build]
run = "build.sh"

[tasks.test-all]
depends = [
  "build",  # string syntax
  {task = "test", env = {PROP = "a"}},  # object syntax
]
```

## Implementation Details

### Changes Made

1. **Extended TaskDep struct** (`src/task/task_dep.rs`):
   - Added `env: BTreeMap<String, String>` field
   - Updated `render()` method to apply tera rendering to environment variables

2. **Enhanced deserialization**:
   - Added custom `Deserialize` implementation to handle three formats:
     - String: `"task"`
     - Array: `["task", "arg1", "arg2"]`
     - Object: `{task = "task", args = ["arg1"], env = {VAR = "value"}}`

3. **Updated serialization**:
   - Modified `Serialize` implementation to output object format when env vars are present
   - Maintains backward compatibility for string/array formats when no env vars

4. **Enhanced task matching** (`src/task/mod.rs`):
   - Updated `match_tasks()` function to apply environment variables to matched tasks
   - Environment variables are merged into the task's env map

5. **Improved display**:
   - Updated `Display` implementation to show environment variables in human-readable format

### Test Coverage

- Unit tests for all three deserialization formats
- E2E test scenarios covering common use cases
- Tests for mixed syntax and args+env combinations

### Backward Compatibility

The implementation maintains full backward compatibility with existing task dependency formats:
- String dependencies: `depends = ["task1", "task2"]`
- Array dependencies: `depends = [["task", "arg1", "arg2"]]`

## Example Use Cases

### Running Tests with Different Environments

```toml
[tasks.test]
run = "npm test"

[tasks.test-all-envs]
depends = [
  {task = "test", env = {NODE_ENV = "development"}},
  {task = "test", env = {NODE_ENV = "production"}},
  {task = "test", env = {NODE_ENV = "staging"}},
]
```

### Building for Multiple Platforms

```toml
[tasks.build]
run = "cargo build"

[tasks.build-all]
depends = [
  {task = "build", env = {TARGET = "x86_64-unknown-linux-gnu"}},
  {task = "build", env = {TARGET = "aarch64-apple-darwin"}},
  {task = "build", env = {TARGET = "x86_64-pc-windows-msvc"}},
]
```

### Database Migrations for Different Environments

```toml
[tasks.migrate]
run = "db-migrate"

[tasks.migrate-all]
depends = [
  {task = "migrate", env = {DATABASE_URL = "postgres://localhost/dev"}},
  {task = "migrate", env = {DATABASE_URL = "postgres://localhost/test"}},
]
```

## Benefits

1. **Clean and Explicit**: The object syntax makes it clear what environment variables are being set
2. **Type Safe**: Environment variables are properly typed as strings in the TOML schema
3. **Flexible**: Can combine with existing args and mix with string dependencies
4. **Maintainable**: Easier to understand and modify compared to string parsing approaches
5. **Backward Compatible**: Existing configurations continue to work unchanged

## Alternative Syntax Considered

An alternate string-based syntax was considered:
```toml
depends = ["PROP=a test", "PROP=b test"]
```

This was rejected because:
- It requires string parsing which is error-prone
- It's less obvious what the syntax means
- Order matters (env must come before task name) which is unintuitive
- It would be harder to extend with additional features

## Files Modified

### Core Implementation
1. **`src/task/task_dep.rs`** - Extended TaskDep struct with env field and updated serialization/deserialization
2. **`src/task/mod.rs`** - Updated match_tasks function to apply environment variables to tasks

### Tests
3. **`e2e/tasks/test_task_depends_env`** - New e2e test file demonstrating the feature

### Documentation  
4. **`task_dependencies_with_environment_variables.md`** - This documentation file

## Summary

The implementation successfully adds support for environment variables in task dependencies using the preferred object syntax:

```toml
[tasks.test-all]
depends = [
  {task = "test", env = {PROP = "a"}},
  {task = "test", env = {PROP = "b"}},
]
```

The feature is:
- **Backward compatible** - existing string and array syntax continues to work
- **Type-safe** - uses proper TOML objects instead of string parsing  
- **Flexible** - supports mixing with args and other dependency formats
- **Well-tested** - includes both unit and e2e tests
- **Clean** - avoids complex string parsing in favor of structured data
