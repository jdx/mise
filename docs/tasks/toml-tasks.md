# TOML-based Tasks

Tasks can be defined in `mise.toml` files in different ways:

```toml
[tasks.cleancache]
run = "rm -rf .cache"
hide = true # hide this task from the list

[tasks.clean]
depends = ['cleancache']
run = "cargo clean" # runs as a shell command

[tasks.build]
description = 'Build the CLI'
run = "cargo build"
alias = 'b' # `mise run b`

[tasks.test]
description = 'Run automated tests'
# multiple commands are run in series
run = [
    'cargo test',
    './scripts/test-e2e.sh',
]
dir = "{{cwd}}" # run in user's cwd, default is the project's base directory

[tasks.lint]
description = 'Lint with clippy'
env = { RUST_BACKTRACE = '1' } # env vars for the script
# you can specify a multiline script instead of individual commands
run = """
#!/usr/bin/env bash
cargo clippy
"""

[tasks.ci] # only dependencies to be run
description = 'Run CI tasks'
depends = ['build', 'lint', 'test']

[tasks.release]
description = 'Cut a new release'
file = 'scripts/release.sh' # execute an external script
```

## run

Provide the script to run. Can be a single command or an array of commands:

```toml
[tasks.test]
run = 'cargo test'
```

Commands are run in series. If a command fails, the task will stop and the remaining commands will not run.

```toml
[tasks.test]
run = [
    'cargo test',
    './scripts/test-e2e.sh',
]
```

### run_windows

You can specify an alternate command to run on Windows by using the `run_windows` key:

```toml
[tasks.test]
run = 'cargo test'
run_windows = 'cargo test --features windows'
```

## dir

Tasks are executed with `cwd` set to the directory containing `mise.toml`. You can use the directory
from where the task was run with `dir = "{{cwd}}"`:

```toml
[tasks.test]
run = 'cargo test'
dir = "{{cwd}}"
```

Also, `MISE_ORIGINAL_CWD` is set to the original working directory and will be passed to the task.

## description

You can add a description to a task:

```toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
```

The description will for example be displayed when running [`mise tasks ls`](/cli/tasks/ls.html) or [`mise run`](/cli/run.html)` with no arguments.

```shell
❯ mise run
Tasks
# Select a tasks to run
# > build  Build the CLI
#   test   Run the tests
```

## depends

You can specify dependencies for a task. Dependencies are run before the task itself. If a dependency fails, the task will not run.

```toml
[tasks.build]
run = 'cargo build'

[tasks.test]
depends = ['build']
```

## wait_for

If a task must run after another task, you can use `wait_for`:

```toml
[tasks.clean]
run = 'cargo clean'

[tasks.build]
run = 'cargo build'
wait_for = ['clean']
```

## hide

You can hide a task from the list of available tasks by setting `hide = true`:

```toml
[tasks.cleancache]
run = "rm -rf .cache"
hide = true # hide this task from mise tasks ls / mise run
```

## alias

You can specify an alias for a task. This alias can be used to run the task:

```toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
alias = 'b' # `mise run b`
```

## env

You can specify environment variables for a task:

```toml
[tasks.lint]
description = 'Lint with clippy'
env = { RUST_BACKTRACE = '1' } # env vars for the script
# you can specify a multiline script instead of individual commands
run = """
#!/usr/bin/env bash
cargo clippy
"""
```

## sources / outputs

If you want to skip executing a task if certain files haven't changed (up-to-date), you should specify `sources` and `outputs`:

```toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
sources = ['Cargo.toml', 'src/**/*.rs'] # skip running if these files haven't changed
outputs = ['target/debug/mycli']
```

You can use `sources` alone if with [`mise watch`](/cli/watch.html) to run the task when the sources change.

## shell / shebang

Tasks are executed with `set -e` (`set -o erropt`) if the shell is `sh`, `bash`, or `zsh`. This means that the script
will exit if any command fails. You can disable this by running `set +e` in the script.

```toml
[tasks.echo]
run = '''
set +e
cd /nonexistent
echo "This will not fail the task"
'''
```

You can specify a `shell` command to run the script with (default is [`sh -c`](/configuration/settings.html#unix_default_inline_shell_args) or [`cmd /c`](/configuration/settings.html#windows_default_inline_shell_args)):

```toml
[tasks.lint]
shell = 'bash -c'
run = "cargo clippy"
```

or use a shebang:

```toml
[tasks.lint]
run = """
#!/usr/bin/env bash
cargo clippy
"""
```

By using a `shebang` (or `shell`), you can run tasks in different languages (e.g., Python, Node.js, Ruby, etc.):

::: code-group

```toml [python]
[tools]
python = 'latest'

[tasks.python_task]
run = '''
#!/usr/bin/env python
for i in range(10):
    print(i)
'''
```

```toml [node]
[tools]
node = 'lts'

[tasks.node_task]
shell = 'node -e'
run = [
  "console.log('First line')",
  "console.log('Second line')",
]
```

```toml [bun]
[tools]
bun = 'latest'

[tasks.bun_shell]
description = "https://bun.sh/docs/runtime/shell"
run = """
#!/usr/bin/env bun

import { $ } from "bun";
const response = await fetch("https://example.com");
await $`cat < ${response} | wc -c`; // 1256
"""
```

```toml [deno]
[tools]
deno = 'latest'

[tasks.deno_task]
description = "A more complex task using Deno imports"
run = '''
#!/usr/bin/env -S deno run
import ProgressBar from "jsr:@deno-library/progress";
import { delay } from "jsr:@std/async";

if (!confirm('Start download?')) {
    Deno.exit(1);
}

const progress = new ProgressBar({ title:  "downloading:", total: 100 });
let completed = 0;
async function download() {
  while (completed <= 100) {
    await progress.render(completed++);
    await delay(10);
  }
}
await download();
'''
# ❯ mise run deno_task
# [download_task] $ import ProgressBar from "jsr:@deno-library/progress";
# Start download? [y/N] y
# downloading: ...
```

```toml [ruby]
[tools]
ruby = 'latest'

[tasks.ruby_task]
run = """
#!/usr/bin/env ruby
puts 'Hello, ruby!'
"""
```

:::

::: details What's a shebang? What's the difference between `#!/usr/bin/env` and `#!/usr/bin/env -S`

A shebang is the character sequence `#!` at the beginning of a script file that tells the system which program should be used to interpret/execute the script.
The [env command](https://manpages.ubuntu.com/manpages/jammy/man1/env.1.html) comes from GNU Coreutils. `mise` does not use `env` but will behave similarly.

For example, `#!/usr/bin/env python` will run the script with the Python interpreter found in the `PATH`.

The `-S` flag allows passing multiple arguments to the interpreter.
It treats the rest of the line as a single argument string to be split.

This is useful when you need to specify interpreter flags or options.
Example: `#!/usr/bin/env -S python -u` will run Python with unbuffered output.

:::

## file

You can specify a file to run as a task:

```toml
[tasks.release]
description = 'Cut a new release'
file = 'scripts/release.sh' # execute an external script
```

### Remote tasks

Task files can be fetched via http:

```toml
[tasks.build]
file = "https://example.com/build.sh"
```

Currently, they're fetched everytime they're executed, but we may add some cache support later.
This could be extended with other protocols like mentioned in [this ticket](https://github.com/jdx/mise/issues/2488) if there were interest.

## raw

Stdin is not read by default. To enable this, set `raw = true` on the task that needs it. This will prevent
it running in parallel with any other task. A RWMutex will get a write lock in this case.

```toml
[tasks.build]
raw = true
run = 'echo "Enter your name:"; read name; echo "Hello, $name!"'
```

## quiet

Set `quiet = false` to supress mise additional output.

## Arguments

By default, arguments are passed to the last script in the `run` array. So if a task was defined as:

```toml
[tasks.test]
run = ['cargo test', './scripts/test-e2e.sh']
```

Then running `mise run test foo bar` will pass `foo bar` to `./scripts/test-e2e.sh` but not to
`cargo test`.

You can also define arguments using templates:

```toml
[tasks.test]
run = [
    'cargo test {{arg(name="cargo_test_args", var=true)}}',
    './scripts/test-e2e.sh {{option(name="e2e_args")}}',
]
```

Then running `mise run test foo bar` will pass `foo bar` to `cargo test`.
`mise run test --e2e-args baz` will pass `baz` to `./scripts/test-e2e.sh`.
If any arguments are defined with templates then mise will not pass the arguments to the last script
in the `run` array.

:::tip
Using templates to define arguments will make them work with completion and help messages.
:::

### Positional Arguments

These are defined in scripts with <span v-pre>`{{arg()}}`</span>. They are used for positional
arguments where the order matters.

Example:

```toml
[tasks.test]
run = 'cargo test {{arg(name="file")}}'
# execute: mise run test my-test-file
# runs: cargo test my-test-file
```

- `i`: The index of the argument. This can be used to specify the order of arguments. Defaults to
  the order they're defined in the scripts.
- `name`: The name of the argument. This is used for help/error messages.
- `var`: If `true`, multiple arguments can be passed.
- `default`: The default value if the argument is not provided.

### Options

These are defined in scripts with <span v-pre>`{{option()}}`</span>. They are used for named
arguments where the order doesn't matter.

Example:

```toml
[tasks.test]
run = 'cargo test {{option(name="file")}}'
# execute: mise run test --file my-test-file
# runs: cargo test my-test-file
```

- `name`: The name of the argument. This is used for help/error messages.
- `var`: If `true`, multiple values can be passed.
- `default`: The default value if the option is not provided.

### Flags

Flags are like options except they don't take values. They are defined in scripts with <span v-pre>
`{{flag()}}`</span>.

Examples:

```toml
[tasks.echo]
run = 'echo {{flag(name=("myflag")}}'
# execute: mise run echo --myflag
# runs: echo true
```

```toml
[tasks.maybeClean]
run = """
if [ '{{flag(name='clean')}}' = 'true' ]; then
  echo 'cleaning'
fi
"""
# execute: mise run maybeClean --clean
# runs: echo cleaning
```

- `name`: The name of the flag. This is used for help/error messages.

The value will be `true` if the flag is passed, and `false` otherwise.
