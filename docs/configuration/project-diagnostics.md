---
description: "Declare project checks with actionable failures and run them with mise doctor project."
---

# Project diagnostics

`mise doctor project` runs named checks from your project's configuration and
reports every result. Use it to check requirements that tool versions alone
cannot establish, such as whether a compiler can discover a native library or
whether a database accepts connections.

```toml
[doctor.checks.openssl]
description = "OpenSSL development files are discoverable"
run = "pkg-config --exists openssl"
hint = "Run `mise bootstrap packages apply` to install the declared build dependencies."
timeout = "5s"
os = ["linux", "macos"]

[doctor.checks.database]
description = "PostgreSQL accepts connections"
run = "pg_isready --quiet"
hint = "Start the project's services with pitchfork."
timeout = "5s"
```

```sh
mise doctor project
mise doctor project --json
```

Example output:

```text
PASS database: PostgreSQL accepts connections
FAIL openssl: OpenSSL development files are discoverable
  Command exited with exit status: 1
  Run `mise bootstrap packages apply` to install the declared build dependencies.
```

Ordinary `mise doctor` continues to diagnose mise itself; it does not run these
checks. Checks do not run automatically when entering a directory or running a
task. There are no implicit project checks: declare the requirements you need.

## Check configuration

Each `[doctor.checks.<name>]` table supports:

| Field         | Meaning                                                                                                                                                       |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `run`         | Required shell command; exit zero to pass.                                                                                                                    |
| `description` | Human-readable requirement. Defaults to the check name in text output.                                                                                        |
| `hint`        | Guidance shown after a failure or execution error. Never executed.                                                                                            |
| `timeout`     | Positive duration such as `500ms` or `5s`; defaults to `10s`.                                                                                                 |
| `dir`         | Working directory. Relative paths resolve from the declaring configuration's project root; absolute paths are used as given. Defaults to that root.           |
| `shell`       | Executable and arguments, including the command flag, such as `["bash", "-c"]` or `["pwsh", "-NoProfile", "-Command"]`. Defaults to mise's inline task shell. |
| `os`          | List of operating systems, such as `["linux", "macos"]`. Omit to run everywhere.                                                                              |

Checks use the active configuration hierarchy, including environment-specific
configuration. A higher-precedence definition replaces the entire check of the
same name, including its hint and working directory. Checks run sequentially in
name order, and a failed check does not stop later checks.

Commands receive the project's environment and installed tool paths, including
environment removals. Diagnostics do not install tools or run task dependencies
or task hooks. Environment directives still evaluate normally, so author checks
as inspection commands and use normal mise configuration trust. Project checks
are unavailable in safe mode; they are not sandboxed.

Each command has a deadline and a combined 64 KiB stdout/stderr limit. Timeouts
and output-limit failures terminate the owned process tree. Command output is
captured and discarded rather than included in reports, so a probe cannot
accidentally print a credential into the report. Use `description` and `hint` to
explain the requirement and remedy; run the command directly for its detailed
output.

Absolute paths and `..` components in `dir` are allowed, as with task working
directories. The configuration root anchors relative paths; it is not a
filesystem boundary. Checks can intentionally inspect a sibling checkout or
shared local service directory.

On Unix, Ctrl-C, SIGTERM, and SIGHUP cancel the current check and close its owned
process group, including when doctor runs inside a mise task. As with any cleanup
that requires the supervisor to run, SIGKILL prevents cleanup; stop doctor with
SIGTERM before escalating to SIGKILL.

## Results and automation

The JSON report contains a `checks` array. Each entry contains `name`,
`description`, `source` (the declaring configuration file), `status`, `message`,
and `hint`. Optional fields are `null` when absent.

- `pass`: the command exited successfully.
- `fail`: the command exited unsuccessfully.
- `error`: mise could not execute or finish the check, including a timeout,
  invalid check options, or an unavailable project environment.
- `skipped`: the check's `os` selection excludes the current system.

A shell can report a missing command as a nonzero exit; that is a `fail`, while
failure to launch the shell itself is an `error`.

The command exits with status 1 if any check fails or cannot be completed.
An empty check list or only skipped checks exits successfully. Configuration
loading failures, including required environment values that prevent loading,
are reported as a `configuration` error. If configuration loads but preparing
the tool environment fails, affected checks each report an error and
platform-excluded checks remain skipped.

This is a compatibility check of the current machine. Passing does not establish
whole-environment reproducibility. Keep tool and application lockfiles for the
parts of your environment they cover.
