# Script Tasks

Tasks can be defined in 2 ways, either as standalone script files in `.rtx/tasks/:task_name` such as the following build script
for cargo:

```bash
#!/usr/bin/env bash
# rtx:description "Build the CLI"
cargo build
```

:::tip
The `rtx:description` comment is optional but recommended. It will be used in the output of `rtx tasks`.
The other configuration for "script" tasks is supported in this format so you can specify things like the
following-note that this is parsed a TOML table:

```bash
# rtx alias="b"
# rtx sources=["Cargo.toml", "src/**/*.rs"]
# rtx outputs=["target/debug/mycli"]
# rtx env={RUST_BACKTRACE = "1"}
# rtx depends=["lint", "test"]
```

Assuming that file was located in `.rtx/tasks/build`, it can then be run with `rtx run build` (or with its alias: `rtx run b`).
This script can be edited with by running `rtx task edit build` (using $EDITOR). If it doesn't exist it will be created.
These are convenient for quickly making new scripts. Having the code in a bash file and not TOML helps make it work
better in editors since they can do syntax highlighting and linting more easily. They also still work great for non-rtx users—though
of course they'll need to find a different way to install their dev tools the tasks might use.
