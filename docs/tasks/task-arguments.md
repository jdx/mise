# Task Arguments

Task arguments allow you to pass parameters to tasks, making them more flexible and reusable. There are three ways to define task arguments in mise, but only two are recommended for current use.

## Recommended Methods

### 1. Usage Field (Preferred) {#usage-field}

The **usage field** is the recommended approach for defining task arguments. It provides a clean, declarative syntax that works with both TOML tasks and file tasks.

#### Quick Example

```toml [mise.toml]
[tasks.deploy]
description = "Deploy application"
usage = '''
arg "<environment>" help="Target environment" choices=["dev", "staging", "prod"]
flag "-v --verbose" help="Enable verbose output"
option "--region <region>" help="AWS region" default="us-east-1" env="AWS_REGION"
'''
run = '''
echo "Deploying to $usage_environment in $usage_region"
[[ "$usage_verbose" == "true" ]] && set -x
./deploy.sh "$usage_environment" "$usage_region"
'''
```

Arguments defined in the usage field are automatically available as environment variables prefixed with `usage_`:

```bash
# Execute with arguments
$ mise run deploy staging --verbose --region us-west-2

# Inside the task, these are available as:
# $usage_environment = "staging"
# $usage_verbose = "true"
# $usage_region = "us-west-2"
```

**Help output example:**

```bash
$ mise run deploy --help
Deploy application

Usage: deploy <environment> [OPTIONS]

Arguments:
  <environment>  Target environment [possible values: dev, staging, prod]

Options:
  -v, --verbose          Enable verbose output
      --region <region>  AWS region [env: AWS_REGION] [default: us-east-1]
  -h, --help            Print help
```

## Complete Usage Specification Reference

### Positional Arguments (`arg`)

Positional arguments are defined with `arg` and must be provided in order.

#### Basic Syntax

```
arg "<name>" help="Description"               # Required positional arg
arg "[name]" help="Description"               # Optional positional arg
arg "<file>"                                  # Completed as filename
arg "<dir>"                                   # Completed as directory
```

#### With Defaults

```
arg "<file>" default="config.toml"            # Default value if not provided
arg "[output]" default="out.txt"              # Optional with default
```

#### Variadic Arguments

```
arg "[files]" var=true                        # 0 or more files
arg "<files>" var=true                        # 1 or more files (required)
arg "<files>" var=true var_min=2              # At least 2 files required
arg "<files>" var=true var_max=5              # Maximum 5 files allowed
arg "<files>" var=true var_min=1 var_max=3    # Between 1 and 3 files
```

#### Environment Variable Backing

```
arg "<token>" env="API_TOKEN"                 # Can be set via $API_TOKEN
arg "<host>" env="API_HOST" default="localhost"
```

Priority order: CLI argument > Environment variable > Default value

#### Choices (Enum Values)

```
arg "<level>" choices=["debug", "info", "warn", "error"]
arg "<shell>" {
  choices "bash" "zsh" "fish"
  help "Shell type"
}
```

#### Advanced Features

```
arg "<file>" long_help="Extended help text shown with --help"
arg "<file>" hide=true                        # Hidden from help output
arg "<input>" parse="mycli parse-input {}"    # Parse value with external command
```

#### Double-Dash Behavior

```
arg "<file>" double_dash="required"           # Must use: mycli -- file.txt
arg "<file>" double_dash="optional"           # Both work: mycli file.txt or mycli -- file.txt
arg "<files>" double_dash="automatic"         # After first arg, behaves as if -- was used
```

### Flags (`flag`)

Flags are boolean options that can be enabled/disabled.

#### Flag Syntax

```
flag "-f --force"                             # Boolean flag
flag "-v --verbose" help="Enable verbose mode"
flag "--dry-run" help="Preview without executing"
```

#### Short-Only or Long-Only

```
flag "-f"                                     # Short flag only
flag "--force"                                # Long flag only
```

#### With Values

Flags can also accept values (making them similar to options):

```
flag "--color <when>" choices=["auto", "always", "never"] default="auto"
flag "-u --user <user>" help="User to run as"
```

#### Count Flags

```
flag "-v --verbose" count=true                # Can be repeated: -vvv
                                             # $usage_verbose = number of times used (e.g., 3)
```

#### Negation

```
flag "--color" negate="--no-color" default=true
# Default: $usage_color = "true"
# With --no-color: $usage_color = "false"
```

#### Global Flags

```
flag "-v --verbose" global=true               # Available on all subcommands (if using cmd structure)
```

#### Environment Variable and Config Backing

```
flag "--color" env="MYCLI_COLOR"              # Can be set via $MYCLI_COLOR
flag "--format <fmt>" config="ui.format"      # Backed by config file value
flag "--debug" env="DEBUG" default=false
```

Priority order: CLI flag > Environment variable > Config file > Default value

#### Conditional Requirements

```
flag "--file <file>" required_if="--output"        # If --output is set, --file must be too
flag "--file <file>" required_unless="--stdin"     # Either --file or --stdin must be set
flag "--file <file>" overrides="--stdin"           # If --file is set, --stdin is ignored
```

#### Flag Advanced Features

```
flag "--verbose" long_help="Extended help text"
flag "--debug" hide=true                      # Hidden from help
flag "-q --quiet" {
  help "Suppress output"
  alias "--silent"                            # Alternative name
}
```

### Options (Flags with Values)

Options are flags that require a value. In mise's usage syntax, these are defined as flags with `<value>` placeholders.

```
option "-o --output <file>" help="Output file"
option "--format <format>" help="Output format" default="json"
option "--port <port>" help="Server port" default="8080" env="PORT"
```

Options support all the same features as flags (environment variables, config backing, choices, etc.).

### Completion (`complete`)

Custom completion can be defined for any argument or flag by name:

```
arg "<plugin>"
complete "plugin" run="mise plugins ls"       # Complete with command output
```

#### With Descriptions

```
complete "plugin" run="mycli plugins list" descriptions=true
```

Output format (split on `:` for value and description):

```
nodejs:JavaScript runtime
python:Python language
ruby:Ruby language
```

### Long Help Text

For detailed help text, use multi-line format:

```toml
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
run = 'process-data "$usage_input" --format "$usage_format"'
```

### Hide Arguments

Hide arguments from help output (useful for deprecated or internal options):

```
arg "<legacy_arg>" hide=true
flag "--internal-debug" hide=true
```

### Combining Features Example

```toml [mise.toml]
[tasks.deploy]
description = "Deploy application to cloud"
usage = '''
# Positional arguments
arg "<environment>" {
  help "Deployment environment"
  choices "dev" "staging" "prod"
}

arg "[services]" {
  help "Services to deploy (default: all)"
  var true
  var_min 0
}

# Flags
flag "-v --verbose" {
  help "Enable verbose logging"
  count true
}

flag "--dry-run" help="Show what would be deployed without doing it"

flag "--region <region>" {
  help "Cloud region"
  env "AWS_REGION"
  default "us-east-1"
  choices "us-east-1" "us-west-2" "eu-west-1"
}

flag "--skip-tests" help="Skip running tests before deploy"

flag "--force" {
  help "Force deployment even with warnings"
  required_if "--skip-tests"
}

# Custom completions
complete "services" run="mycli list-services"
'''
run = '''
#!/usr/bin/env bash
set -euo pipefail

# Handle verbosity
if [[ "${usage_verbose:-0}" -ge 2 ]]; then
  set -x
elif [[ "${usage_verbose:-0}" -ge 1 ]]; then
  export VERBOSE=1
fi

# Validate environment
ENVIRONMENT="$usage_environment"
REGION="$usage_region"
DRY_RUN="${usage_dry_run:-false}"
SKIP_TESTS="${usage_skip_tests:-false}"
FORCE="${usage_force:-false}"

echo "Deploying to $ENVIRONMENT in $REGION"

# Run tests unless skipped
if [[ "$SKIP_TESTS" != "true" ]]; then
  echo "Running tests..."
  npm test
fi

# Deploy services
if [[ -n "$usage_services" ]]; then
  echo "Deploying services: $usage_services"
  for service in $usage_services; do
    deploy_service "$service" "$ENVIRONMENT" "$REGION" "$DRY_RUN"
  done
else
  echo "Deploying all services"
  deploy_all "$ENVIRONMENT" "$REGION" "$DRY_RUN"
fi
'''
```

### 2. File Task Headers {#file-task-headers}

For file tasks, you can define arguments directly in the file using special `#MISE` or `#USAGE` comment syntax:

```bash [.mise/tasks/deploy]
#!/usr/bin/env bash
#MISE description "Deploy application"
#USAGE arg "<environment>" help="Deployment environment" choices=["dev", "staging", "prod"]
#USAGE flag "--dry-run" help="Preview changes without deploying"
#USAGE flag "--region <region>" help="AWS region" default="us-east-1" env="AWS_REGION"

ENVIRONMENT="$usage_environment"
REGION="$usage_region"
DRY_RUN="${usage_dry_run:-false}"

if [[ "$DRY_RUN" == "true" ]]; then
  echo "DRY RUN: Would deploy to $ENVIRONMENT in $REGION"
else
  echo "Deploying to $ENVIRONMENT in $REGION..."
  ./scripts/deploy.sh "$ENVIRONMENT" "$REGION"
fi
```

::: tip Syntax Options
Use `#MISE` (uppercase, recommended) or `#USAGE` for defining arguments in file tasks. The older `# mise` (lowercase) syntax is deprecated.
:::

## Deprecated Method

### Tera Template Functions <Badge type="danger" text="deprecated" /> {#tera-templates}

::: danger Deprecated - Removal in 2026.11.0
The Tera template method for defining task arguments is **deprecated** and will be **removed in mise 2026.11.0**.

**Why it's being removed:**

- **Two-pass parsing issues**: Template functions return empty strings during spec collection, causing unexpected behavior when trying to use them as normal template values
- **Complex escaping rules**: Shell escaping rules are confusing and error-prone
- **Inconsistent behavior**: Doesn't work the same way between TOML and file tasks
- **Poor user experience**: Mixes argument definitions with script logic

**Migration required:** Please migrate to the [usage field](#usage-field) method before 2026.11.0.
:::

<details>
<summary>Click to see deprecated Tera template syntax (not recommended)</summary>

Previously, you could define arguments inline in run scripts using Tera template functions:

```toml [mise.toml]
# ❌ DEPRECATED - Do not use
[tasks.test]
run = 'cargo test {{arg(name="file", default="all")}}'
```

```toml [mise.toml]
# ❌ DEPRECATED - Do not use
[tasks.build]
run = [
    'cargo build {{option(name="profile", default="dev")}}',
    './scripts/package.sh {{flag(name="verbose")}}'
]
```

**Problems with this approach:**

1. **Empty strings during parsing**: During spec collection (first pass), template functions return empty strings, so you can't use them in templates like:

   ```toml
   # This doesn't work as expected!
   run = 'echo "File: {{arg(name="file")}}" > {{arg(name="file")}}.log'
   # First pass: 'echo "File: " > .log' (invalid!)
   ```

2. **Escaping complexity**: Different shell types require different escaping:

   ```toml
   # Escaping behavior varies by shell
   run = 'cmd {{arg(name="file")}}' # May or may not be properly escaped
   ```

3. **No help generation**: Doesn't generate proper `--help` output

</details>

### Migration Guide

Here's how to migrate from Tera templates to the usage field:

#### Example 1: Simple Arguments

<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">

<div>

**Old (Deprecated):**

```toml
[tasks.test]
run = '''
cargo test {{arg(
  name="file",
  default="all",
  help="Test file"
)}}
'''
```

</div>

<div>

**New (Preferred):**

```toml
[tasks.test]
usage = 'arg "<file>" help="Test file" default="all"'
run = 'cargo test $usage_file'
```

</div>

</div>

#### Example 2: Multiple Arguments with Flags

<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">

<div>

**Old (Deprecated):**

```toml
[tasks.build]
run = [
  'cargo build {{arg(name="target", default="debug")}}',
  './package.sh {{flag(name="verbose")}}'
]
```

</div>

<div>

**New (Preferred):**

```toml
[tasks.build]
usage = '''
arg "<target>" default="debug"
flag "-v --verbose"
'''
run = [
  'cargo build $usage_target',
  './package.sh $usage_verbose'
]
```

</div>

</div>

#### Example 3: Options with Choices

<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">

<div>

**Old (Deprecated):**

```toml
[tasks.deploy]
run = '''
deploy {{option(
  name="env",
  choices=["dev", "prod"]
)}} {{flag(name="force")}}
'''
```

</div>

<div>

**New (Preferred):**

```toml
[tasks.deploy]
usage = '''
option "--env <env>" choices=["dev", "prod"]
flag "--force"
'''
run = 'deploy --env $usage_env $usage_force'
```

</div>

</div>

#### Example 4: Variadic Arguments

<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">

<div>

**Old (Deprecated):**

```toml
[tasks.lint]
run = 'eslint {{arg(name="files", var=true)}}'
```

</div>

<div>

**New (Preferred):**

```toml
[tasks.lint]
usage = 'arg "<files>" var=true'
run = 'eslint $usage_files'
```

</div>

</div>

## See Also

- [Task Configuration](/tasks/task-configuration) - Complete task configuration reference
- [TOML Tasks](/tasks/toml-tasks) - TOML task syntax
- [File Tasks](/tasks/file-tasks) - File-based task syntax
- [Running Tasks](/tasks/running-tasks) - How to execute tasks
- [Usage Spec Documentation](https://usage.jdx.dev/spec/) - Complete usage specification reference
