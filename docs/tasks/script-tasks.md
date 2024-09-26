# Script Tasks

In addition to defining tasks through the configuration, they can also be defined as standalone script files in one of the following directories:

* `mise-tasks/:task_name`
* `.mise-tasks/:task_name`
* `mise/tasks/:task_name`
* `.mise/tasks/:task_name`
* `.config/mise/tasks/:task_name`

Here is an example of a script task that builds a Rust CLI:

```bash
#!/usr/bin/env bash
# mise description="Build the CLI"
cargo build
```

Ensure that the file is executable, otherwise mise will not be able to detect it.

:::tip
The `mise:description` comment is optional but recommended. It will be used in the output of `mise tasks`.
The other configuration for "script" tasks is supported in this format so you can specify things like the
following-note that this is parsed a TOML table:

```bash
# mise alias="b"
# mise sources=["Cargo.toml", "src/**/*.rs"]
# mise outputs=["target/debug/mycli"]
# mise env={RUST_BACKTRACE = "1"}
# mise depends=["lint", "test"]
```

Assuming that file was located in `.mise/tasks/build`, it can then be run with `mise run build` (or with its alias: `mise run b`).
This script can be edited with by running `mise task edit build` (using $EDITOR). If it doesn't exist it will be created.
These are convenient for quickly making new scripts. Having the code in a bash file and not TOML helps make it work
better in editors since they can do syntax highlighting and linting more easily. They also still work great for non-mise users—though
of course they'll need to find a different way to install their dev tools the tasks might use.
:::

## Task Grouping

Script tasks in `.mise/tasks`, `mise/tasks`, or `.config/mise/tasks` can be grouped into
sub-directories which will automatically apply prefixes to their names
when loaded.

### Example

With a folder structure like below:

```text
.mise
└── tasks
    ├── build
    └── test
        ├── integration
        └── units
```

Running `mise tasks` will give the below output:

```text
$ mise tasks
Name              Description Source
build                         .../.mise/tasks/build
test:integration              .../.mise/tasks/test/integration
test:units                    .../.mise/tasks/test/units
```

### Argument parsing with usage

[usage](https://usage.jdx.dev) spec can be used within these files to provide argument parsing, autocompletion,
documentation when running mise and can be exported to markdown. Essentially this turns tasks into
fully-fledged CLIs.

Here is an example of a script task that builds a Rust CLI using some of the features of usage:

```bash
#!/usr/bin/env -S usage bash
set -e

#USAGE flag "-c --clean" help="Clean the build directory before building"
#USAGE flag "-p --profile <profile>" help="Build with the specified profile" 
#USAGE flag "-u --user <user>" help="The user to build for"
#USAGE complete "user" run="mycli users"
#USAGE arg "<target>" help="The target to build" 

if [ "$usage_clean" = "true" ]; then
  cargo clean
fi

cargo build --profile "${usage_profile:-debug}" --target "$usage_target"
```

(Note that cli and markdown help for tasks is not yet implemented in mise as of this writing but that is planned.)
