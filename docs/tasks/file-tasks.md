# File Tasks

In addition to defining tasks through the configuration, they can also be defined as standalone script files in one of the following directories:

- `mise-tasks/:task_name`
- `.mise-tasks/:task_name`
- `mise/tasks/:task_name`
- `.mise/tasks/:task_name`
- `.config/mise/tasks/:task_name`

Note that you can configure directories using the [task_config](/tasks/task-configuration.html#task-config-options) section.

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
#MISE description="Hello, World in Powershell"

$current_directory = Get-Location
Write-Host "Hello from Powershell, current directory is $current_directory"
```

:::

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
The `usage` CLI is not required to execute mise tasks with the usage spec.
However, for completions to work, the `usage` CLI must be installed and available in the PATH.
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

If you have installed `usage`, completions will be enabled for your task. In this example,

- `mise run -- build --profile <tab><tab>`
  will show `debug` and `release` as options.
- The `--user` flag will also show completions generated by the output of `mycli users`.
- Note: Use `--` to separate mise flags from task arguments: `mise run -- build --profile release <target>`

(Note that cli and markdown help for tasks is not yet implemented in mise as of this writing but that is planned.)

:::tip
If you don't get any autocomplete suggestions, use the `-v` (verbose) flag to see what's going on.
For example, if you use `mise run build -v` and have an invalid `usage` spec, you will see an error message such as `DEBUG failed to parse task file with usage`
:::

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

Autocomplete will show the available choices for the `output_file` argument if `usage` is installed.

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
