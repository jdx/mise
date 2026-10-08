---
description: "Define short commands and task groups in the [tasks] table of mise.toml."
---

# TOML tasks

Define a task in `mise.toml` when its command fits on a line or two, or when it
mostly runs other tasks. Move longer scripts to [file tasks](/tasks/file-tasks.html)
so your editor can highlight and lint them.

## Define a task

The shortest form maps a task name to a command:

```toml [mise.toml]
[tasks]
build = "cargo build"
test = "cargo test"
lint = "cargo clippy"
```

Give a task its own table to add properties:

```mise-toml [mise.toml]
[tasks.build]
description = "Build the CLI"
run = "cargo build"
```

Each property below has one example. The
[task configuration reference](/tasks/task-configuration.html) lists them all.

### Add a task from the command line

[`mise tasks add`](/cli/tasks/add.html) writes a task to `mise.toml`:

```sh
mise tasks add pre-commit --depends "test" --depends "render" -- echo pre-commit
```

This adds:

```mise-toml [mise.toml]
[tasks.pre-commit]
depends = ["test", "render"]
run = "echo pre-commit"
```

## Common properties

### Run commands

`run` is a single command or an array of commands:

```mise-toml
[tasks.test]
run = "cargo test"
```

```mise-toml
[tasks.test]
run = [
  "cargo test",
  "./scripts/test-e2e.sh",
]
```

Array entries run in order. If one fails, the task stops and the remaining
entries do not run. Extra command-line arguments go to the last entry. An entry
can also run another task; see
[Run steps in order](/tasks/architecture.html#run-steps-in-order).

`run_windows` replaces `run` on Windows:

```mise-toml
[tasks.test]
run = "cargo test"
run_windows = "cargo test --features windows"
```

### Working directory

Tasks run from the [config root](/configuration.html#config-root) by default.
Set <span v-pre>`dir = "{{cwd}}"`</span> to run from the directory where you
called mise:

```mise-toml
[tasks.test]
run = "cargo test"
dir = "{{cwd}}"
```

`MISE_ORIGINAL_CWD` also holds that directory. See
[`dir`](/tasks/task-configuration.html#dir) for other values.

### Description and alias

```mise-toml
[tasks.build]
description = "Build the CLI"
run = "cargo build"
alias = "b"
```

`mise run b` now runs `build`. The description appears in
[`mise tasks ls`](/cli/tasks/ls.html) and in the selector that
[`mise run`](/cli/run.html) opens when you give it no task name.

Set `hide = true` on a helper task to leave it out of `mise tasks ls`. You can
still run it by name, and `mise tasks ls --hidden` lists it.

### Dependencies

Dependencies run before the task. If one fails, the task does not run:

```mise-toml
[tasks.build]
run = "cargo build"

[tasks.test]
depends = ["build"]
run = "cargo test"
```

[Dependencies and execution order](/tasks/architecture.html) compares
`depends` with `wait_for`, `depends_post`, and run steps.

### Environment variables

`env` sets variables for this task only, not for its dependencies:

```mise-toml
[tasks.test]
env = { RUST_BACKTRACE = "1" }
run = "cargo test"
```

Project-wide [environment variables](/environments/) from `[env]` reach every
task. Use [`[vars]`](/configuration/vars.html) for values a command needs that
should not be exported to its environment:

```mise-toml [mise.toml]
[env]
VERBOSE_ARGS = "--verbose"

[vars]
e2e_args = "--headless"

[tasks.test]
run = "./scripts/test-e2e.sh {{vars.e2e_args}} $VERBOSE_ARGS"
```

### Sources and outputs

With [`sources`](/tasks/task-configuration.html#sources) and
[`outputs`](/tasks/task-configuration.html#outputs), mise skips the task while
its outputs are up to date. See
[Skip tasks that are up to date](/tasks/running-tasks.html#skip-tasks-that-are-up-to-date)
for an example, and [Task caching](/tasks/caching.html) for how the checks work.
`sources` alone also tells [`mise watch`](/cli/watch.html) which
files to watch. In a run script, the
[`task_source_files()`](/templates.html#task-source-files) template function
returns the files that match `sources`.

### Confirmation

`confirm` prompts before the task's own command runs:

```toml
[tasks.release]
confirm = "Are you sure you want to cut a new release?"
description = "Cut a new release"
file = "scripts/release.sh"
```

The task's `depends` have already run when the prompt appears. To prompt before
that work, put the confirmation on those tasks or call them as
[run steps](/tasks/architecture.html#run-steps-in-order). See
[`confirm`](/tasks/task-configuration.html#confirm).

### Daemons <Badge type="warning" text="experimental" />

::: warning Experimental
Daemons require `experimental = true` under `[settings]`.
:::

Set `daemons = "postgres"` to start a
[project daemon](/daemons.html#tasks-that-require-daemons) and wait until it is
ready before the task runs. The daemon keeps running afterward. See
[`daemons`](/tasks/task-configuration.html#daemons) for the accepted values.

## Specifying a shell or an interpreter {#shell-shebang}

On Unix, inline commands run with the default
[`sh -o errexit -c`](/configuration/settings.html#unix_default_inline_shell_args),
so the script stops at the first failing command. Add `set +e` to keep going:

```mise-toml
[tasks.cleanup]
run = '''
set +e
cd /nonexistent
echo "This does not fail the task"
'''
```

The default on Windows is
[`cmd /c`](/configuration/settings.html#windows_default_inline_shell_args).
Set `shell` to run a task's commands with another shell:

```mise-toml
[tasks.lint]
shell = "bash -c"
run = "cargo clippy"
```

Or start the script with a shebang:

```mise-toml
[tasks.lint]
run = '''
#!/usr/bin/env bash
cargo clippy
'''
```

A custom `shell` (even `sh -c` or `bash -c`), a shebang script, and the Windows
default `cmd /c` do not stop at the first failure. Add `set -e` (or
`set -euo pipefail`) to the script yourself:

```mise-toml
[tasks.lint]
run = '''
#!/usr/bin/env bash
set -euo pipefail
cargo clippy
'''
```

mise runs a shebang task as a script file. Extra arguments that a
[usage spec](/tasks/task-arguments.html) does not define reach the script as
ordinary arguments, such as `$1` and `$@` in Bash:

```mise-toml
[tasks.greet]
run = '''
#!/usr/bin/env bash
echo "hello $1"
'''
```

```sh
mise run greet world
# hello world
```

Without a shebang, mise appends the arguments to the last command instead, so
`$1` is empty.

### Other languages

A shebang or `shell` runs the task with any interpreter, such as Python,
Node.js, or Ruby:

::: code-group

```mise-toml [python]
[tools]
python = "latest"

[tasks.python_task]
run = '''
#!/usr/bin/env python
for i in range(10):
    print(i)
'''
```

```mise-toml [python + uv]
[tools]
uv = "latest"

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
node = "24"

[tasks.node_task]
shell = "node -e"
run = [
  "console.log('First line')",
  "console.log('Second line')",
]
```

```mise-toml [bun]
[tools]
bun = "latest"

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
deno = "latest"

[tasks.deno_task]
run = '''
#!/usr/bin/env -S deno run
console.log(`Hello from Deno ${Deno.version.deno}`)
'''
```

```mise-toml [ruby]
[tools]
ruby = "latest"

[tasks.ruby_task]
run = '''
#!/usr/bin/env ruby
puts 'Hello, ruby!'
'''
```

:::

::: details What is a shebang, and what does `-S` do?

A shebang is the character sequence `#!` at the start of a script that names
the program that interprets it. `#!/usr/bin/env python` runs the script with
the `python` found on `PATH`. On Linux and macOS the system runs the shebang
line, so [`env`](https://manpages.ubuntu.com/manpages/jammy/man1/env.1.html)
finds `python` on `PATH`. Windows has no shebangs: mise reads the line, drops a
leading `/usr/bin/env` or `/usr/bin/env -S`, and starts the named interpreter
itself.

`-S` splits the rest of the line into separate arguments, so you can pass flags
to the interpreter. For example, `#!/usr/bin/env -S python -u` runs Python with
unbuffered output.

:::

### Simple commands run without a shell

On Unix, when a task uses the default shell, mise can start a simple inline
command such as `node build.js` directly, without `sh`. A command that uses
shell syntax (quoting, expansion, operators, or builtins), or that runs with
`ENV` or `BASH_ENV` set, still goes through the shell, as do sandboxed tasks and
tasks with `cache.audit = true`. A task `shell`, `mise run --shell`, or the
`unix_default_inline_shell_args` setting always uses the shell, even when it
names the default, so set one if a wrapper named `sh` on `PATH` must run.

## Using a file or remote script {#using-a-file-or-remote-script}

`file` runs a script instead of an inline command:

```toml
[tasks.release]
description = "Cut a new release"
file = "scripts/release.sh"
```

### Remote tasks

`file` can also fetch the script from a URL. mise downloads it and runs it, so
use only sources you trust.

#### HTTP

```toml
[tasks.build]
file = "https://example.com/build.sh"
```

#### Git

::: code-group

```toml [ssh]
[tasks.build]
file = "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0"
```

```toml [https]
[tasks.build]
file = "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0"
```

:::

The URL has the form `git::<protocol>://<repository>.git//<path>?ref=<ref>`,
where `ref` is an optional branch, tag, or commit. See
[Git URL syntax](/tasks/task-discovery.html#git-url-syntax) for every field.

#### Cache

mise caches each remote task file under `MISE_CACHE_DIR` and does not download
it again until you run `mise cache clear`. To fetch fresh copies, pass
`mise run --no-cache` or set
[`task.remote_no_cache`](/configuration/settings.html#task.remote_no_cache)
(`MISE_TASK_REMOTE_NO_CACHE=1`).

## Arguments {#arguments}

Define arguments with a `usage` spec. mise parses them and passes each value to
the command as a `usage_*` environment variable:

```mise-toml
[tasks.test]
usage = '''
arg "<file>" help="Test file to run" default="all"
flag "--format <format>" help="Output format" default="text"
'''
run = 'echo "Testing ${usage_file?} with format ${usage_format?}"'
```

See [Task arguments](/tasks/task-arguments.html) for the spec syntax. Without a
spec, extra arguments are [forwarded](/tasks/running-tasks.html#pass-arguments)
to the command. Tasks that still use `arg()`, `option()`, or `flag()` in `run`
should follow the [migration guide](/tasks/task-arguments.html#tera-templates).
