# OpenTelemetry <Badge type="warning" text="experimental" />

mise can export traces for `mise run` to any
OpenTelemetry-compatible backend such as [Jaeger](https://www.jaegertracing.io/),
[Grafana Tempo](https://grafana.com/oss/tempo/), or [SigNoz](https://signoz.io/).

This is useful when you want to answer questions like:

- Which task is slow?
- Which task failed?
- Which part of a monorepo run did a task belong to?

## Quick Start

Enable OpenTelemetry trace export and set your collector endpoint:

```toml [mise.toml]
[settings]
otel.enabled = true
```

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
```

Then run your tasks as usual:

```bash
mise run build ::: test ::: lint
```

If your collector is reachable, mise will export:

- spans for individual tasks
- grouped spans for monorepo task roots
- a root span covering the executed tasks (see [Span Timing](#span-timing))

## Configuration

mise uses the standard
[OpenTelemetry environment variables](https://opentelemetry.io/docs/specs/otel/protocol/exporter/)
for configuration. The mise-specific settings are opt-in gates — they prevent mise from
unexpectedly emitting telemetry in environments that set `OTEL_EXPORTER_OTLP_*` for other
tools.

| Setting        | Env Var             | Default | Description                                            |
| -------------- | ------------------- | ------- | ------------------------------------------------------ |
| `otel.enabled` | `MISE_OTEL_ENABLED` | `false` | Enable OpenTelemetry trace export for task executions. |

Traces are exported only when `otel.enabled = true` **and** a traces endpoint is
configured (`OTEL_EXPORTER_OTLP_ENDPOINT` or `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`).

Setting `otel.enabled` does not, by itself, ship any task output to the collector.

### Standard OTEL Environment Variables

When trace export is enabled, mise reads the following standard env vars:

| Env Var                              | Description                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------- |
| `OTEL_EXPORTER_OTLP_ENDPOINT`        | General OTLP endpoint (e.g. `http://localhost:4318`).                           |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | Signal-specific traces endpoint. Takes priority over the general endpoint.      |
| `OTEL_EXPORTER_OTLP_HEADERS`         | Headers for export requests (comma-separated `key=value` pairs), e.g. for auth. |
| `OTEL_EXPORTER_OTLP_TRACES_HEADERS`  | Signal-specific traces headers. Takes priority over the general headers.        |
| `OTEL_SERVICE_NAME`                  | The `service.name` resource attribute (defaults to `mise`).                     |
| `OTEL_RESOURCE_ATTRIBUTES`           | Additional resource attributes (comma-separated `key=value` pairs).             |

Example with authentication:

```bash
export MISE_OTEL_ENABLED=1
export OTEL_EXPORTER_OTLP_ENDPOINT=https://otel.example.com:4318
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer mytoken"
```

Example with resource attributes:

```bash
export OTEL_RESOURCE_ATTRIBUTES="deployment.environment=staging,team.name=platform"
```

## What You See

Each `mise run` creates one trace.

That trace contains:

- a root span covering the task-execution phase of `mise run`
- task spans for individual tasks
- monorepo group spans when tasks come from different `config_root`s

Typical shape:

```
mise run                          ← root span (see Root Span Timing)
├── packages/frontend             ← monorepo group span
│   ├── lint                      ← task span
│   ├── typecheck                 ← task span
│   └── build                     ← task span
├── packages/backend              ← monorepo group span
│   └── test                      ← task span
└── deploy                        ← task span (direct child of root)
```

For monorepos, this makes it easier to see which package or subproject a task came from. See
[Monorepo Tasks](/tasks/monorepo) for background on `config_root`.

### Span Timing

Spans are live for exactly as long as the thing they measure, so durations nest the way
you'd expect: the root span opens once telemetry is initialized and closes after the last
task finishes, and each group span covers its members.

Telemetry is initialized after task resolution, tool installation, and automatic
dependency setup, so those phases sit outside the root span. Everything after it —
scheduler overhead, per-task queueing on the `jobs` semaphore, toolset and environment
resolution — is inside it. A task span starts when the scheduler picks the task up, not
when its process is spawned, so the gap between a task span's start and its first output
is mise's own per-task setup rather than the task itself.

Task spans include attributes such as:

| Attribute               | Description                                                                                                 |
| ----------------------- | ----------------------------------------------------------------------------------------------------------- |
| `mise.task.name`        | Task name                                                                                                   |
| `mise.task.args`        | CLI arguments passed to the task (space-joined)                                                             |
| `mise.task.source`      | Path to the config file defining the task                                                                   |
| `mise.task.config_root` | Config root directory (for monorepo tasks)                                                                  |
| `mise.task.skipped`     | `true` when the task was skipped because sources were up to date                                            |
| `mise.task.cancelled`   | `true` when the task was stopped because a *sibling* task failed                                            |
| `process.command_args`  | Full argv as a string array (`["mise", task_name, ...args]`), per OTel CLI semantic conventions             |
| `process.exit.code`     | Exit code of the task as an integer (`0` for success/skipped, propagated from the failed command otherwise) |

### Failed vs. Cancelled Tasks

By default a failing task stops its siblings (unless `--continue-on-error` is set), so
several tasks can end at once from a single fault. Only the task that actually failed
gets an `Error` span status; the siblings mise shut down are recorded with an `Unset`
status and `mise.task.cancelled = true`, so a search for errored spans returns the one
real cause rather than every task that happened to be running. Because they were
terminated by a signal and have no exit code of their own, cancelled tasks carry no
`process.exit.code`. The root span is still marked `Error`, since the run as a whole
failed.

## Privacy and Trust Boundary

Exporting traces ships information about your tasks to your OpenTelemetry
collector. Even though all of this is visible locally already, **the collector is a
different trust boundary** — anything sent there may be stored, indexed, queryable
by other users of that backend, and retained according to its policy.

What trace export (`otel.enabled`) sends per task:

- the task name, display name, args, config source, and config root
- `process.command_args` (the full argv as a string array, per OTel CLI semconv)
- `process.exit.code`
- timing and span status

**Implications:**

- **Secrets in args.** If a secret appears in `mise.task.args` /
  `process.command_args` (for example `mise run deploy -- --token=hunter2`), trace
  export will ship it to the collector. Prefer passing secrets via environment
  variables, which are never exported.
Task stdout/stderr is **not** exported by trace export — only the attributes listed
above leave the machine.

## Example: Local Development with Jaeger

Start Jaeger with OTLP/HTTP support:

```bash
docker run -d --name jaeger \
  -p 16686:16686 \
  -p 4318:4318 \
  jaegertracing/all-in-one:latest
```

Configure mise:

```toml [mise.toml]
[settings]
otel.enabled = true
```

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
```

Now run any mise task and open `http://localhost:16686`.

## Trace Propagation

mise propagates trace context to child processes using the
[OpenTelemetry Environment Carriers](https://opentelemetry.io/docs/specs/otel/context/env-carriers/)
spec via the `TRACEPARENT` and `TRACESTATE` env vars (W3C Trace Context
format). This means:

- **Nested `mise run`** invocations automatically join the parent trace.
- **Any OTEL-instrumented tool** a task invokes (Node.js, Go, Python, etc.)
  will automatically parent its spans under the mise task span — no
  mise-specific integration needed.

## Notes

- When `otel.enabled` is not set, mise does not create trace context or export any
  telemetry.
- Export failures are logged at debug level and never break task execution.
