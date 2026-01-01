# TOML-based Tasks

Tasks can be defined in `mise.toml` files in different ways. Trivial tasks can be written into a `[tasks]` section, while more detailed tasks each get their own section.

## Trivial task examples

```mise-toml [mise.toml]
build = "cargo build"
test = "cargo test"
lint = "cargo clippy"
```

## Detailed task examples

```mise-toml [mise.toml]
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
run = '''
#!/usr/bin/env bash
cargo clippy
'''

[tasks.ci] # only dependencies to be run
description = 'Run CI tasks'
depends = ['build', 'lint', 'test']

[tasks.release]
confirm = 'Are you sure you want to cut a new release?'
description = 'Cut a new release'
file = 'scripts/release.sh' # execute an external script
```

You can use [environment variables](/environments/) or [`vars`](/tasks/task-configuration.html#vars-options) to define common arguments:

```mise-toml [mise.toml]
[env]
VERBOSE_ARGS = '--verbose'

# Vars can be shared between tasks like environment variables,
# but they are not passed as environment variables to the scripts
[vars]
e2e_args = '--headless'

[tasks.test]
run = './scripts/test-e2e.sh {{vars.e2e_args}} $VERBOSE_ARGS'
```

## Adding tasks

You can edit the `mise.toml` file directly or using [`mise tasks add`](/cli/tasks/add)

```shell
mise tasks add pre-commit --depends "test" --depends "render" -- echo pre-commit
```

will add the following to `mise.toml`:

```shell
[tasks.pre-commit]
depends = ["test", "render"]
run = "echo pre-commit"
```

## Common options

For an exhaustive list, see [task configuration](/tasks/task-configuration).

### Run command

Provide the script to run. Can be a single command or an array of commands:

```mise-toml
[tasks.test]
run = 'cargo test'
```

Commands are run in series. If a command fails, the task will stop and the remaining commands will not run.

```mise-toml
[tasks.test]
run = [
    'cargo test',
    './scripts/test-e2e.sh',
]
```

You can specify an alternate command to run on Windows by using the `run_windows` key:

```mise-toml
[tasks.test]
run = 'cargo test'
run_windows = 'cargo test --features windows'
```

### Specifying which directory to use

The [`dir`](/tasks/task-configuration.html#dir) property determines the `cwd` in which the task is executed. You can use the directory
from where the task was run with <span v-pre>`dir = "{{cwd}}"`</span>:

```mise-toml
[tasks.test]
run = 'cargo test'
dir = "{{cwd}}"
```

Also, `MISE_ORIGINAL_CWD` is set to the original working directory and will be passed to the task.

### Adding a description and alias

You can add a description to a task and alias for a task.

```mise-toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
alias = 'b' # `mise run b`
```

- This alias can be used to run the task
- The description will be displayed when running [`mise tasks ls`](/cli/tasks/ls.html) or [`mise run`](/cli/run.html) with no arguments.

```shell
❯ mise run
Tasks
# Select a task to run
# > build  Build the CLI
#   test   Run the tests
```

### Dependencies

You can specify dependencies for a task. Dependencies are run before the task itself. If a dependency fails, the task will not run.

```mise-toml
[tasks.build]
run = 'cargo build'

[tasks.test]
depends = ['build']
```

There are other ways to specify dependencies, see [wait_for](/tasks/task-configuration.html#wait-for) and [depends_post](/tasks/task-configuration.html#depends-post)

### Environment variables

You can specify environment variables for a task:

```mise-toml
[tasks.lint]
description = 'Lint with clippy'
env = { RUST_BACKTRACE = '1' } # env vars for the script
# you can specify a multiline script instead of individual commands
run = '''
#!/usr/bin/env bash
cargo clippy
'''
```

### Sources / Outputs

If you want to skip executing a task if certain files haven't changed (up-to-date), you should specify `sources` and `outputs`:

```mise-toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
sources = ['Cargo.toml', 'src/**/*.rs'] # skip running if these files haven't changed
outputs = ['target/debug/mycli']
```

You can use `sources` alone if with [`mise watch`](/cli/watch.html) to run the task when the sources change.
You can use the [`task_source_files()`](../templates.md#task-source-files) function to get the resolved paths of a task's `sources` from within
its [template](../templates.md).

### Confirmation

A message to show before running the task. The user will be prompted to confirm before the task is run.

```mise-toml
[tasks.release]
confirm = 'Are you sure you want to cut a new release?'
description = 'Cut a new release'
file = 'scripts/release.sh'
```

## Specifying a shell or an interpreter {#shell-shebang}

Tasks are executed with `set -e` (`set -o erropt`) if the shell is `sh`, `bash`, or `zsh`. This means that the script
will exit if any command fails. You can disable this by running `set +e` in the script.

```mise-toml
[tasks.echo]
run = '''
set +e
cd /nonexistent
echo "This will not fail the task"
'''
```

You can specify a `shell` command to run the script with (default is [`sh -c`](/configuration/settings.html#unix_default_inline_shell_args) or [`cmd /c`](/configuration/settings.html#windows_default_inline_shell_args)):

```mise-toml
[tasks.lint]
shell = 'bash -c'
run = "cargo clippy"
```

or use a shebang:

```mise-toml
[tasks.lint]
run = '''
#!/usr/bin/env bash
cargo clippy
'''
```

By using a `shebang` (or `shell`), you can run tasks in different languages (e.g., Python, Node.js, Ruby, etc.):

::: code-group

```mise-toml [python]
[tools]
python = 'latest'

[tasks.python_task]
run = '''
#!/usr/bin/env python
for i in range(10):
    print(i)
'''
```

```mise-toml [python + uv]
[tools]
uv = 'latest'

[tasks.python_uv_task]
run = '''
#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests<3", "rich"]
# ///

import requests
from rich.pretty import pprint

resp = requests.get("https://peps.python.org/api/peps.json")
data = resp.json()
pprint([(k, v["title"]) for k, v in data.items()][:10])
'''
```

```mise-toml [node]
[tools]
node = 'lts'

[tasks.node_task]
shell = 'node -e'
run = [
  "console.log('First line')",
  "console.log('Second line')",
]
```

```mise-toml [bun]
[tools]
bun = 'latest'

[tasks.bun_shell]
description = "https://bun.sh/docs/runtime/shell"
run = '''
#!/usr/bin/env bun

import { $ } from "bun";
const response = await fetch("https://example.com");
await $`cat < ${response} | wc -c`; // 1256
'''
```

```mise-toml [deno]
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

```mise-toml [ruby]
[tools]
ruby = 'latest'

[tasks.ruby_task]
run = '''
#!/usr/bin/env ruby
puts 'Hello, ruby!'
'''
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

## Using a file or remote script

You can specify a file to run as a task:

```mise-toml
[tasks.release]
description = 'Cut a new release'
file = 'scripts/release.sh' # execute an external script
```

### Remote tasks

Task files can be fetched remotely with multiple protocols:

#### HTTP

```mise-toml
[tasks.build]
file = "https://example.com/build.sh"
```

Please note that the file will be downloaded and executed. Make sure you trust the source.

#### Git <Badge type="warning" text="experimental" />

::: code-group

```mise-toml [ssh]
[tasks.build]
file = "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0"
```

```mise-toml [https]
[tasks.build]
file = "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0"
```

:::

Url format must follow these patterns `git::<protocol>://<url>//<path>?<ref>`

Required fields:

- `protocol`: The git repository URL.
- `url`: The git repository URL.
- `path`: The path to the file in the repository.

Optional fields:

- `ref`: The git reference (branch, tag, commit).

#### Cache

Each task file is cached in the `MISE_CACHE_DIR` directory. If the file is updated, it will not be re-downloaded unless the cache is cleared.

:::tip
You can reset the cache by running `mise cache clear`.
:::

You can use the `MISE_TASK_REMOTE_NO_CACHE` environment variable to disable caching of remote tasks.

## Arguments

::: tip
For comprehensive information about task arguments, see the dedicated [Task Arguments](/tasks/task-arguments) page.
:::

By default, arguments are passed to the last script in the `run` array. So if a task was defined as:

```mise-toml
[tasks.test]
run = ['cargo test', './scripts/test-e2e.sh']
```

Then running `mise run test foo bar` will pass `foo bar` to `./scripts/test-e2e.sh` but not to
`cargo test`.

### Recommended: Using the Usage Field

The recommended way to define arguments is using the `usage` field:

```mise-toml
[tasks.test]
usage = '''
arg "<file>" help="Test file to run" default="all"
flag "--format <format>" help="Output format" default="text"
flag "-v --verbose" help="Enable verbose output"
'''
run = 'cargo test ${usage_file?} --format ${usage_format?}'
```

Arguments defined in the usage field are available as environment variables prefixed with `usage_`.

See the [Task Arguments](/tasks/task-arguments#usage-field) page for complete documentation.

### Tera Template Functions <Badge type="danger" text="deprecated" />

::: danger Deprecated - Removal in 2026.11.0
Using Tera template functions (`arg()`, `option()`, `flag()`) in run scripts is **deprecated** and will be **removed in mise 2026.11.0**. Versions >= 2026.5.0 will show a deprecation warning.

**Why it's being removed:**

- Template functions return empty strings during spec collection (two-pass parsing issue)
- Complex and unpredictable shell escaping rules
- Doesn't work consistently between TOML/file tasks

**Please migrate to using the `usage` field instead.** See the [migration guide](/tasks/task-arguments#tera-templates).
:::

<details>
<summary>Click to see deprecated Tera template syntax (not recommended)</summary>

You can define arguments using Tera template functions (deprecated):

```mise-toml
[tasks.test]
run = [
    'cargo test {{arg(name="cargo_test_args", var=true)}}',
    './scripts/test-e2e.sh {{option(name="e2e_args")}}',
]
```

Then running `mise run test foo bar` will pass `foo bar` to `cargo test`.
`mise run test --e2e-args baz` will pass `baz` to `./scripts/test-e2e.sh`.

#### Positional Arguments

These are defined in scripts with <span v-pre>`{{arg()}}`</span>. They are used for positional
arguments where the order matters.

Example:

```mise-toml
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

#### Options

These are defined in scripts with <span v-pre>`{{option()}}`</span>. They are used for named
arguments where the order doesn't matter.

Example:

```mise-toml
[tasks.test]
run = 'cargo test {{option(name="file")}}'
# execute: mise run test --file my-test-file
# runs: cargo test my-test-file
```

- `name`: The name of the argument. This is used for help/error messages.
- `var`: If `true`, multiple values can be passed.
- `default`: The default value if the option is not provided.

#### Flags

Flags are like options except they don't take values. They are defined in scripts with <span v-pre>
`{{flag()}}`</span>.

Examples:

```mise-toml
[tasks.echo]
run = 'echo {{flag(name="myflag")}}'
# execute: mise run echo --myflag
# runs: echo true
```

```mise-toml
[tasks.maybeClean]
run = '''
if [ '{{flag(name='clean')}}' = 'true' ]; then
  echo 'cleaning'
fi
'''
# execute: mise run maybeClean --clean
# runs: echo cleaning
```

- `name`: The name of the flag. This is used for help/error messages.

The value will be `true` if the flag is passed, and `false` otherwise.

</details>
