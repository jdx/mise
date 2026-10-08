---
description: "Define task arguments and flags with a usage spec to get parsing, validation, --help, and completions."
socialDescription: "Define task arguments and flags with a usage spec for validation, --help, and completions."
---

# Task arguments

Define arguments when a task needs named inputs, validation, `--help` output, or
shell completions. mise parses the command line against the task's
[usage](https://usage.jdx.dev) spec and passes each value to the script as a
`usage_*` environment variable. Without a spec, mise
[forwards extra arguments](/tasks/running-tasks.html#pass-arguments) to the
command unchanged.

## Define arguments {#usage-field}

Write the spec in a TOML task's `usage` field or in `#USAGE` comments in a
[file task](/tasks/file-tasks.html). Both forms define the same spec:

::: code-group

```mise-toml [mise.toml]
[tasks.deploy]
description = "Deploy application"
usage = '''
arg "<environment>" help="Target environment" {
  choices "dev" "staging" "prod"
}
flag "-v --verbose" help="Enable verbose output"
flag "--region <region>" help="AWS region" default="us-east-1" env="AWS_REGION"
'''
run = '''
#!/usr/bin/env bash
if [ "${usage_verbose:-false}" = "true" ]; then
  echo "Verbose mode enabled"
fi
echo "Deploying to ${usage_environment?} in ${usage_region?}"
'''
```

```bash [mise-tasks/deploy]
#!/usr/bin/env bash
#MISE description="Deploy application"
#USAGE arg "<environment>" help="Target environment" {
#USAGE   choices "dev" "staging" "prod"
#USAGE }
#USAGE flag "-v --verbose" help="Enable verbose output"
#USAGE flag "--region <region>" help="AWS region" default="us-east-1" env="AWS_REGION"

if [ "${usage_verbose:-false}" = "true" ]; then
  echo "Verbose mode enabled"
fi
echo "Deploying to ${usage_environment?} in ${usage_region?}"
```

:::

Each value reaches the script as a `usage_*` variable:

```sh
mise run deploy staging --verbose --region us-west-2
```

```text
Verbose mode enabled
Deploying to staging in us-west-2
```

`mise run deploy --help` prints help generated from the spec:

```text
Deploy application

Usage: deploy [-v --verbose] [--region <region>] <environment>

Arguments:
  <environment>  Target environment
                 [possible values: dev, staging, prod]

Flags:
  -v, --verbose          Enable verbose output
      --region <region>  AWS region
                         [env: AWS_REGION]
                         (default: us-east-1)
  -h, --help             Print help
```

A missing required argument or a value outside the choices fails before the
script starts:

```sh
mise run deploy qa
```

```text
mise ERROR failed to validate task deploy
mise ERROR Invalid choice for arg environment: qa, expected one of dev, staging, prod
```

With mise's [shell completions](/shell-setup.html) enabled,
`mise run deploy <Tab>` offers the choices. The separate `usage` CLI is not
needed. [`mise generate task-docs`](/cli/generate/task-docs.html) renders
Markdown documentation from the same spec.

### Comment syntax in file tasks {#file-task-headers}

In a file task, `#USAGE` lines hold the usage spec and `#MISE` lines hold task
properties as TOML. The comment marker can be `#`, `//`, or `::`, and
whitespace may follow it. The keyword can also be bracketed: `#USAGE`,
`# USAGE`, `// USAGE`, and `# [USAGE]` all work, so formatters that add a space
do not break them. [File tasks](/tasks/file-tasks.html#configure-with-mise-comments)
describes `#MISE` lines.

## Spec quick reference

| Spec                                               | Meaning                                                                                   |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `arg "<file>"`                                     | Required positional argument                                                              |
| `arg "[file]"`                                     | Optional positional argument                                                              |
| `default="config.toml"`                            | Value used when the argument or flag is not given                                         |
| `arg "[files]" var=#true`                          | Zero or more values; `arg "<files>" var=#true` needs at least one                         |
| `var_min=2`, `var_max=5`                           | Limits on the number of values for a variadic argument                                    |
| `arg "<level>" { choices "debug" "info" "warn" }`  | Accept only the listed values                                                             |
| `env="API_TOKEN"`                                  | Take the value from an environment variable; see [backing](#environment-variable-backing) |
| `flag "-f --force"`                                | Boolean flag; also `flag "-f"` or `flag "--force"` alone                                  |
| `flag "-o --output <file>"`                        | Flag that takes a value                                                                   |
| `required=#true`                                   | Fail unless the flag is passed                                                            |
| `flag "-v --verbose" count=#true`                  | Repeatable flag whose value is the count, such as `3` for `-vvv`                          |
| `flag "--color" negate="--no-color" default=#true` | Boolean flag that `--no-color` turns off                                                  |
| `help="…"`                                         | One-line help                                                                             |
| `long_help="…"`                                    | Longer help that replaces `help` in `--help` output                                       |
| `hide=#true`                                       | Hide the argument or flag from help                                                       |
| `double_dash="required"`                           | Accept the argument's values only after `--`                                              |
| `arg "<file>"`, `arg "<dir>"`                      | An argument named `file` or `path` completes file names; `dir` completes directories      |
| `complete "user" run="mycli users"`                | Complete the `user` argument or flag from a command's output, one value per line          |

Properties can also go in a block, one per line, as `choices` and `long_help`
do in the next example. See the [usage spec](https://usage.jdx.dev/spec/) for
every option.

### Long help

```mise-toml
[tasks.complex]
usage = '''
arg "<input>" {
  help "Input file to process"
  long_help """
  The input file should be in JSON or YAML format.

  Supported schemas:
  - schema-v1: Legacy format
  - schema-v2: Current format (recommended)
  - schema-v3: Experimental format

  Example:
    mise run complex data.json
  """
}
flag "--format <fmt>" {
  help "Output format"
  long_help """
  Supported output formats:
  - json: JSON output (default)
  - yaml: YAML output
  - toml: TOML output
  """
  choices "json" "yaml" "toml"
  default "json"
}
'''
run = 'process-data "${usage_input?}" --format "${usage_format?}"'
```

### Double dash

`double_dash` controls how an argument relates to `--`:

```kdl
// Values only after --: mycli -- file.txt
arg "<file>" double_dash="required"

// Both work: mycli file.txt or mycli -- file.txt
arg "<file>" double_dash="optional"

// After the first value, the rest are treated as if -- was used
arg "<files>" double_dash="automatic"

// Keep -- as a value in a variadic argument
arg "<args>..." double_dash="preserve"
```

### Completions with descriptions

With `descriptions=#true`, each line of the command's output is split at the
first `:` into a value and its description:

```kdl
arg "<plugin>"
complete "plugin" run="mycli plugins list" descriptions=#true
```

```text
nodejs:JavaScript runtime
python:Python language
ruby:Ruby language
```

## Read argument values

### Environment variables

Each argument and flag becomes a `usage_<name>` variable, with `-` in the name
replaced by `_`: `--dry-run` becomes `usage_dry_run`, and `<api-key>` becomes
`usage_api_key`.

- A boolean flag is `true` when passed. Without a `default`, it is unset when
  not passed.
- A count flag holds the number of times it was passed, and is unset when not
  passed.
- An optional argument without a `default` is unset when not given.
- A variadic argument arrives as one shell-quoted string, such as `a 'b c'`.

To use a variadic value as a Bash array, `eval` it, then quote each element:

```mise-toml
[tasks.process]
usage = 'arg "<files>" var=#true'
run = '''
#!/usr/bin/env bash
eval "files=($usage_files)"
for f in "${files[@]}"; do
  echo "Processing: $f"
done
'''
```

### The `usage` map in templates

Run scripts can also read values through the `usage` map in
[Tera templates](/templates.html). It uses the same snake_case keys, so
`--dry-run` is <span v-pre>`{{ usage.dry_run }}`</span>. Variadic arguments and
flags are arrays that work with Tera's `for` loops and filters such as
`length`:

```mise-toml [mise.toml]
[tasks.deploy]
description = "Deploy application"
usage = '''
arg "<environment>" help="Target environment"
flag "-v --verbose" help="Enable verbose output"
flag "--region <region>" help="AWS region" default="us-east-1"
'''
run = '''
echo "Deploying to {{ usage.environment }} in {{ usage.region }}"
{% if usage.verbose %}
  echo "Verbose mode enabled"
{% endif %}
'''
```

The `usage` map is separate from the deprecated `arg()`, `option()`, and
`flag()` functions described in [the migration guide](#tera-templates). Do not
mix the two in one task.

<span v-pre>`{{ usage.* }}`</span> also works in `depends`, `depends_post`, and
`wait_for`, to pass the task's arguments on to its dependencies. See
[Passing parent task arguments to dependencies](/tasks/task-configuration.html#passing-parent-task-arguments-to-dependencies).

### Inherited `usage_*` variables

mise clears `usage_*` variables inherited from the calling environment, even for
tasks without a usage spec. Tasks with [`raw_args = true`](/tasks/task-configuration.html#raw-args)
keep them. To pass a value in deliberately, use a separately named variable,
for example through `env=`:

```mise-toml [mise.toml]
[tasks.deploy]
usage = 'arg "[environment]" env="DEPLOY_ENV"'
run = 'echo "Deploying to ${usage_environment:-default}"'
```

```sh
DEPLOY_ENV=staging mise run deploy
# Deploying to staging
```

## Bash variable expansion {#bash-variable-expansion}

Parameter expansion lets [shellcheck](https://www.shellcheck.net/) see that a
`usage_*` variable may be unset, and supplies defaults for flags:

| Syntax            | Behavior                              | Use case                                                    | Example                       |
| ----------------- | ------------------------------------- | ----------------------------------------------------------- | ----------------------------- |
| `${var?}`         | Error if unset                        | Required arguments, and arguments or flags with a `default` | `${usage_profile?}`           |
| `${var:?}`        | Error if unset or empty               | Values that must not be empty                               | `${usage_target:?}`           |
| `${var:-default}` | Use default if unset or empty         | Boolean flags and optional arguments without a `default`    | `${usage_clean:-false}`       |
| `${var:=default}` | Set and use default if unset or empty | A default you want to reuse later in the script             | `${usage_dir:=.}`             |
| `${var:+value}`   | Use value if set and non-empty        | Optional string values                                      | `${usage_output:+has-output}` |

Follow three rules:

- Use `${usage_x?}` for an argument that is required or has a `default`; mise
  has already checked it.
- Use `${usage_x:-false}` for a boolean flag without a `default`, and
  `${usage_x:-}` for an optional argument without one.
- Compare booleans with `= "true"`. `${usage_x:+…}` also expands for the string
  `false`, so it does not test whether a flag is on.

```bash
#!/usr/bin/env bash
# --profile has default="dev"; --clean has no default
cargo build --profile "${usage_profile?}"
if [ "${usage_clean:-false}" = "true" ]; then
  cargo clean
fi
```

## Environment variable backing {#environment-variable-backing}

An argument or flag with `env="NAME"` takes its value from that environment
variable when it is not on the command line. The precedence is the command
line, then the environment variable, then the `default`:

```mise-toml [mise.toml]
[tasks.deploy]
usage = '''
arg "[environment]" env="DEPLOY_ENV" default="development"
flag "-p --profile <profile>" env="BUILD_PROFILE" default="dev"
flag "-v --verbose" env="VERBOSE"
'''
run = 'echo "env=${usage_environment?} profile=${usage_profile?} verbose=${usage_verbose:-false}"'
```

```sh
mise run deploy
# env=development profile=dev verbose=false

DEPLOY_ENV=staging BUILD_PROFILE=release VERBOSE=true mise run deploy
# env=staging profile=release verbose=true

DEPLOY_ENV=staging mise run deploy production
# env=production profile=dev verbose=false
```

A boolean flag counts as passed when its variable is `true` or `1`. The
variable also satisfies a required argument:

```mise-toml [mise.toml]
[tasks.publish]
usage = 'arg "<api-key>" env="API_KEY" help="API key for publishing"'
run = 'publish --api-key "${usage_api_key?}"'
```

`mise run publish` fails with `Missing required arg: <api-key>` unless `API_KEY`
is set or the key is passed on the command line. `mise run publish --help`
shows `[env: API_KEY]` next to the argument. File tasks use the same syntax in
`#USAGE` lines.

## Share flags between tasks {#shared-flags}

Define shared flags once in a `.usage.kdl` file to keep their names, help text,
and validation consistent across tasks. An `include` loads the file, and `use`
adds a named _flagset_ to the task's arguments.

For example, save this flagset in your project root:

```kdl [shared.usage.kdl]
flagset "common" {
  flag "--env <env>" help="Target environment" {
    arg "<env>" {
      choices "dev" "staging" "prod"
    }
  }
  flag "--dry-run" help="Print what would happen"
}
```

### Include shared flags in a task

In a file task, include the file from a `#USAGE` comment. Use
`$MISE_CONFIG_ROOT` to locate it relative to the task's config root:

```bash [mise-tasks/deploy]
#!/usr/bin/env bash
#USAGE include file="$MISE_CONFIG_ROOT/shared.usage.kdl"
#USAGE use "common"
#USAGE flag "--replicas <n>" help="How many to run"
echo "env=${usage_env?} replicas=${usage_replicas?}"
```

For a TOML task, build the include path with the
<span v-pre>`{{ config_root }}`</span> template variable instead:

```mise-toml [mise.toml]
[tasks.deploy]
usage = """
include file="{{ config_root }}/shared.usage.kdl"
use "common"
flag "--replicas <n>" help="How many to run"
"""
run = 'echo "env=${usage_env?} replicas=${usage_replicas?}"'
```

Use either definition. Both accept `--env`, `--dry-run`, and `--replicas`, and
reject values outside the choices for `--env`:

```sh
mise run deploy --env staging --replicas 3
mise run deploy --help
```

Shared flags appear in `--help` where the `use` node is written. Including a
flag defines its interface; the task's script must implement its behavior.
These examples only print the selected environment and replica count.

### Include paths in file tasks

Relative paths resolve from the directory that contains the task file, whatever
directory you run mise from. For `mise-tasks/deploy`, this includes
`shared.usage.kdl` from the project root:

```bash
#USAGE include file="../shared.usage.kdl"
```

Include paths also expand `$NAME` and `${NAME}` references to environment
variables, with `$$` for a literal dollar sign. When mise parses a file task's
usage spec, these variables are available:

- Variables inherited when mise starts.
- `MISE_CONFIG_ROOT` and `MISE_PROJECT_ROOT`, when the corresponding roots are
  available.
- `MISE_TASK_DIR` and `MISE_TASK_FILE`, for the task's directory and file path.

These paths resolve the same way for execution, help, task listing, and
validation. File-task `#USAGE` comments are not rendered as Tera templates, so
use `$MISE_CONFIG_ROOT` rather than <span v-pre>`{{ config_root }}`</span>.

Task and project `env` directives are applied after mise parses the usage spec,
so they cannot supply variables for include paths. If an include references an
undefined variable, mise reports an invalid usage spec. The task still loads,
but without its argument parsing, help, and validation.

To share tools, environment variables, or dependencies between tasks in the
same project, use [task templates](/tasks/templates.html).

## Mount a spec from another CLI

A file task that wraps another CLI can mount the usage spec that CLI generates:

```bash [mise-tasks/run-release]
#!/usr/bin/env bash
#USAGE mount "mise run run-release -- --usage-spec"

exec ./target/release/mycli "$@"
```

The mount command runs when shell completion asks for the task's spec, so it
must work outside the task's own process. Calling the task itself, as shown,
lets mise apply the task's configuration before it forwards `--usage-spec`.

## Migrate from Tera argument functions <Badge type="danger" text="deprecated" /> {#tera-templates}

`arg()`, `option()`, and `flag()` in run scripts are deprecated, and mise warns
when a task uses them. They will be removed in mise 2027.5.0. They render as
empty strings while mise collects the spec, and their quoting differs by shell.
Rewrite them as a `usage` spec, as in the examples below.

To stop mise from reading these functions now, set
[`task.disable_spec_from_run_scripts`](/configuration/settings.html#task.disable_spec_from_run_scripts)
(`MISE_TASK_DISABLE_SPEC_FROM_RUN_SCRIPTS=1`). mise then builds the spec only
from the `usage` field:

```toml
# ~/.config/mise/config.toml
[settings]
task.disable_spec_from_run_scripts = true
```

### Example 1: Simple arguments

::: code-group

```mise-toml [Usage]
[tasks.test]
usage = 'arg "<file>" help="Test file" default="all"'
run = 'cargo test "${usage_file?}"'
```

```mise-toml [Deprecated]
[tasks.test]
run = '''
cargo test {{arg(
  name="file",
  default="all",
  help="Test file"
)}}
'''
```

:::

### Example 2: Multiple arguments with flags

::: code-group

```mise-toml [Usage]
[tasks.build]
usage = '''
arg "<profile>" default="dev"
flag "-v --verbose"
'''
run = '''
#!/usr/bin/env bash
args=()
if [ "${usage_verbose:-false}" = "true" ]; then
  args+=(--verbose)
fi
cargo build --profile "${usage_profile?}"
./package.sh "${args[@]}"
'''
```

```mise-toml [Deprecated]
[tasks.build]
run = [
  'cargo build --profile {{arg(name="profile", default="dev")}}',
  './package.sh {{flag(name="verbose")}}'
]
```

:::

### Example 3: Options with choices

::: code-group

```mise-toml [Usage]
[tasks.deploy]
usage = '''
flag "--env <env>" required=#true {
  choices "dev" "prod"
}
flag "--force"
'''
run = '''
#!/usr/bin/env bash
args=(--env "${usage_env?}")
if [ "${usage_force:-false}" = "true" ]; then
  args+=(--force)
fi
deploy "${args[@]}"
'''
```

```mise-toml [Deprecated]
[tasks.deploy]
run = '''
deploy {{option(
  name="env",
  choices=["dev", "prod"]
)}} {{flag(name="force")}}
'''
```

:::

### Example 4: Variadic arguments

::: code-group

```mise-toml [Usage]
[tasks.lint]
usage = 'arg "<files>" var=#true'
run = '''
#!/usr/bin/env bash
eval "files=(${usage_files?})"
eslint "${files[@]}"
'''
```

```mise-toml [Deprecated]
[tasks.lint]
run = 'eslint {{arg(name="files", var=true)}}'
```

:::
