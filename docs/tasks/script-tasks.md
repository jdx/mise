# Script Tasks

In addition to defining tasks through the configuration, they can also be defined as standalone script files in
`.mise/tasks/:task_name` such as the following build script for cargo:

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
