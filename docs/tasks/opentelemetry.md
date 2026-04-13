# OpenTelemetry <Badge type="warning" text="experimental" />

mise can export traces and logs for `mise run` to any OpenTelemetry-compatible backend such as
[Jaeger](https://www.jaegertracing.io/), [Grafana Tempo](https://grafana.com/oss/tempo/), or
[SigNoz](https://signoz.io/).

This is useful when you want to answer questions like:

- Which task is slow?
- Which task failed?
- What did a task print to stdout/stderr?
- Which part of a monorepo run did a task belong to?

## Quick Start

Enable OpenTelemetry and set your collector endpoint:

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

- one trace for the full `mise run`
- spans for individual tasks
- grouped spans for monorepo task roots
- task logs from stdout/stderr

## Configuration

mise uses the standard
[OpenTelemetry environment variables](https://opentelemetry.io/docs/specs/otel/protocol/exporter/)
for configuration. The only mise-specific setting is `otel.enabled` which acts as an opt-in
gate — this prevents mise from unexpectedly emitting spans in environments that set
`OTEL_EXPORTER_OTLP_ENDPOINT` for other tools.

| Setting        | Env Var             | Default | Description                                  |
| -------------- | ------------------- | ------- | -------------------------------------------- |
| `otel.enabled` | `MISE_OTEL_ENABLED` | `false` | Enable OpenTelemetry export for task traces. |

### Standard OTEL Environment Variables

Once `otel.enabled` is `true`, mise reads the following standard env vars:

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

- a root span for the full `mise run`
- task spans for individual tasks
- monorepo group spans when tasks come from different `config_root`s

Typical shape:

```
mise run                          ← root span (full duration)
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

Task spans include attributes such as:

| Attribute               | Description                                                        |
| ----------------------- | ------------------------------------------------------------------ |
| `mise.task.name`        | Task name                                                          |
| `mise.task.args`        | CLI arguments passed to the task (space-joined)                    |
| `mise.task.source`      | Path to the config file defining the task                          |
| `mise.task.config_root` | Config root directory (for monorepo tasks)                         |
| `mise.task.skipped`     | `"true"` when the task was skipped because sources were up to date |

## Logs

Task stdout and stderr are exported as logs and linked to the corresponding task span, so you can
inspect output directly from the trace.

- stdout is exported with severity `INFO`
- stderr is exported with severity `WARN`

::: tip
Log streaming works with any output mode that captures task output line-by-line (`prefix`,
`keep-order`, `timed`, and `interleave`/`quiet` when no redactions are configured). With
`--raw`, output goes straight to the terminal and is not exported as logs.

In `interleave`/`quiet` mode the child process's stdio is piped (not a TTY) while otel is
active so mise can tee every line to the collector. Tasks that rely on `isatty()` or use
`\r` to overwrite lines will see different behaviour — run them under `--raw` to keep a
real TTY (at the cost of log export for that task).
:::

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

- When `otel.enabled` is not set, mise does not create trace context or export any telemetry.
- Export failures are logged at debug level and never break task execution.
