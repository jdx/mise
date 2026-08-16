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
run = "./scripts/test-e2e.sh --{{ vars.test_mode }}"
```

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
and plugin-provided directive forms. These directives populate `vars` instead of exporting the
values as environment variables when used under `[vars]`.

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
run = "./scripts/test-e2e.sh --{{ vars.test_mode }}"
```

See [Task Configuration](/tasks/task-configuration.html#task-vars) for task-local vars.
