# Task Runner Parity Tracker

> [!IMPORTANT]
> This is a temporary implementation tracker. Delete this file in the same pull request that checks
> off the final remaining item.

This tracks the work required to bring mise's experimental task output cache and monorepo task
graph close to Turborepo's functionality, with selected Nx-style inference where it fits mise's
language-agnostic model.

Cache lookup, project selection, and task inference are separate layers: the cache decides whether a
selected task executes or restores an artifact, affected analysis decides which project tasks are
selected, and workspace providers turn ecosystem metadata into projects, dependencies, and inferred
tasks. Keeping those layers separate should allow explicit mise tasks to coexist with inference.

Distributed task execution, code generation, and a comprehensive Nx-style plugin ecosystem remain
outside this tracker.

## Experimental local cache

- [x] Opt-in experimental task cache configuration
- [x] Scoped cache defaults for eligible tasks with task-level opt-out
- [x] Content-derived keys for sources, task definitions, arguments, declared environment, selected
      ambient environment variables, resolved tools, operating system, and architecture
- [x] Store and restore explicitly declared file and directory outputs
- [x] Preserve directories, regular files, symlinks, permissions, and modification times
- [x] Reject automatic, absolute, escaping, and source-containing output paths
- [x] Treat corrupt entries as misses and cache write failures as warnings
- [x] Integrate artifacts with the existing cache clear and prune commands
- [x] Document the experimental behavior and configuration schema
- [x] Cover hits, misses, environment changes, source changes, and invalid configurations in tests

## Workspace and project graph

- [x] Define a provider-neutral project model with stable project IDs, roots, metadata, and
      dependency edges
- [x] Define a workspace provider interface that can discover projects and dependencies
      deterministically
- [x] Merge graphs from multiple providers for polyglot repositories
- [x] Allow explicit configuration to add, remove, or override inferred projects and dependency
      edges
- [x] Detect project graph cycles and report actionable diagnostics
- [x] Preserve dependency traversal through projects that do not implement the requested task
- [x] Add human-readable and machine-readable project graph inspection

## Workspace providers and task inference

- [x] Add a Node workspace provider for npm, pnpm, Yarn, and Bun workspace definitions
- [x] Infer Node project dependency edges from declared internal package dependencies
- [x] Import package scripts as scoped mise tasks without requiring a `mise.toml` in every package
- [x] Add root task defaults that apply by task name across inferred and explicit projects
- [x] Add dependency-scoped task relationships equivalent to building the same task in upstream
      projects first
- [x] Define deterministic precedence between provider inference, root task defaults, task
      templates, and project-local configuration
- [x] Allow providers to suggest inputs, outputs, cacheability, and task dependencies when they can
      derive them confidently
- [x] Explain which provider inferred each project, task, input, output, and dependency
- [x] Add Cargo workspace and path-dependency inference
- [x] Add uv workspace and local/path-dependency inference
- [x] Add Go workspace discovery, with explicit dependency overrides where module metadata is
      insufficient

## Affected project execution

- [x] Map changed files to their owning projects
- [x] Compute affected projects from changed projects and reverse project dependency edges
- [x] Add configurable Git base and head revisions with sensible local and CI defaults
- [x] Treat workspace-global files and explicitly declared global inputs as affecting the
      appropriate project set
- [x] Account for lockfile changes at project granularity when a provider can determine the affected
      external dependencies
- [x] Allow affected selection to feed existing task patterns, dependency expansion, scheduling, and
      caching
- [x] Show why each project and task was considered affected
- [x] Add machine-readable affected-project and affected-task output

## Local cache parity

- [x] Include dependency artifact keys in downstream task keys
- [x] Allow dependents to restore when dependencies publish stable artifact keys;
      unkeyed dependency work still forces execution
- [x] Capture and replay stdout and stderr while respecting mise output modes
- [x] Cache useful results for tasks with no filesystem outputs
- [x] Support reusable and global input groups
- [x] Support global environment inputs and pass-through environment variables
- [x] Support negative input and output patterns
- [x] Support command-output inputs
- [x] Include external dependency and lockfile state through explicit input configuration
- [x] Add per-run cache read/write controls, including local-only and cache-disabled modes
- [x] Add a configurable local task-cache directory

## Inspection and diagnostics

- [x] Explain the inputs that produced a task's cache key
- [x] Report the reason for each cache miss
- [x] Show resolved cache inputs and outputs
- [x] Add machine-readable cache details to dry-run or task-info output
- [x] Report hit rate, bytes restored, and time saved
- [x] Add per-task cache inspection and deletion

## Integrity and portability

- [x] Add an artifact checksum independent of the cache key
- [x] Test and harden concurrent readers and writers for the same key
- [x] Clean up interrupted and abandoned partial writes
- [x] Add configurable cache size and age limits
- [x] Test restore behavior on Windows
- [x] Test executable bits, symlinks, empty directories, and case-sensitive path edge cases
- [x] Benchmark hashing, archive creation, and restoration for large task graphs and artifacts
- [x] Avoid rehashing unchanged source files when reliable metadata is available
- [x] Add an optional audit mode for undeclared task reads and writes

## Remote cache

- [x] Extract a versioned cache-store interface from the local filesystem implementation
- [x] Define and document a versioned remote cache protocol
- [x] Add a composite local and remote cache store
- [x] Stream artifact uploads and downloads
- [x] Add remote read-only, write-only, and read/write modes
- [x] Add authentication and repository or organization namespaces
- [ ] Add timeouts, retries, request deduplication, and offline fallback
- [ ] Verify remote artifact integrity and authenticity
- [ ] Document secret handling for cached logs and artifacts
- [ ] Provide a self-hosting reference or compatibility test suite

## Stabilization

- [ ] Decide which experimental configuration names become stable
- [ ] Document cache correctness requirements and deterministic-task expectations
- [ ] Publish migration notes if the experimental configuration or artifact format changes
- [ ] Remove the experimental gate
- [ ] Delete this tracker
