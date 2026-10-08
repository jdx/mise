---
description: "Write tasks as executable scripts in a task directory and configure them with #MISE comments."
---

# File tasks

A file task is an executable script in a task directory. Its path is the task
name, and `#MISE` comments at the top hold its configuration. Use one when a
script is long enough to benefit from your editor's highlighting and linting.

Save this as `mise-tasks/build`:

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE description="Build the CLI"
cargo build
```

On Linux and macOS, make the script executable:

```sh
chmod +x mise-tasks/build
```

`mise run build` now runs the script with the tools and environment from your
config. mise lists only executable scripts as tasks. When you run a script that
is not executable, mise asks whether to mark it executable, and fails if you
decline. Windows has no execute bit; see [Windows](#windows).

You can still run the script without mise, but its `#MISE` and `#USAGE`
comments then have no effect, so the caller must supply its tools, environment,
and any `usage_*` variables.

`mise-tasks/` is one of several
[default task directories](/tasks/task-discovery.html#default-task-directories).
Setting [`task_config.includes`](/tasks/task-configuration.html#task_config.includes)
replaces them with the paths you list.

## Name and group file tasks {#task-grouping}

A file's path below the task directory is its task name, with `/` replaced by
`:`. `mise-tasks/db/migrate.sh` is `db:migrate`, and you can run it as
`mise run db:migrate` or `mise run db:migrate.sh`. mise shows the name without
the extension unless another task already has that name. A file named
`_default` takes its directory's name, so `mise-tasks/test/_default` is `test`.

For this layout:

```text
mise-tasks
├── build
├── db
│   └── migrate.sh
└── test
    ├── _default
    ├── integration
    └── units
```

`mise tasks --extended` lists:

```text
Name              Aliases  Source                         Description
build                      ./mise-tasks/build
db:migrate                 ./mise-tasks/db/migrate.sh
test                       ./mise-tasks/test/_default
test:integration           ./mise-tasks/test/integration
test:units                 ./mise-tasks/test/units
```

`mise run 'test:*'` runs `test:integration` and `test:units`; see
[Wildcards](/tasks/running-tasks.html#wildcards).

## Configure with `#MISE` comments

Add `#MISE` comments near the top of the script to set
[task properties](/tasks/task-configuration.html):

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE description="Build the CLI"
#MISE alias="b"
#MISE sources=["Cargo.toml", "src/**/*.rs"]
#MISE outputs=["target/debug/mycli"]
#MISE env={RUST_BACKTRACE = "1"}
#MISE depends=["lint", "test"]
#MISE tools={rust="1.90"}
cargo build
```

`mise run build` or its alias `mise run b` runs this file.

The comment marker can be `#`, `//` (for JavaScript, TypeScript, or Go) or `::`
(for batch files), and whitespace may follow it. The keyword can also be
bracketed. `#MISE`, `# MISE`, `// MISE`, and `# [MISE]` all work, so a
formatter that adds a space after `#` does not break the header. To disable a
header line, change the keyword, for example to `# NOMISE`.

Each `#MISE` line holds TOML. Headers accept most task properties, but not
`timeout`, `vars`, or the sandbox keys (`deny_*` and `allow_*`); mise warns
about those and ignores them. To set one of them, or to configure a script you
cannot edit, add a `[tasks.<name>]` block for the script to `mise.toml`; see
[Configuring file tasks from TOML](/tasks/task-discovery.html#configuring-file-tasks-from-toml).

### Multi-line values

An array or inline table can span several lines as long as every line keeps the
`#MISE` prefix, which keeps long `depends` and `sources` lists readable:

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE description="Build the CLI"
#MISE depends=[
#MISE   "lint",
#MISE   "test",
#MISE ]
#MISE sources=[
#MISE   "Cargo.toml",
#MISE   "src/**/*.rs",
#MISE ]
cargo build
```

Dotted keys build a table one line at a time, without braces:

```bash
#MISE tools.node="24"
#MISE tools.python="3.13"
```

### Extend a task template

`extends` names a [task template](/tasks/templates.html), so several file tasks
can share one set of tools, environment variables, and arguments:

```toml [mise.toml]
[task_templates.rust]
tools = { rust = "1.90" }
env = { RUST_BACKTRACE = "1" }
```

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE extends="rust"
#MISE description="Build the CLI"
cargo build
```

The script is the task's command, so a file task ignores the template's `run`.
It inherits everything else by the template
[merge rules](/tasks/templates.html#merge-semantics).

## Shebang

The shebang selects the interpreter. It is optional on Linux and macOS, but
Windows needs it for files without an executable extension (see
[Windows](#windows)). Use it to write tasks in any language:

::: code-group

```js [node]
#!/usr/bin/env node
//MISE description="Hello, World in Node.js"

console.log("Hello, World!");
```

```python [python]
#!/usr/bin/env python
#MISE description="Hello, World in Python"

print('Hello, World!')
```

```ts [deno]
#!/usr/bin/env -S deno run
//MISE description="Hello, World in Deno"

console.log("Hello, World!");
```

```powershell [powershell]
#!/usr/bin/env pwsh
#MISE description="Hello, World in PowerShell"

$current_directory = Get-Location
Write-Host "Hello from PowerShell, current directory is $current_directory"
```

:::

## Windows

Windows has no execute bit. A file there is a task if its extension is listed in
[`windows_executable_extensions`](/configuration/settings.html#windows_executable_extensions),
such as `.cmd` or `.ps1`, or if it starts with a shebang. Windows does not
implement shebangs itself: mise reads the line and starts the interpreter. A
file with neither is not a task on Windows, even though it works on Linux and
macOS:

```bash [mise-tasks/build]
# no shebang and no extension: not a task on Windows
cargo build
```

Add `#!/usr/bin/env bash` to scripts you share. It changes nothing on the other
platforms.

### PowerShell scripts without `.ps1`

Windows PowerShell runs only files whose names end in `.ps1`, so mise runs a
`#!/usr/bin/env pwsh` task from a temporary `.ps1` copy and removes the copy
when the task finishes. `$PSScriptRoot` and `$PSCommandPath` name the copy;
the working directory, `$args`, and the environment are unchanged.

To find files next to the task, read `$env:MISE_TASK_DIR`, which names the
directory of the task file on every platform (see
[Task environment](/tasks/running-tasks.html#task-environment)). Or give the
task a `.ps1` extension, which runs in place.

### One task, two scripts

A file task has no equivalent of a TOML task's
[`run_windows`](/tasks/task-configuration.html#run-windows). Instead, put a
POSIX script and a Windows script with the same stem in the same directory:

```text
mise-tasks/
  build.sh       # #!/usr/bin/env bash
  build.ps1      # the Windows version
```

On Windows, mise runs `build.ps1` as `build` and drops the POSIX script. On
Linux and macOS, the `.ps1` has no execute bit, so only `build.sh` is a task.
`mise run build` picks the right script on each platform. The POSIX script is
any file without one of the `windows_executable_extensions`, so a script with
no extension, such as `build`, pairs the same way.

Keep the `.ps1` non-executable on Linux and macOS. If it is executable, it is a
task there too: next to `build.sh` it also answers to `build`, so
`mise run build` runs both scripts. When there is more than one Windows
candidate, such as `build.ps1` and `build.cmd`, mise does not choose between
them on Windows and keeps all three files as tasks; run one by its full name,
such as `mise run build.cmd`.

To give the scripts unrelated names, or to choose explicitly, use a
[TOML task](/tasks/toml-tasks.html) that calls them:

```mise-toml [mise.toml]
[tasks.build]
run = "./scripts/build.sh"
run_windows = "pwsh -File ./scripts/windows-build.ps1"
```

The Windows command calls `pwsh` explicitly because
[`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
is `cmd /c` by default, and cmd does not start a `.ps1` file by itself.

## Create or edit a file task

`mise tasks edit build` opens `build` in `$EDITOR`. If the task does not exist,
mise first creates an executable script with a Bash shebang in the first task
directory that exists, or in `mise-tasks/` when there is none.
[`mise tasks add --file`](/cli/tasks/add.html) creates a file task from a
command:

```sh
mise tasks add --file hello -- echo hello
```

## Arguments {#arguments}

Declare arguments with `#USAGE` comments. mise parses them, passes each value
to the script as a `usage_*` environment variable, and provides `--help` and
shell completions:

```bash [mise-tasks/build]
#!/usr/bin/env bash
set -e

#USAGE flag "-c --clean" help="Clean the build directory before building"
#USAGE flag "-p --profile <profile>" help="Build with the specified profile" default="dev" {
#USAGE   choices "dev" "release"
#USAGE }
#USAGE flag "-u --user <user>" help="The user to build for"
#USAGE complete "user" run="mycli users"
#USAGE arg "<target>" help="The target to build"

if [ "${usage_clean:-false}" = "true" ]; then
  cargo clean
fi

cargo build --profile "${usage_profile?}" --target "${usage_target?}"
```

With mise's [shell completions](/shell-setup.html) enabled,
`mise run build --profile <Tab>` offers `dev` and `release`, and
`mise run build --user <Tab>` offers the output of `mycli users`. The separate
`usage` CLI is not needed. `mise run build --help` prints help for the task,
and [`mise generate task-docs`](/cli/generate/task-docs.html) renders Markdown
documentation from the same spec.

Put mise's own flags before the task name, as in
`mise run --dry-run build --profile release x86_64-unknown-linux-gnu`.
Everything after the task name goes to the task.

[Task arguments](/tasks/task-arguments.html) covers the spec syntax,
[reading values in Bash](/tasks/task-arguments.html#bash-variable-expansion), and
[environment variable backing](/tasks/task-arguments.html#environment-variable-backing).

If completions or `--help` do not reflect your spec, run
`mise tasks validate` or the task itself. An invalid spec produces a warning
such as `invalid usage spec in task file mise-tasks/build`, followed by the line
that failed to parse.

### A Node.js task with arguments

`//USAGE` comments declare arguments in JavaScript; mise parses them the same
way as `#USAGE` lines:

```js [mise-tasks/greet]
#!/usr/bin/env node
//MISE description="Write a greeting to a file"
//USAGE flag "-f --force" help="Overwrite existing <file>"
//USAGE flag "-u --user <user>" help="User to run as"
//USAGE arg "<output_file>" help="The file to write" default="file.txt" {
//USAGE   choices "greeting.txt" "file.txt"
//USAGE }

const fs = require("fs");

const { usage_user, usage_force, usage_output_file } = process.env;

if (usage_force === "true") {
  fs.rmSync(usage_output_file, { force: true });
}

const user = usage_user ?? "world";
fs.appendFileSync(usage_output_file, `Hello, ${user}\n`);
console.log(`Greeting written to ${usage_output_file}`);
```

```sh
mise run greet greeting.txt --user Alice
# Greeting written to greeting.txt
```

mise rejects a value outside the choices before the script starts:

```sh
mise run greet invalid.txt --user Alice
```

```text
mise ERROR failed to validate task greet
mise ERROR Invalid choice for arg output_file: invalid.txt, expected one of greeting.txt, file.txt
```

With completions enabled, `mise run greet <Tab>` offers `greeting.txt` and
`file.txt`.

## Working directory

File tasks run from the [config root](/configuration.html#config-root), the
project directory, such as `~/proj` for both `~/proj/mise.toml` and
`~/proj/.config/mise.toml`. Add <span v-pre>`#MISE dir="{{cwd}}"`</span> to run
from the directory where you called mise:

```bash
#!/usr/bin/env bash
#MISE dir="{{cwd}}"
```

The directory where you called mise is also in `MISE_ORIGINAL_CWD`, and
`MISE_PROJECT_ROOT` and the other [task variables](/tasks/running-tasks.html#task-environment)
locate the project from any directory:

```bash
#!/usr/bin/env bash
cd "$MISE_ORIGINAL_CWD"
```

## Run a script by path

`mise run ./scripts/build.sh` runs any executable script as a task, with its
`#MISE` and `#USAGE` headers, even outside a task directory:

```sh
mise run ./path/to/script.sh
```

The path must start with `/`, `./`, or `../` (on Windows, backslash forms such
as `.\`, `..\`, or `C:\` also work) and the file must exist. Otherwise mise
looks the argument up as a task name. On Linux and macOS the file must also be
executable. In a terminal mise offers to mark it executable; otherwise it stops
with `` `./build.sh` is not executable. Run: chmod +x ./build.sh ``.

Inside a project, mise checks that the file exists relative to the current
directory, but resolves a relative path from the project's config root when it
runs the script. Run it from the project root, or pass an absolute path such as
`"$PWD/build.sh"` from a subdirectory. The script runs in the config root, like
other tasks.
