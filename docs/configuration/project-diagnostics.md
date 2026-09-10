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

| Field         | Meaning                                                                                                                                             |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `run`         | Required shell command; exit zero to pass.                                                                                                          |
| `description` | Human-readable requirement. Defaults to the check name in text output.                                                                              |
| `hint`        | Guidance shown after a failure or execution error. Never executed.                                                                                  |
| `timeout`     | Positive duration such as `500ms` or `5s`; defaults to `10s`.                                                                                       |
| `dir`         | Working directory. Relative paths resolve from the declaring configuration's project root; absolute paths are used as given. Defaults to that root. |
| `shell`       | Executable and arguments, including the command flag, such as `"bash -c"` or `"pwsh -Command"`. Defaults to mise's inline task shell.               |
| `os`          | OS or OS/arch selector, or a nonempty list, such as `"linux/arm64"` or `["linux", "darwin"]`. Omit to run everywhere.                               |

Checks use the active configuration hierarchy, including environment-specific
configuration. A higher-precedence definition replaces the entire check of the
same name, including its hint and working directory. Checks run concurrently, bounded by the `jobs` setting (`MISE_JOBS`), so slow
probes can share their waiting time. Results remain in name order and a failed
check does not stop other checks. Set `jobs = 1` for sequential execution.

Commands receive the project's environment and installed tool paths, including
environment removals. Diagnostics do not install tools or run task dependencies
or task hooks. Environment directives still evaluate normally, so author checks
as inspection commands and use normal mise configuration trust. Project checks
that apply to the current platform report errors in safe mode. Empty or entirely
platform-excluded check lists still succeed. Checks are not sandboxed.

Each command has a deadline and a combined 64 KiB stdout/stderr limit. Timeouts
and output-limit failures terminate the owned process tree. Command output is
captured and discarded rather than included in reports, so a probe cannot
accidentally print a credential into the report. Use `description` and `hint` to
explain the requirement and remedy; run the command directly for its detailed
output.

Absolute paths and `..` components in `dir` are allowed, as with task working
directories. The configuration root anchors relative paths; it is not a
filesystem boundary. Checks can intentionally inspect a sibling checkout or
shared local service directory. Checks declared in global or system configuration
use the invocation directory as their root, like tasks. Shell overrides use task
quoting conventions and honor `windows_powershell_no_profile`. The OS selector
accepts the same aliases (`darwin`, `win`, `amd64`, and others) as tool filters;
`os = []` is rejected. Unknown `[doctor]` container options are ignored for
forward compatibility, while unknown check fields are rejected to catch typos.

On Unix, Ctrl-C, SIGTERM, and SIGHUP cancel running checks and close their owned
process groups, including when doctor runs inside a mise task. As with any cleanup
that requires the supervisor to run, SIGKILL prevents cleanup; stop doctor with
SIGTERM before escalating to SIGKILL.

## Results and automation

The JSON report contains `checks` and `errors` arrays. Top-level `errors`
contains configuration loading errors; in that case `checks` is empty. Otherwise
`errors` is empty and each declared check has a result in `checks`. Each entry contains `name`,
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
are reported in the top-level `errors` array. If configuration loads but preparing
the tool environment fails, affected checks each report an error and
platform-excluded checks remain skipped.

This is a compatibility check of the current machine. Passing does not establish
whole-environment reproducibility. Keep tool and application lockfiles for the
parts of your environment they cover.
