---
description: "Declare project checks with actionable failures and run them with mise doctor project."
---

# Project diagnostics

`mise doctor project` runs the checks a project declares in `[doctor.checks]`
and reports every result. Use it for requirements that tool versions alone do
not cover, such as whether a compiler can find a native library or whether a
database accepts connections.

```mise-toml [mise.toml]
[doctor.checks.openssl]
description = "OpenSSL development files are discoverable"
run = "pkg-config --exists openssl"
hint = "Run `mise bootstrap packages apply` to install the declared build dependencies."
timeout = "5s"
os = ["linux", "macos"]

[doctor.checks.database]
description = "PostgreSQL accepts connections"
run = "pg_isready --quiet"
hint = "Start PostgreSQL, for example with `mise daemons start postgres`."
timeout = "5s"

[doctor.checks.signtool]
description = "The Windows SDK signing tool is installed"
run = "where signtool"
os = "windows"
```

On Linux, with PostgreSQL running and the OpenSSL headers missing:

```sh
mise doctor project
```

```text
PASS database: PostgreSQL accepts connections
FAIL openssl: OpenSSL development files are discoverable
  Command exited with exit status: 1
  Run `mise bootstrap packages apply` to install the declared build dependencies.
SKIP signtool: The Windows SDK signing tool is installed
  Check does not apply to this operating system
```

Results are listed by check name. One failing check does not stop the others.
Checks never run on their own: not when you `cd` into the project and not
before tasks. Plain [`mise doctor`](/cli/doctor.html) checks mise's own
installation and does not run them.

## Check fields

Each `[doctor.checks.<name>]` table accepts:

| Field         | Meaning                                                                                                                                                                                                                                                                                                            |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `run`         | Required. Shell command; exit status 0 passes.                                                                                                                                                                                                                                                                     |
| `description` | The requirement in words. Text output shows the check name when it is missing.                                                                                                                                                                                                                                     |
| `hint`        | What to do after a failure or error. mise prints it and never runs it.                                                                                                                                                                                                                                             |
| `timeout`     | A duration such as `500ms` or `5s`. Defaults to `10s`.                                                                                                                                                                                                                                                             |
| `dir`         | Working directory. Defaults to the [config root](/configuration.html#config-root) of the declaring file, and relative paths resolve from it; `~/` and absolute paths are used as given. In global or system config, the directory mise runs in takes the place of the config root.                                 |
| `shell`       | Program and arguments, including the command flag, such as `"bash -c"` or `"pwsh -Command"`. Defaults to [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args) or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args). |
| `os`          | An OS or OS/architecture selector, or a list of them, such as `"linux/arm64"` or `["linux", "macos"]`. Omit it to run everywhere.                                                                                                                                                                                  |

`os` accepts the same aliases as tool filters, such as `darwin`, `win`, and
`amd64`; an empty list is rejected. `dir` can point outside the project, such as
a sibling checkout, and is not rendered as a template. A `shell` override follows
task quoting rules and honors
[`windows_powershell_no_profile`](/configuration/settings.html#windows_powershell_no_profile).

Checks follow the [config hierarchy](/configuration.html#configuration-hierarchy),
including [config environments](/configuration/environments.html). A check with
the same name in a higher-precedence file replaces the whole check, including
its hint and `dir`.

Checks run in parallel, up to [`jobs`](/configuration/settings.html#jobs) at a
time. Set `jobs = 1` to run them one at a time.

## What a check sees

A check runs with the project's environment and tool `PATH`, as
[`mise exec`](/cli/exec.html) would set them. `mise doctor project` does not
install missing tools, and it does not run task dependencies or hooks. Checks
are not sandboxed and `[env]` directives run as usual, so write checks that only
inspect state. In [safe mode](/security.html#safe-mode), checks that apply to
the current platform report `error` instead of running.

An unknown field inside a check is an error, which catches typos. An unknown
key directly under `[doctor]` produces a warning.

## Results

Each check ends with one of these statuses:

| Status    | Meaning                                                                                                                                                   |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pass`    | The command exited with status 0.                                                                                                                         |
| `fail`    | The command exited with another status. A shell that reports a missing command this way also counts as `fail`.                                            |
| `error`   | mise could not run or finish the check: a timeout, too much output, invalid options, a shell that failed to start, or an unavailable project environment. |
| `skipped` | The check's `os` selection excludes this machine.                                                                                                         |

`mise doctor project` exits with status 1 when any check fails or errors. With
no checks, or only skipped ones, it exits with status 0.

`--json` prints the whole report. `errors` lists config loading errors; when it
is not empty, `checks` is empty. Otherwise each declared check has an entry in
`checks`, and optional fields are `null` when absent:

```json
{
  "errors": [],
  "checks": [
    {
      "name": "openssl",
      "description": "OpenSSL development files are discoverable",
      "source": "/home/user/src/app/mise.toml",
      "status": "fail",
      "message": "Command exited with exit status: 1",
      "hint": "Run `mise bootstrap packages apply` to install the declared build dependencies."
    }
  ]
}
```

When config loads but the tool environment cannot be prepared, each check that
applies to the machine reports `error`, and checks excluded by `os` stay
`skipped`.

## Run checks in CI

```yaml
- run: mise install
- run: mise doctor project
```

The step fails when any check fails or errors.

## Limits

mise stops a check when it reaches its `timeout` or writes more than 64 KiB of
combined stdout and stderr, and ends the processes the check started. Command
output is discarded, so a probe cannot leak a credential into the report. To see
why a check failed, run its command yourself:

```sh
mise exec -- pkg-config --exists openssl
```

Interrupting `mise doctor project` stops every running check.

A passing report says that this machine meets the declared requirements. It
does not make the environment reproducible; keep tool and application
lockfiles for that.
