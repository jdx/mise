# Task artifact cache benchmark

Run `mise run perf:task-cache` to measure the major local-cache phases against
the release binary. The harness creates an isolated temporary project and
reports median wall-clock times for:

- hashing a large source set and publishing a result-only entry;
- executing a task and creating an archive from a large artifact;
- restoring that artifact from the archive; and
- traversing a large graph whose tasks all have current cache entries.

The defaults favor a useful local signal over a quick smoke test: 2,000 source
files, a 32 MiB artifact, 500 graph nodes, and five measured runs per phase.
Override `TASK_CACHE_BENCH_SOURCES`, `TASK_CACHE_BENCH_ARTIFACT_BYTES`,
`TASK_CACHE_BENCH_TASKS`, or `TASK_CACHE_BENCH_RUNS` to scale the fixture. Set
`MISE_BIN` to compare another already-built mise binary without rebuilding it:

```sh
MISE_BIN=/path/to/mise bash benchmarks/task-cache/benchmark.sh
```

Preparation such as deleting restored outputs happens outside each timed
interval. Results are wall-clock measurements, so compare binaries on the same
machine under similar load.
