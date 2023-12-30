# TOML-based Tasks

Tasks can also be defined in `.rtx.toml` files in different ways. This is a more "traditional" method of defining tasks:

```toml
tasks.clean = 'cargo clean && rm -rf .cache' # runs as a shell command

[tasks.build]
description = 'Build the CLI'
run = "cargo build"
alias = 'b' # `rtx run b`

[tasks.test]
description = 'Run automated tests'
run = [ # multiple commands are run in series
    'cargo test',
    './scripts/test-e2e.sh',
]
dir = "{{cwd}}" # run in user's cwd, default is the project's base directory

[tasks.lint]
description = 'Lint with clippy'
env = {RUST_BACKTRACE = '1'} # env vars for the script
# you can specify a multiline script instead of individual commands
run = """
#!/usr/bin/env bash
cargo clippy
"""

[task.ci] # only dependencies to be run
description = 'Run CI tasks'
depends = ['build', 'lint', 'test']

[tasks.release]
description = 'Cut a new release'
file = 'scripts/release.sh' # execute an external script
```
