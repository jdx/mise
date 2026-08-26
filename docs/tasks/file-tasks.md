# File Tasks

In addition to defining tasks through the configuration, they can also be defined as standalone script files in one of the following directories:

- `mise-tasks/:task_name`
- `.mise-tasks/:task_name`
- `mise/tasks/:task_name`
- `.mise/tasks/:task_name`
- `.config/mise/tasks/:task_name`

These are the default file-task directories. If [`task_config.includes`](/tasks/task-configuration.html#task_config.includes)
is set for the current config scope, mise searches only the paths listed there instead.

Here is an example of a file task that builds a Rust CLI:

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE description="Build the CLI"
cargo build
```

::: tip Important
Ensure that the file is executable, otherwise mise will not be able to detect it.

```shell
chmod +x mise-tasks/build
```

On Windows there is no permission bit to set, and `chmod` is not the answer there — see
[Windows](#windows) for what makes a file task detectable instead.
:::

Having the code in a bash file and not TOML helps make it work
better in editors since they can do syntax highlighting and linting more easily.

They also still work great for non-mise users—though
of course they'll need to find a different way to install their dev tools the tasks might use.

## Task Configuration

All configuration options can be found here [task configuration](/tasks/task-configuration)
You can provide additional configuration for file tasks by adding `#MISE` comments at the top of the file.

```bash
#MISE description="Build the CLI"
#MISE alias="b"
#MISE sources=["Cargo.toml", "src/**/*.rs"]
#MISE outputs=["target/debug/mycli"]
#MISE env={RUST_BACKTRACE = "1"}
#MISE depends=["lint", "test"]
#MISE tools={rust="1.50.0"}
```

Assuming that file was located in `mise-tasks/build`, it can then be run with `mise run build` (or with its alias: `mise run b`).

### Multi-line values

Each `#MISE` line is TOML. An array or inline table may be split across several
lines as long as every line keeps the `#MISE` prefix, which keeps long
`depends`/`sources` lists readable:

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

A table can also be built up by repeating the prefix with dotted keys, which
avoids the surrounding braces entirely:

```bash
#MISE tools.node="20"
#MISE tools.python="3.11"
```

Mise provides file tasks with project context variables such as
`MISE_PROJECT_ROOT`, which identifies the project root regardless of the
directory from which the task is invoked. See [Tasks](/tasks/#environment-variables-passed-to-tasks)
for the complete list of variables.

:::tip
Beware of formatters changing `#MISE` to `# MISE`.
It's intentionally ignored by mise to avoid unintentional configuration.
To workaround this, use the alternative: `# [MISE]`.
:::

## Shebang

The shebang line is optional, but if it is present, it will be used to determine the shell to run the script with.
You can also use it to run the script with various programming languages.

::: code-group

```js [node]
#!/usr/bin/env node
//MISE description="Hello, World in Node.js"

console.log("Hello, World!");
```

```python
#!/usr/bin/env python
#MISE description="Hello, World in Python"

print('Hello, World!')
```

```ts [deno]
#!/usr/bin/env -S deno run --allow-env
//MISE description="Hello, World in Deno"

console.log(`PATH, ${Deno.env.get("PATH")}`);
```

```powershell [powershell]
#!/usr/bin/env pwsh
#MISE description="Hello, World in PowerShell"

$current_directory = Get-Location
Write-Host "Hello from PowerShell, current directory is $current_directory"
```

:::

## Windows

Windows has no execute permission for mise to look at, so it decides whether a file is a task a
different way. A file is a task if **either** holds:

- its extension is one of [`windows_executable_extensions`](/configuration/settings.html#windows_executable_extensions)
  — by default `exe`, `bat`, `cmd`, `com`, `ps1`, `vbs`
- it starts with a **shebang**

The two answer different questions. The extension means Windows itself can run the file; the shebang
means mise can work out an interpreter for it. Windows does not implement shebangs — mise reads the
line and starts the interpreter itself — which is why a `.sh` script, or a file with no extension at
all, is still a task there as long as it has one.

The practical consequence is that a file with **neither** is invisible on Windows even though it
works on Linux and macOS:

```bash [mise-tasks/build]
# no shebang, no extension -> not a task on Windows
cargo build
```

Adding `#!/usr/bin/env bash` is usually all it takes, and it costs nothing on the other platforms.

### PowerShell tasks with no `.ps1` extension

Windows PowerShell refuses to open a script whose name does not end in `.ps1` — a rule the Linux
and macOS builds do not have. So that a `#!/usr/bin/env pwsh` task behaves the same everywhere,
mise runs it from a `.ps1` copy in the temp directory and removes the copy when the task finishes.

Only the script's view of its own location changes: `$PSScriptRoot` and `$PSCommandPath` name the
copy rather than the task file. The working directory, `$args`, and the environment are untouched.

A task that needs to find files next to itself has two ways out, and the first works everywhere:

- Read [`MISE_TASK_DIR`](/tasks/#environment-variables-passed-to-tasks), which names the directory
  the task file is in. mise sets it from the task rather than from whatever is executing, so the
  copy does not move it — and it reads the same on Linux and macOS, where nothing is copied at all.
- Give the task a `.ps1` extension, which is run in place.

### Writing one task for both platforms

File tasks have no equivalent of a TOML task's
[`run_windows`](/tasks/task-configuration.html#run-windows) — the script _is_ the command, so there
is nowhere to put a second one. Write the two scripts side by side instead, giving the Windows one an
executable extension:

```
mise-tasks/
  build.sh       # #!/usr/bin/env bash
  build.ps1      # the Windows version
```

The two have to share a directory and a stem — that pairing is what makes them one task rather than
two that happen to be named alike.

On Windows mise prefers the native script: `build.ps1` answers to `build`, and the POSIX one is
dropped. On Linux and macOS the `.ps1` has no execute permission, so only `build.sh` is found.
`mise run build` does the right thing on each.

The POSIX half is anything _without_ one of the
[`windows_executable_extensions`](/configuration/settings.html#windows_executable_extensions), so a
file with no extension at all works the same way:

```
mise-tasks/
  build          # #!/usr/bin/env bash
  build.ps1      # the Windows version
```

Marking the `.ps1` executable on Linux or macOS does not break that — it simply appears there as a
separate task called `build.ps1`, since the rename to `build` only happens on Windows.

If you would rather name the two files something unrelated, or say which is which explicitly, use a
[TOML task](/tasks/toml-tasks.html) that calls them:

```toml
[tasks.build]
run = "./scripts/build.sh"
run_windows = "pwsh -File ./scripts/windows-build.ps1"
```

Spelled out rather than `./scripts/windows-build.ps1`, because
[`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
defaults to `cmd /c`, and cmd will not start a `.ps1` on its own.

If there is more than one Windows candidate — say `build.ps1` _and_ `build.cmd` — mise cannot choose
between them, so it leaves everything alone: all three files stay, listed as `build.sh`, `build.ps1`
and `build.cmd`.

## Editing tasks

This script can be edited by running `mise tasks edit build` (using `$EDITOR`). If it doesn't exist it will be created.
This is convenient for quickly editing or creating new scripts.

## Task Grouping

File tasks in `mise-tasks`, `.mise/tasks`, `mise/tasks`, or `.config/mise/tasks` can be grouped into
sub-directories which will automatically apply prefixes to their names
when loaded.

**Example**: With a folder structure like below:

```text
mise-tasks
├── build
└── test
    ├── _default
    ├── integration
    └── units
```

Running `mise tasks` will give the below output:

```shellsession
$ mise tasks
Name              Description Source
build                         ./mise-tasks/build
test                          ./mise-tasks/test/_default
test:integration              ./mise-tasks/test/integration
test:units                    ./mise-tasks/test/units
```

## Arguments

::: tip
For comprehensive information about task arguments, see the dedicated [Task Arguments](/tasks/task-arguments) page.
:::

[usage](https://usage.jdx.dev) spec can be used within these files to provide argument parsing, autocompletion,
documentation when running mise and can be exported to markdown. Essentially this turns tasks into
fully-fledged CLIs.

:::tip
The separate `usage` CLI is not required to execute or complete mise tasks with a usage spec.
Task completions work when mise's shell completion script is installed and enabled.
:::

### Example file task with arguments

Here is an example of a file task that builds a Rust CLI using some of the features of usage:

```bash [mise-tasks/build]
#!/usr/bin/env bash
set -e

#USAGE flag "-c --clean" help="Clean the build directory before building"
#USAGE flag "-p --profile <profile>" help="Build with the specified profile" default="debug" {
#USAGE   choices "debug" "release"
#USAGE }
#USAGE flag "-u --user <user>" help="The user to build for"
#USAGE complete "user" run="mycli users"
#USAGE arg "<target>" help="The target to build"

if [ "${usage_clean:-false}" = "true" ]; then
  cargo clean
fi

cargo build --profile "${usage_profile?}" --target "${usage_target?}"
```

::: tip
For details on bash parameter expansion patterns like `${var?}`, `${var:-default}`, and `${var:+value}`, see [Bash Variable Expansion for Usage Variables](/tasks/task-arguments#bash-variable-expansion).
:::

With mise's shell completions enabled, this example provides the following task completions:

- `mise run -- build --profile <tab><tab>`
  will show `debug` and `release` as options.
- The `--user` flag will also show completions generated by the output of `mycli users`.
- Note: Use `--` to separate mise flags from task arguments: `mise run -- build --profile release <target>`

(Note that cli and markdown help for tasks is not yet implemented in mise as of this writing but that is planned.)

:::tip
If you don't get any autocomplete suggestions, use the `-v` (verbose) flag to see what's going on.
For example, if you use `mise run build -v` and have an invalid `usage` spec, you will see an error message such as `DEBUG failed to parse task file with usage`
:::

### Environment variable backing

Arguments and flags can be backed by environment variables with `env="..."`.
The precedence order is CLI argument, environment variable, then default value:

```bash [.mise/tasks/deploy]
#!/usr/bin/env bash
#MISE description="Deploy application"
#USAGE arg "[environment]" env="DEPLOY_ENV" default="development"
#USAGE flag "--region <region>" env="AWS_REGION" default="us-east-1"

echo "Deploying to ${usage_environment} in ${usage_region}"
```

This lets the same file task work with either explicit arguments or the
environment of the calling shell:

```shell
DEPLOY_ENV=staging AWS_REGION=us-west-2 mise run deploy
```

See [Environment Variable Backing](https://mise.jdx.dev/tasks/task-arguments.html#environment-variable-backing)
for more details.

### Example of a NodeJS file task with arguments

Here is how you can use [usage](https://usage.jdx.dev/cli/scripts#usage-scripts) to parse arguments in a Node.js script:

```js [mise-tasks/greet]
#!/usr/bin/env -S node
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

Run it with:

```shell
mise run greet greeting.txt --user Alice
# Greeting written to greeting.txt
```

If you pass an invalid argument, you will get an error message:

```shell
mise run greet invalid.txt --user Alice
# [greet] ERROR
#   0: Invalid choice for arg output_file: invalid.txt, expected one of greeting.txt, file.txt
```

Autocomplete will show the available choices for the `output_file` argument when mise's shell completions are enabled.

```shell
mise run greet <TAB>
# > greeting.txt
#   file.txt
```

## CWD

mise sets the current working directory to the directory of `mise.toml` before running tasks.
This can be overridden by setting <span v-pre>`dir="{{cwd}}"`</span> in the task header:

```bash
#!/usr/bin/env bash
#MISE dir="{{cwd}}"
```

Also, the original working directory is available in the `MISE_ORIGINAL_CWD` environment variable:

```bash
#!/usr/bin/env bash
cd "$MISE_ORIGINAL_CWD"
```

## Running tasks directly

Tasks don't need to be configured as part of a config, you can just run them directly by passing the path to the script:

```bash
mise run ./path/to/script.sh
```

Note that the path must start with `/` or `./` to be considered a file path. (On Windows it can be `C:\` or `.\`)
