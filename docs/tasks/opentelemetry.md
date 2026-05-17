# OpenTelemetry <Badge type="warning" text="experimental" />

mise can export traces (and, separately, task stdout/stderr logs) for `mise run` to any
OpenTelemetry-compatible backend such as [Jaeger](https://www.jaegertracing.io/),
[Grafana Tempo](https://grafana.com/oss/tempo/), or [SigNoz](https://signoz.io/).

This is useful when you want to answer questions like:

- Which task is slow?
- Which task failed?
- What did a task print to stdout/stderr? _(requires log export, see below)_
- Which part of a monorepo run did a task belong to?

## Quick Start

Enable OpenTelemetry trace export and set your collector endpoint:

```toml [mise.toml]
[settings]
otel.enabled = true
# Optionally also ship task stdout/stderr as OTLP logs.
# Read the privacy notes below before turning this on.
otel.logs = true
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
- a root span covering the executed tasks (see [Root Span Timing](#root-span-timing))
- task logs from stdout/stderr — only when `otel.logs` is also enabled

## Configuration

mise uses the standard
[OpenTelemetry environment variables](https://opentelemetry.io/docs/specs/otel/protocol/exporter/)
for configuration. The mise-specific settings are opt-in gates — they prevent mise from
unexpectedly emitting telemetry in environments that set `OTEL_EXPORTER_OTLP_*` for other
tools, and they keep log export (which is a larger privacy/security boundary) separate
from trace export.

| Setting        | Env Var             | Default | Description                                                           |
| -------------- | ------------------- | ------- | --------------------------------------------------------------------- |
| `otel.enabled` | `MISE_OTEL_ENABLED` | `false` | Enable OpenTelemetry trace export for task executions.                |
| `otel.logs`    | `MISE_OTEL_LOGS`    | `false` | Enable OpenTelemetry log export for task stdout/stderr (see Privacy). |

Traces and logs are gated independently:

- Traces are exported only when `otel.enabled = true` **and** a traces endpoint is
  configured (`OTEL_EXPORTER_OTLP_ENDPOINT` or `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`).
- Logs are exported only when `otel.logs = true` **and** a logs endpoint is configured
  (`OTEL_EXPORTER_OTLP_ENDPOINT` or `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`).

Setting `otel.enabled` does not, by itself, ship any task output to the collector.

### Standard OTEL Environment Variables

When trace and/or log export is enabled, mise reads the following standard env vars:

| Env Var                              | Description                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------- |
| `OTEL_EXPORTER_OTLP_ENDPOINT`        | General OTLP endpoint (e.g. `http://localhost:4318`).                           |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | Signal-specific traces endpoint. Takes priority over the general endpoint.      |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`   | Signal-specific logs endpoint. Takes priority over the general endpoint.        |
| `OTEL_EXPORTER_OTLP_HEADERS`         | Headers for export requests (comma-separated `key=value` pairs), e.g. for auth. |
| `OTEL_EXPORTER_OTLP_TRACES_HEADERS`  | Signal-specific traces headers. Takes priority over the general headers.        |
| `OTEL_EXPORTER_OTLP_LOGS_HEADERS`    | Signal-specific logs headers. Takes priority over the general headers.          |
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

### Root Span Timing

The root span covers the **task-execution phase** of a `mise run`, not the full
invocation. Telemetry is initialized after task resolution, tool installation, and
automatic dependency setup, and the root span's duration is derived from the earliest
task start time and the latest task end time. Setup, tool install, scheduler overhead,
and post-run teardown are not included in the root span.

Task spans include attributes such as:

| Attribute               | Description                                                                                                 |
| ----------------------- | ----------------------------------------------------------------------------------------------------------- |
| `mise.task.name`        | Task name                                                                                                   |
| `mise.task.args`        | CLI arguments passed to the task (space-joined)                                                             |
| `mise.task.source`      | Path to the config file defining the task                                                                   |
| `mise.task.config_root` | Config root directory (for monorepo tasks)                                                                  |
| `mise.task.skipped`     | `true` when the task was skipped because sources were up to date                                            |
| `process.command_args`  | Full argv as a string array (`["mise", task_name, ...args]`), per OTel CLI semantic conventions             |
| `process.exit.code`     | Exit code of the task as an integer (`0` for success/skipped, propagated from the failed command otherwise) |

## Logs

Log export is a separate, explicit opt-in (`otel.logs = true` / `MISE_OTEL_LOGS=1`)
because shipping task stdout/stderr to the collector is a different trust boundary
from trace export. Read [Privacy and Trust Boundary](#privacy-and-trust-boundary)
before enabling it.

When enabled, each line of task stdout and stderr is exported as an OTLP log record
linked to the corresponding task span, so you can inspect output directly from the
trace.

- stdout is exported with severity `INFO`
- stderr is exported with severity `WARN` (many tools write progress, diagnostics,
  and compiler warnings to stderr that are not errors — actual failure is conveyed
  by the task span status and the `process.exit.code` attribute)

::: tip
Log streaming works with any output mode that captures task output line-by-line (`prefix`,
`keep-order`, `timed`, and `interleave`/`quiet` when no redactions are configured). With
`--raw`, output goes straight to the terminal and is not exported as logs.

In `interleave`/`quiet` mode the child process's stdio is piped (not a TTY) while log
export is active so mise can tee every line to the collector. This can change buffering,
colour output, progress bars, prompts, and any `isatty()`-dependent behaviour. Run
affected tasks under `--raw` to keep a real TTY (at the cost of log export for that
task).
:::

## Privacy and Trust Boundary

Exporting traces and logs ships information about your tasks to your OpenTelemetry
collector. Even though all of this is visible locally already, **the collector is a
different trust boundary** — anything sent there may be stored, indexed, queryable
by other users of that backend, and retained according to its policy.

What trace export (`otel.enabled`) sends per task:

- the task name, display name, args, config source, and config root
- `process.command_args` (the full argv as a string array, per OTel CLI semconv)
- `process.exit.code`
- timing and span status

What log export (`otel.logs`) additionally sends:

- every line written to the task's stdout
- every line written to the task's stderr

**Implications:**

- **Secrets in args.** If a secret appears in `mise.task.args` /
  `process.command_args` (for example `mise run deploy -- --token=hunter2`), trace
  export will ship it to the collector. Prefer passing secrets via environment
  variables, which are never exported.
- **Secrets in output.** With `otel.logs = true`, any secret that a task writes to
  stdout/stderr is shipped to the collector. This includes anything the task
  receives in env vars and accidentally echoes (e.g. via `set -x`, debug logging,
  or shell tracing).
- **Redaction.** mise's terminal redaction (`redactions = […]` in `mise.toml`)
  applies before lines are forwarded to the OTLP log pipeline, so redacted values
  are also redacted in exported logs. However, redaction only covers values you've
  explicitly listed — it does not detect arbitrary secrets in output.
- **`--raw`.** `--raw` bypasses mise's line capture entirely, so task output goes
  straight to the terminal and is **not** exported as logs. Note that this also
  disables redactions for that task.

If you don't want task output leaving the machine, leave `otel.logs = false` (the
default) and rely on trace export alone.

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
spec via the `TRACEPARENT` env var (W3C Traceparent format). This means:

- **Nested `mise run`** invocations automatically join the parent trace.
- **Any OTEL-instrumented tool** a task invokes (Node.js, Go, Python, etc.)
  will automatically parent its spans under the mise task span — no
  mise-specific integration needed.

## Notes

- When neither `otel.enabled` nor `otel.logs` is set, mise does not create trace context
  or export any telemetry.
- Export failures are logged at debug level and never break task execution.
