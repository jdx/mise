---
description: "Export OpenTelemetry traces and task output logs from mise run to Jaeger, Grafana Tempo, or any OTLP backend."
socialDescription: "Export traces and task logs from mise run to any OpenTelemetry backend."
---

# OpenTelemetry <Badge type="warning" text="experimental" />

mise can export a trace of each `mise run` to any backend that accepts OTLP
over HTTP, such as [Jaeger](https://www.jaegertracing.io/),
[Grafana Tempo](https://grafana.com/oss/tempo/), or [SigNoz](https://signoz.io/).
Traces show which tasks are slow or failing and which monorepo project each
task belongs to. With log export on, each task's output appears next to its
span.

::: warning Experimental
Span names, attributes, and settings may change between releases. Export does
not need `experimental = true`; the `otel.enabled` and `otel.logs` settings
turn it on.
:::

## Quick start

Start a local Jaeger that accepts OTLP over HTTP:

```sh
docker run -d --name jaeger -p 16686:16686 -p 4318:4318 jaegertracing/jaeger:latest
```

Turn on trace export:

```toml [mise.toml]
[settings]
otel.enabled = true
```

Point mise at Jaeger and run some tasks:

```sh
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
mise run build ::: test ::: lint
```

Open `http://localhost:16686` and choose the `mise` service. To export task
output as well, see [Logs](#logs).

## Configuration

Two settings turn export on, one per signal:

| Setting                                                     | Environment variable | Default | Exports                                                  |
| ----------------------------------------------------------- | -------------------- | ------- | -------------------------------------------------------- |
| [`otel.enabled`](/configuration/settings.html#otel.enabled) | `MISE_OTEL_ENABLED`  | `false` | Traces for `mise run`.                                   |
| [`otel.logs`](/configuration/settings.html#otel.logs)       | `MISE_OTEL_LOGS`     | `false` | Task stdout and stderr as logs. See [Privacy](#privacy). |

Everything else comes from the standard
[OpenTelemetry environment variables](https://opentelemetry.io/docs/specs/otel/protocol/exporter/).
mise exports nothing unless one of the settings is on, so an
`OTEL_EXPORTER_OTLP_ENDPOINT` set for another tool does not make mise send
data. Each signal also needs an endpoint: the general
`OTEL_EXPORTER_OTLP_ENDPOINT` or the signal's own `..._TRACES_ENDPOINT` or
`..._LOGS_ENDPOINT`. An empty value counts as unset.

### Standard environment variables

| Variable                                                                | Purpose                                                                                                                                                |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `OTEL_EXPORTER_OTLP_ENDPOINT`                                           | The OTLP endpoint for both signals, for example `http://localhost:4318`.                                                                               |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`                                    | The traces endpoint. Takes priority over the general endpoint.                                                                                         |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`                                      | The logs endpoint. Takes priority over the general endpoint.                                                                                           |
| `OTEL_EXPORTER_OTLP_HEADERS`                                            | Headers for export requests, as comma-separated `key=value` pairs, for example for authentication.                                                     |
| `OTEL_EXPORTER_OTLP_TRACES_HEADERS`                                     | Headers for trace export. Take priority over the general headers.                                                                                      |
| `OTEL_EXPORTER_OTLP_LOGS_HEADERS`                                       | Headers for log export. Take priority over the general headers.                                                                                        |
| `OTEL_EXPORTER_OTLP_PROTOCOL`                                           | `http/protobuf` (the default) or `http/json`. gRPC is not supported.                                                                                   |
| `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL`                                    | The protocol for trace export. Takes priority over the general protocol.                                                                               |
| `OTEL_EXPORTER_OTLP_LOGS_PROTOCOL`                                      | The protocol for log export. Takes priority over the general protocol.                                                                                 |
| `OTEL_EXPORTER_OTLP_TIMEOUT` (`..._TRACES_TIMEOUT`, `..._LOGS_TIMEOUT`) | The export request timeout in milliseconds. mise defaults to 3 seconds, so an unreachable collector delays the end of `mise run` by at most that long. |
| `OTEL_SERVICE_NAME`                                                     | The `service.name` resource attribute. Defaults to `mise`.                                                                                             |
| `OTEL_RESOURCE_ATTRIBUTES`                                              | More resource attributes, as comma-separated `key=value` pairs. A `service.name` here also replaces the default.                                       |

To send traces to an authenticated collector:

```sh
export MISE_OTEL_ENABLED=1
export OTEL_EXPORTER_OTLP_ENDPOINT=https://otel.example.com:4318
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer mytoken"
export OTEL_RESOURCE_ATTRIBUTES="deployment.environment=staging,team.name=platform"
```

## Trace structure

Each `mise run` creates one trace. Its root span covers the whole run, setup
spans cover the work before tasks start, and each task gets a span. Tasks whose
config root differs from the project you ran in, such as another monorepo
project or a parent directory's config, are grouped under a span for that
config root:

```text
mise run //packages/frontend:lint …       ← root span
├── resolve tasks                         ← setup span
├── install tools                         ← setup span
├── deps                                  ← setup span
├── start daemons                         ← setup span
├── packages/frontend                     ← monorepo group span
│   ├── //packages/frontend:lint          ← task span
│   ├── //packages/frontend:typecheck     ← task span
│   └── //packages/frontend:build         ← task span
├── packages/backend                      ← monorepo group span
│   └── //packages/backend:test           ← task span
└── //:deploy                             ← task span (direct child of root)
```

The root span is named after the tasks you asked for, by their full resolved
names. A task span uses the task's full name plus any arguments, such as
`//:deploy prod`. A run that fetches [remote tasks](/tasks/toml-tasks.html#remote-tasks)
also has a `fetch tasks` setup span. See [Monorepo tasks](/tasks/monorepo.html)
for how mise names projects.

The root span has the attribute `mise.span_type = "run"`, setup spans have
`"setup"`, and group spans have `"monorepo_group"` plus `mise.config_root`, the
group's config root. Task spans have no `mise.span_type`.

### Span timing

The root span starts once mise knows which tasks you asked for and ends after
the last task finishes. The setup spans show where the time before the first
task went: fetching remote tasks, resolving the task graph, installing missing
tools, running deps providers, and starting daemons. A slow first run on a fresh
CI runner shows up as a long `install tools` span. A failed setup phase marks
both its span and the root span as errors.

A task span starts when mise picks the task up, so the gap before its first
output is mise preparing that task's tools and environment. Root-span time that
no child covers is scheduling overhead and time spent waiting for a free job
slot (see [`jobs`](/configuration/settings.html#jobs)). A group span stays open until the
run ends, because mise cannot know whether another task from that project is
still to come.

### Task span attributes

| Attribute                | Value                                                                                                    |
| ------------------------ | -------------------------------------------------------------------------------------------------------- |
| `mise.task.name`         | The task name.                                                                                           |
| `mise.task.display_name` | The task name plus its arguments, which is also the span name.                                           |
| `mise.task.args`         | The arguments passed to the task, joined with spaces. Present only when there are arguments.             |
| `mise.task.source`       | The path of the config file that defines the task.                                                       |
| `mise.task.config_root`  | The config root of the task's config file.                                                               |
| `mise.task.skipped`      | `true` when mise skipped the task because its sources were up to date.                                   |
| `mise.task.cancelled`    | `true` when mise stopped the task because another task failed or the run was interrupted (Ctrl-C).       |
| `process.command_args`   | The full argv as a string array (`["mise", task_name, ...args]`), per the OpenTelemetry CLI conventions. |
| `process.exit.code`      | The task's exit code as an integer: `0` for success or a skipped task, otherwise the failed command's.   |

### Failed vs. cancelled tasks

Unless you pass `--continue-on-error`, a failing task stops the tasks running
beside it, so several tasks can end at once from one fault. Only the task that
failed gets an `Error` span status. The tasks mise shut down get an `Unset`
status and `mise.task.cancelled = true`, so a search for errored spans finds
the one real cause. Because a signal ended them, cancelled tasks usually have no
`process.exit.code`. The root span is still an `Error`, since the run failed.

A task that exits with its own non-zero status after the first failure is still
an `Error`, and so is one that crashes. Only tasks ended by the SIGTERM mise
sends, or interrupted by SIGINT, count as cancelled. Windows reports a
terminated process as an ordinary exit code, so there every task that ends
after the first failure is recorded as cancelled.

A task whose tools fail to install never starts, but it still gets an `Error`
span, since its failure stopped the run.

When `--timeout` expires, mise ends the root span as an `Error` and flushes it.
Tasks still running at that point are not exported.

## Trace propagation

mise passes each task its trace context in the `TRACEPARENT` and `TRACESTATE`
environment variables, following the
[OpenTelemetry environment carriers](https://opentelemetry.io/docs/specs/otel/context/env-carriers/)
specification. A nested `mise run` joins the same trace, and an
OpenTelemetry-instrumented program that a task runs, in any language, puts its
spans under the task's span without other setup.

## Logs

Log export is a separate opt-in (`otel.logs = true` or `MISE_OTEL_LOGS=1`)
because it sends task output, not only timing, to the collector. Read
[Privacy](#privacy) before you turn it on.

With log export on, mise exports each line a task writes to stdout or stderr as
an OTLP log record linked to the task's span, so you can read the output from
the trace. stdout lines have severity `INFO`, and stderr lines have `WARN`,
because many tools write progress and warnings to stderr; the span status and
`process.exit.code` show whether the task failed. Each record carries
`mise.task.name`, `mise.task.args` when the task has arguments, and
`output.stream` (`stdout` or `stderr`).

The link to a span needs trace export too. With `otel.logs` but not
`otel.enabled`, records still carry trace and span IDs, but no spans are
exported for them to point at.

### What is exported

mise exports output in every mode that reads it line by line: `prefix`,
`keep-order`, `timed`, `replacing`, `interleave`, and `quiet`. Output in the
`silent` mode, from [silenced](/tasks/task-configuration.html#silent) tasks, or
under `--raw` is not exported.

::: warning
With log export on, `interleave` and `quiet` tasks get a pipe instead of the
terminal so mise can read every line. That can change buffering, colors,
progress bars, prompts, and anything else that checks for a TTY. Run such a task
with `--raw` to keep the terminal; its output is then not exported.
:::

### Nested `mise run`

When a task runs `mise run`, the inner run's output also flows through the outer
task. mise exports each line once, attributed to the innermost task that printed
it:

```mise-toml
[tasks.outer]
run = "echo building; mise run inner; echo done"
```

`building` and `done` belong to the `outer` span, and everything `inner` prints
belongs to the `inner` span. If the inner run does not export logs itself,
because `otel.logs` is off for it or it uses `--raw`, the outer run exports that
output instead. Output from a `raw` or `interactive` task inside a nested run
that does export logs is exported by neither run. Terminal output is the same
either way.

## Privacy

Anything mise exports can be stored, indexed, and read by everyone with access
to your telemetry backend, under that backend's retention policy. Trace export
sends each task's name, display name, arguments, config file and config root,
`process.command_args`, exit code, timing, and status. Log export also sends
every line the task writes to stdout and stderr.

- Pass secrets in environment variables, not arguments. Arguments, for example
  `mise run deploy -- --token=hunter2`, are exported unless they match a
  [redaction](/environments/secrets/#redaction); environment variables are not
  exported.
- With `otel.logs = true`, any secret a task prints is exported, including one
  that leaks through `set -x` or debug logging.
- Redactions apply to exported arguments, span names, error messages, and log
  lines, but they hide only the values you list. They do not detect secrets.
- `--raw` output is never exported, and redactions do not apply to it either.

To keep task output on the machine, leave `otel.logs` off.

## Behavior and limits

- Without `otel.enabled` or `otel.logs`, mise creates no trace context and
  exports nothing.
- Offline mode ([`offline`](/configuration/settings.html#offline) or
  `MISE_OFFLINE=1`) turns export off.
- Export failures never fail a task. Run with `MISE_DEBUG=1` to see them.
