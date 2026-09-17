---
description: "Define reusable configuration variables and reference them in Tera templates."
---

# Variables

`[vars]` defines values that can be reused in mise configuration templates. Vars are similar to
environment variables, but mise does not export them to child processes. Reference a var in a
Tera template with <span v-pre>`{{ vars.NAME }}`</span>.

```mise-toml
[vars]
node_version = "24"
test_mode = "headless"

[tools]
node = "{{ vars.node_version }}"

[tasks.test]
run = "echo {{ vars.test_mode | quote }}"
```

Run `mise run test` to print `headless`. `test_mode` is available through the
`vars` template map, but it is not exported as `$test_mode`. The `quote` filter in
this example targets POSIX shells; see [template quoting](/templates.html#string-manipulation).

Vars are available to Tera-rendered configuration such as tool versions and options, task
definitions, hooks, task includes, watch configuration, and dotfile templates. See
[Templates](/templates) for the complete template syntax and context.

## Value directives

Vars support the same value-producing directives as [`[env]`](/environments/), including defaults,
required values, redaction, files, sources, and [secrets](/environments/secrets/).

```mise-toml
[vars]
test_mode = { default = "headless" }
api_token = { required = "Set api_token in mise.local.toml" }
secret_arg = { value = "--token=abc123", redact = true }
_.file = ".env"
```

The `default` form uses a process environment variable with the same name when it is set and
non-empty; values from `[env]` are not used for this lookup. A `required` var must be supplied by the
process environment or a later config file. Values marked `redact = true` are hidden from task
output.

See the [`env._` directive reference](/environments/#env-directives) for the available file, source,
and plugin-provided directive forms. When used under `[vars]`, these directives populate `vars`
instead of exporting the values as environment variables.

## When vars are resolved

Vars are resolved once, while mise loads the config. Each entry is rendered against the vars
resolved before it, and its result is a plain string from then on:

```mise-toml
[vars]
flavor = "vanilla"
label = "{{ vars.flavor }} scoop"
```

`label` is the string `vanilla scoop`. mise never renders a var's value a second time, so
nothing that happens afterwards changes it. Resolving vars once is what lets a single value
serve `[tools]`, `[env]`, hooks, and tasks alike.

A var is therefore a value, not a macro. To share a command fragment that each task fills in
differently, write it in a [task template](/tasks/templates) instead of in `[vars]`.

## Configuration hierarchy

Vars follow mise's [configuration hierarchy](/configuration.html#configuration-hierarchy). They can
be defined in the global config and overridden by project or environment-specific config files.

For example, a default can be defined globally:

```mise-toml [~/.config/mise/config.toml]
[vars]
test_mode = "headless"
```

Then overridden for a project:

```mise-toml [mise.local.toml]
[vars]
test_mode = "headed"
```

## Task-local vars

TOML tasks can define their own vars. Task-local values override config vars while that task is
rendered, but do not change the vars available elsewhere in the configuration.

```mise-toml
[vars]
test_mode = "headless"

[tasks.test]
vars = { test_mode = "headed" }
run = "echo {{ vars.test_mode | quote }}"
```

Here, `mise run test` prints `headed`; other tasks still see `headless` unless
they define their own override. See [Task Configuration](/tasks/task-configuration.html#task-vars)
for task-local vars.

### What a task-local var can change

A task-local var applies to the <span v-pre>`{{ vars.* }}`</span> references written in that
task's own fields. It cannot reach back into a config var that was already resolved:

```mise-toml
[vars]
args = "--mode={{ vars.mode | default(value='fast') }}"

[tasks.test]
vars = { mode = "slow" }
run = "echo {{ vars.args }} / {{ vars.mode }}"
```

`mise run test` prints `--mode=fast / slow`. `args` was rendered while the config loaded, when
`mode` had no value yet, so `default` applied and `args` was fixed at `--mode=fast`. The
<span v-pre>`{{ vars.mode }}`</span> written in `run` is rendered when the task runs, and does
see `slow`.

Without the `default` filter, mise reports the missing var as an error while loading the
config. Adding `default` is what turns that error into a value chosen earlier than intended,
so reach for it only where a var genuinely may be absent.

When several tasks share a command that each one parameterizes, put the command in a
[task template](/tasks/templates) and let each task pass its own vars. A template's fields are
merged into the task before the task renders, so they see the task's vars:

```mise-toml
[task_templates.e2e]
run = "./scripts/test-e2e.sh --mode={{ vars.mode | default(value='headless') }}"

[tasks.test]
extends = "e2e"
vars = { mode = "headed" }
```
