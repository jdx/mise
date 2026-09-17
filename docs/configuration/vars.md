---
description: "Define reusable configuration variables and reference them in Tera templates."
---

# Variables

Define shared configuration values in `[vars]` and reference them with
<span v-pre>`{{ vars.NAME }}`</span> in a [Tera template](/templates). Use vars for values
that mise needs to render configuration; use [`[env]`](/environments/) for values that
commands need as environment variables. mise does not export vars to child processes.

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

## When vars are resolved

mise resolves top-level `[vars]` entries while loading configuration, before applying any
task-local vars. Each entry can reference vars resolved before it. The resulting value is stored
as a string; referencing that value later does not evaluate its template again.

```mise-toml
[vars]
mode = "headless"
args = "--mode={{ vars.mode }}"
```

Here, `args` resolves to `--mode=headless`. A later override of `mode` changes references to
`vars.mode`, but does not recalculate `args`. To change `args`, override `args` itself or move
the expression into the task that needs it.

## Configuration hierarchy

Vars follow mise's [configuration hierarchy](/configuration.html#configuration-hierarchy). Define
shared values globally, then override them in project, local, or environment-specific config files.

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

A task-local override changes direct references to that var in the task's templated fields,
including fields inherited from a task template. It does not recalculate top-level vars that
used the original value:

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

`args` was resolved before the task-local override. The reference to `vars.mode` in `run`
uses the task's value, `headed`.

To make the argument use the task's mode, build it in `run`:

```mise-toml
[tasks.test]
vars = { mode = "headed" }
run = "echo --mode={{ vars.mode }}"
```

This prints `--mode=headed`. For several tasks that share the same command, put `run` in a
[task template](/tasks/templates.html#parameterizing-a-template-with-vars) and let each task
supply its vars.

### Missing vars and defaults

A top-level var cannot reference a value that exists only in a task's `vars`. Without a
fallback, that reference fails when mise loads the configuration.

The Tera `default` filter provides a fallback at the point where the expression is rendered:

```mise-toml
[vars]
args = "--mode={{ vars.mode | default(value='headless') }}"

[tasks.test]
vars = { mode = "headed" }
run = "echo {{ vars.args }} / {{ vars.mode }}"
```

This also prints `--mode=headless / headed`. The filter supplies `headless` while loading
`[vars]`; it does not defer evaluation until the task supplies `mode`. Use a fallback when a
missing value is expected, and put expressions that depend on task-local values in the task
or its template.

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
