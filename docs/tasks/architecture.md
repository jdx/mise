# Task System Architecture

Understanding how mise's task system works helps you write more efficient tasks and troubleshoot dependency issues.

## Task Dependency System

mise uses a sophisticated dependency graph system to manage task execution order and parallelism. This ensures tasks run in the correct order while maximizing performance through parallel execution.

### Dependency Graph Resolution

When you run `mise run build`, mise creates a directed acyclic graph (DAG) of all tasks and their dependencies:

```mermaid
graph TD
    A[lint] --> D[test]
    B[format] --> D[test]
    C[build] --> D[test]
    D[test] --> E[package]
    F[docs] --> E[package]
    E[package] --> G[deploy]
```

This graph ensures that:

- Dependencies run before dependents
- Independent tasks run in parallel
- No circular dependencies exist
- Failed dependencies prevent dependents from running

### Dependency Types

mise supports three types of task dependencies:

#### `depends` - Prerequisites

Tasks that must complete successfully before this task runs:

```toml
[tasks.test]
depends = ["lint", "build"]
run = "npm test"
```

#### `depends_post` - Cleanup Tasks

Tasks that run after this task completes (whether successful or failed):

```toml
[tasks.deploy]
depends = ["build", "test"]
depends_post = ["cleanup", "notify"]
run = "kubectl apply -f deployment.yaml"
```

#### `wait_for` - Soft Dependencies

Tasks that should run first if they're in the current execution, but don't fail if they're not available:

```toml
[tasks.integration-test]
wait_for = ["start-services"]  # Only waits if start-services is also being run
run = "npm run test:integration"
```

## Parallel Execution Engine

### Job Control

mise executes tasks in parallel up to the configured job limit:

```bash
mise run --jobs 8 test        # Use 8 parallel jobs
mise run -j 1 test            # Force sequential execution
```

The default is 4 parallel jobs, but you can configure this globally:

```toml
# ~/.config/mise/config.toml
[settings]
jobs = 8
```

### Interactive Task Scheduling

Tasks with `interactive=true` run with terminal passthrough and are coordinated so runtime process
execution does not overlap with another interactive runtime phase.

In practice:

- Dependencies can still run before an interactive phase.
- Multiple interactive tasks in one run execute in deterministic order (tie-break:
  `task.name`, then args, then env).
- When an interactive task becomes ready, scheduler admission keeps its turn ahead of
  later-ready runtime work, preventing start-order races.
- Admission queues are release-safe: pending interactive/permit entries are removed on
  every terminal path (start, drop, launch error) to avoid stale barriers.
- Structured output modes do not wrap/capture live interactive I/O.

### Scheduler Invariants (Developer Notes)

The scheduler admission pipeline relies on a few invariants to keep behavior deterministic and
deadlock-safe:

- `scheduler_seq` is assigned once per scheduled task in scheduler drain order.
- `interactive_owner` is assigned once per interactive phase and inherited by runtime tasks spawned
  from that phase (injected subgraphs).
- Runtime tasks enqueue into the pending permit queue before admission.
- Interactive phase roots enqueue into the pending interactive queue before admission.
- Queue release rules:
  - `Start`: runtime removes its permit queue entry; interactive removes its own head entry.
  - `Drop` / launch error: queued entries are removed by value to avoid stale queue heads.
- `in_flight`, `runtime_in_flight`, semaphore permits, and interactive gate are always released on
  success, failure, timeout, signal, and pre-exec errors.
- Admission/execution failures are wrapped with a `task trace report` timeline so debugging can
  replay scheduler decisions in order.

When changing admission behavior, preserve these invariants first, then adjust policy.

### Example Execution Flow

Given these tasks:

```toml
[tasks.lint]
run = "eslint src/"

[tasks.test-unit]
depends = ["lint"]
run = "npm run test:unit"

[tasks.test-integration]
depends = ["lint"]
run = "npm run test:integration"

[tasks.build]
depends = ["test-unit", "test-integration"]
run = "npm run build"
```

Execution with `--jobs 2`:

```
Time →
0s:   [lint]
5s:   [test-unit] [test-integration]  # Run in parallel after lint
15s:  [build]                        # Waits for both tests
```

## Task Discovery and Resolution

### Task Sources

mise discovers tasks from multiple sources in this order:

1. **File tasks**: Executable files in task directories
2. **TOML tasks**: Defined in `mise.toml` files
3. **Parent directory tasks**: Available from parent directories

### Task Resolution Process

When you run `mise run build`, mise:

1. **Discovers all tasks** from all configuration sources
2. **Resolves the task name** (handles aliases and partial matches)
3. **Builds dependency graph** including all dependencies
4. **Validates graph** (checks for circular dependencies)
5. **Executes in dependency order** with parallelism

### Task Resolution Across Directories

Tasks from parent directories are available in subdirectories and can be overridden:

```
project/
├── mise.toml              # defines: lint, test, build
└── frontend/
    └── mise.toml          # overrides: test, adds: bundle
```

In `frontend/`, you have access to: `lint` (from parent), `test` (overridden), `build` (from parent), `bundle` (local).

## Advanced Dependency Features

### Conditional Dependencies

Use task arguments for conditional behavior:

```toml
[tasks.test]
depends = ["build"]
run = '''
if [ "$1" = "--with-lint" ]; then
  mise run lint
fi
npm test
'''
```

### Dynamic Dependencies

Tasks can specify dependencies at runtime:

```bash
#!/usr/bin/env bash
#MISE depends=["setup"]

# Additional conditional dependency
if [ ! -f ".env" ]; then
  mise run generate-env
fi

npm start
```

### Cross-Project Dependencies

Reference tasks from other directories:

```toml
[tasks.deploy-all]
depends = [
  "../api:build",
  "../frontend:build",
  "deploy-infrastructure"
]
run = "echo 'All services deployed'"
```

## Performance Optimizations

### Source and Output Tracking

Tasks can skip execution if sources haven't changed:

```toml
[tasks.build]
sources = ["src/**/*.ts", "package.json"]
outputs = ["dist/**/*"]
run = "npm run build"
```

mise will only run the task if:

- Source files are newer than output files
- The task has never been run
- Dependencies have changed

### Incremental Execution

Use `mise run --force` to ignore source/output checking:

```bash
mise run --force build     # Always run, ignore source changes
```

### Parallel File Watching

Use `mise watch` for continuous development:

```bash
mise watch              # Watch all task sources
mise watch build test   # Watch specific tasks
```

This automatically reruns tasks when their source files change.

## Debugging Task Dependencies

### Visualize Dependencies

```bash
mise tasks deps build           # Show build's dependencies
mise tasks deps --dot > deps.dot # Generate graphviz diagram
```

### Execution Tracing

```bash
mise run --verbose build       # Show task execution details
mise run --dry-run build       # Show what would run without executing
```

### Common Issues

**Circular Dependencies**:

```
Error: Circular dependency detected: test → build → test
```

Solution: Remove the circular reference or use `wait_for` instead of `depends`.

**Missing Dependencies**:

```
Error: Task 'build' depends on 'lint' but 'lint' was not found
```

Solution: Define the missing task or remove the dependency.

**Slow Parallel Execution**:

- Check if tasks have unnecessary dependencies
- Use `mise tasks deps` to verify dependency graph
- Consider increasing `--jobs` if you have CPU cores available

The task architecture is designed to scale from simple single-task projects to complex multi-service applications with intricate build dependencies.
