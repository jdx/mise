# Task Cache Parity Tracker

> [!IMPORTANT]
> This is a temporary implementation tracker. Delete this file in the same pull request that checks
> off the final remaining item.

This tracks the work required to bring mise's experimental task output cache close to Turborepo's
caching functionality. Nx-specific project inference, affected-project analysis, and distributed
task execution are outside this tracker because they are broader task-runner features rather than
cache functionality.

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

## Local cache parity

- [ ] Include dependency artifact keys in downstream task keys
- [ ] Allow dependents to restore after dependencies execute or restore
- [ ] Capture and replay stdout and stderr while respecting mise output modes
- [ ] Cache useful results for tasks with no filesystem outputs
- [ ] Support reusable and global input groups
- [ ] Support global environment inputs and pass-through environment variables
- [ ] Support negative input and output patterns
- [ ] Support runtime-command inputs
- [ ] Include external dependency and lockfile state through explicit input configuration
- [ ] Add per-run cache read/write controls, including local-only and cache-disabled modes
- [ ] Add a configurable local task-cache directory

## Inspection and diagnostics

- [ ] Explain the inputs that produced a task's cache key
- [ ] Report the reason for each cache miss
- [ ] Show resolved cache inputs and outputs
- [ ] Add machine-readable cache details to dry-run or task-info output
- [ ] Report hit rate, bytes restored, and time saved
- [ ] Add per-task cache inspection and deletion

## Integrity and portability

- [ ] Add an artifact checksum independent of the cache key
- [ ] Test and harden concurrent readers and writers for the same key
- [ ] Clean up interrupted and abandoned partial writes
- [ ] Add configurable cache size and age limits
- [ ] Test restore behavior on Windows
- [ ] Test executable bits, symlinks, empty directories, and case-sensitive path edge cases
- [ ] Benchmark hashing, archive creation, and restoration for large task graphs and artifacts
- [x] Avoid rehashing unchanged source files when reliable metadata is available
- [ ] Add an optional audit mode for undeclared task reads and writes

## Remote cache

- [ ] Extract a versioned cache-store interface from the local filesystem implementation
- [ ] Define and document a versioned remote cache protocol
- [ ] Add a composite local and remote cache store
- [ ] Stream artifact uploads and downloads
- [ ] Add remote read-only, write-only, and read/write modes
- [ ] Add authentication and repository or organization namespaces
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
