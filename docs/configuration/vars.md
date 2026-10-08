---
description: "Define reusable values in mise.toml vars and reference them from Tera templates without exporting them to commands."
socialDescription: "Define reusable values in mise.toml vars and use them in templates without exporting them."
---

# Config variables

Define shared values in `[vars]` and reference them with
<span v-pre>`{{ vars.NAME }}`</span> in a [Tera template](/templates.html). Use
vars for values that mise needs to render config; use
[`[env]`](/environments/) for values that commands need as environment
variables. mise does not export vars to child processes.

```mise-toml
[vars]
node_version = "24"
test_mode = "headless"

[tools]
node = "{{ vars.node_version }}"

[tasks.test]
run = "echo {{ vars.test_mode | quote }}"
```

`mise run test` prints `headless`. The task reads `test_mode` through the `vars`
template map; there is no `$test_mode` environment variable. The `quote` filter
quotes for POSIX shells; see [template quoting](/templates.html#string-manipulation).

Vars are available wherever mise renders templates in config, such as tool
versions and options, task definitions, [hooks](/hooks.html),
[`watch_files`](/hooks.html#watch-files-hook),
[task includes](/tasks/task-discovery.html), and
[dotfile templates](/dotfiles.html).

## Configuration hierarchy

Vars follow the [config file hierarchy](/configuration.html#configuration-hierarchy):
a higher-precedence file overrides a var of the same name. Define a default in
global config:

```mise-toml [~/.config/mise/config.toml]
[vars]
test_mode = "headless"
```

Then override it for your own checkout:

```mise-toml [mise.local.toml]
[vars]
test_mode = "headed"
```

## Value directives

Vars accept the same value directives as [`[env]`](/environments/), including
defaults, required values, redaction, files, sources, and
[secrets](/environments/secrets/):

```mise-toml
[vars]
test_mode = { default = "headless" }
api_token = { required = "Set api_token in mise.local.toml" }
_.file = { path = ".env.secret", redact = true }
```

- `default` uses a process environment variable with the same name when it is
  set and not empty, and the given value otherwise. Values from `[env]` are not
  used for this lookup.
- `required` fails when no value is supplied by the process environment or a
  higher-precedence config file, such as `mise.local.toml`. The string is shown
  as help in the error.
- `redact = true` hides the value in task output; here it applies to every
  value loaded from `.env.secret`.

See the [`env._` directive reference](/environments/#env-directives) for the
file, source, and plugin directive forms. Under `[vars]`, these directives fill
`vars` instead of exporting environment variables.

## Ask once and remember {#prompt}

`prompt` makes a var a per-machine answer. Pair it with a `default` to offer a
suggestion, or with `required` when there is none:

```mise-toml
[vars.git_name]
default = "Ada Lovelace"
prompt = "Git author name"

[vars.git_email]
required = "Run mise vars prompt to set git_email"
prompt = "Git email"
```

Run `mise vars prompt` to be asked for each of these vars that has no answer
yet; name vars to ask only for those (`mise vars prompt git_name`). Enter accepts
the default. mise saves every answer, including an accepted default, in
`$MISE_STATE_DIR/vars.toml`, so it is never asked again, never lands in your
config or dotfiles history, and is available to every command that reads vars from then on.

```sh
mise vars prompt          # ask for every unanswered prompt var
mise vars prompt git_name=Ada  # set an answer without asking
mise vars ls              # show the saved answers
mise vars unset git_name  # forget one, to be asked again
```

`mise bootstrap --prompt-vars` does the same inside a bootstrap run, which is how
a fresh machine answers them with `mise bootstrap --adopt owner/dotfiles`: the
config that declares the vars only exists once the checkout has been fetched. A
`--dry-run` still saves the answers it asks for.

Only `mise vars prompt` and `--prompt-vars` ever ask. Everything else, including
shell activation, tasks, and runs without a terminal, uses the saved answer and
otherwise falls back to `default`, or fails as any other `required` var does.

A var resolves in this order, highest first: the process environment, a value
in a higher-precedence config file such as `mise.local.toml`, the saved answer,
the `default`. Answers are keyed by var name alone, so projects that use the same
name share one answer on a machine. `prompt` works only in `[vars]`, not `[env]`.

## Task-local vars

TOML tasks can define their own vars. A task-local value overrides a config var
while that task is rendered, without changing the value anywhere else:

```mise-toml
[vars]
test_mode = "headless"

[tasks.test]
vars = { test_mode = "headed" }
run = "echo {{ vars.test_mode | quote }}"
```

`mise run test` prints `headed`; other tasks still see `headless` unless they
define their own value. See [task `vars`](/tasks/task-configuration.html#task-vars).

## When vars are resolved

mise resolves top-level `[vars]` entries while it loads config, before any
task-local vars apply. Each entry can reference the entries resolved before it.
The result is stored as a string, so later references do not evaluate the
template again:

```mise-toml
[vars]
mode = "headless"
args = "--mode={{ vars.mode }}"
```

`args` resolves to `--mode=headless`. If a higher-precedence file such as
`mise.local.toml` sets `mode = "headed"`, <span v-pre>`{{ vars.mode }}`</span>
renders `headed`, but `args` stays `--mode=headless`. To change `args`, override
`args` itself or build the string in the task that uses it.

### What a task-local var can change

A task-local override changes direct references to that var in the task's
templated fields, including fields inherited from a task template. It does not
recalculate top-level vars that used the original value:

```mise-toml
[vars]
mode = "headless"
args = "--mode={{ vars.mode }}"

[tasks.test]
vars = { mode = "headed" }
run = "echo {{ vars.args }} / {{ vars.mode }}"
```

`mise run test` prints:

```text
--mode=headless / headed
```

To make the argument follow the task's mode, build it in `run`:

```mise-toml
[tasks.test]
vars = { mode = "headed" }
run = "echo --mode={{ vars.mode }}"
```

This prints `--mode=headed`. When several tasks share the same command, put
`run` in a [task template](/tasks/templates.html#parameterizing-a-template-with-vars)
and let each task supply its vars.

### Missing vars and defaults

A top-level var cannot reference a value that exists only in a task's `vars`.
Without a fallback, the reference fails when mise loads the config. The Tera
`default` filter supplies a fallback where the expression is rendered:

```mise-toml
[vars]
args = "--mode={{ vars.mode | default(value='headless') }}"

[tasks.test]
vars = { mode = "headed" }
run = "echo {{ vars.args }} / {{ vars.mode }}"
```

This also prints `--mode=headless / headed`: the filter supplies `headless`
while `[vars]` loads, and does not wait for the task to supply `mode`. Put
expressions that depend on task-local values in the task or its template.
