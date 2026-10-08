---
description: "Render config values with Tera templates: the variables, functions, filters, and tests available in mise.toml, tasks, and .miserc.toml."
socialDescription: "Use the variables, functions, filters, and tests of Tera templates in mise config."
---

# Tera templates

mise renders [Tera](https://keats.github.io/tera/) templates in config values,
so a value can come from the project directory, the environment,
[`[vars]`](/configuration/vars.html), or a command's output. These string
templates are different from [task templates](/tasks/templates.html), which
share task definitions through `extends`.

## Where templates are rendered

| File or section                                             | What is rendered                                                                                                                                                | Extra context                                                 |
| ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `mise.toml`                                                 | String values in most sections, such as `[env]`, `[tools]`, `[vars]`, `[hooks]`, and `[plugins]`. Keys are not rendered, and the file must be valid TOML first. | `config_source`                                               |
| `[bootstrap]`                                               | Some values only; see [bootstrap templates](/bootstrap.html#templates)                                                                                          | None                                                          |
| Task fields, such as `run`                                  | When the task runs                                                                                                                                              | `usage`, `tools`, [`task_source_files()`](#task-source-files) |
| [Dotfiles](/dotfiles/managed.html) with `mode = "template"` | The source file's content                                                                                                                                       | See the dotfiles page                                         |
| `.tool-versions`                                            | The whole file                                                                                                                                                  | `config_source`                                               |
| `.miserc.toml`                                              | The whole file, before it is parsed                                                                                                                             | Limited; see [below](#miserc-template-support)                |

## Example

```toml [mise.toml]
[env]
PROJECT_NAME = "{{ config_root | basename }}"
TERRAFORM_VERSION = "1.9"

[tools]
# an [env] value defined above
terraform = "{{ env.TERRAFORM_VERSION }}"
# NODE_VERSION from your shell, or 24 when it is unset
node = "{{ env.NODE_VERSION | default(value='24') }}"
```

In a project directory named `myproj`, `mise env` includes
`export PROJECT_NAME=myproj`. Use `config_root` for paths inside the project:
it stays at the project root when you run mise from a subdirectory, while `cwd`
is the directory you ran mise from.

## Syntax

Templates use Tera v2 syntax with three delimiters:

- <span v-pre>`{{ ... }}`</span> for expressions
- <span v-pre>`{% ... %}`</span> for statements such as `if` and `for`
- <span v-pre>`{# ... #}`</span> for comments

Use a `raw` block to keep delimiters from being rendered:

<div v-pre>

```text
{% raw %}
  Hello {{ name }}
{% endraw %}
```

</div>

This renders as <span v-pre>`Hello {{ name }}`</span>.

| Kind             | Syntax                                                                   | Example                                               |
| ---------------- | ------------------------------------------------------------------------ | ----------------------------------------------------- |
| Literals         | Strings in `''`, `""`, or backticks; numbers; `true`, `false`; `[lists]` | <span v-pre>`{{ ["a", "b"] }}`</span>                 |
| Attribute access | `a.b`, `a["b"]`                                                          | <span v-pre>`{{ env.HOME }}`</span>                   |
| Math             | `+ - * / %`                                                              | <span v-pre>`{{ (num_cpus() \| int) * 2 }}`</span>    |
| Comparison       | `== != < <= > >=`                                                        | <span v-pre>`{{ os() == "linux" }}`</span>            |
| Logic            | `and or not`                                                             | <span v-pre>`{{ a and not b }}`</span>                |
| Concatenation    | `~`                                                                      | <span v-pre>`{{ config_root ~ "/bin" }}`</span>       |
| Membership       | `in`                                                                     | <span v-pre>`{{ os() in ["linux", "macos"] }}`</span> |

Apply a filter with `|`, as in <span v-pre>`{{ name | lower }}`</span>. Call a
function with named arguments, as in
<span v-pre>`{{ exec(command="date") }}`</span>. Check a value with `is`, as in
<span v-pre>`{% if path is dir %}`</span>. Filters can be chained. See Tera's
[expressions](https://keats.github.io/tera/#expressions) and
[control structures](https://keats.github.io/tera/#control-structures) for the
full syntax.

Tera v2 also has slices (`parts[0:2]`, `parts[-1]`), spread
(`[first, ...rest]`, `{...base, "key": value}`), list comprehensions
(`[t.name for t in tools if t.active]`), optional chaining
(`env?.NODE_ENV or "development"`), and conditional expressions
(`"prod" if release else "dev"`).

## Variables {#variables}

| Variable                                                               | Value                                                                                                                                                                                                         | Available in                                                                     |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `env`                                                                  | Environment variables: the environment mise started with, plus `[env]` values resolved so far                                                                                                                 | Everywhere                                                                       |
| `vars`                                                                 | [Config variables](/configuration/vars.html)                                                                                                                                                                  | `mise.toml` and tasks                                                            |
| `cwd`                                                                  | The directory mise runs in                                                                                                                                                                                    | Everywhere                                                                       |
| `config_root`                                                          | The [config root](/configuration.html#config-root) of the file: the project directory, even for `.config/mise.toml`. In `[bootstrap.files]` content templates, the directory that contains the declaring file | Everywhere                                                                       |
| `config_source`                                                        | Absolute path of the config file the template is in, as reached; symlinks are not resolved                                                                                                                    | `mise.toml` values, `.tool-versions`, `[env]` directives, and a task's own `env` |
| `mise_bin`                                                             | Path of the running mise executable                                                                                                                                                                           | Everywhere except `.miserc.toml`                                                 |
| `mise_pid`                                                             | Process ID of the running mise                                                                                                                                                                                | Everywhere except `.miserc.toml`                                                 |
| `mise_env`                                                             | List of the [config environments](/configuration/environments.html) you selected. Undefined when none is selected, so check it with <span v-pre>`{% if mise_env is defined %}`</span>                         | Everywhere except `.miserc.toml`                                                 |
| `xdg_cache_home`, `xdg_config_home`, `xdg_data_home`, `xdg_state_home` | XDG base directories                                                                                                                                                                                          | Everywhere                                                                       |
| `tools`                                                                | Active tools that are installed (see below)                                                                                                                                                                   | Tasks, and `[env]` directives with `tools = true`                                |
| `usage`                                                                | Parsed task arguments; see [Task arguments](#task-arguments)                                                                                                                                                  | Task `run` scripts                                                               |

Task fields other than `env`, such as `run` and `dir`, and file task headers
have `config_root` but not `config_source`.

`config_source` lets a shared config symlinked into `conf.d` add its own `bin`
directory to `PATH`:

```toml
[env]
_.path = "{{ config_source | canonicalize | dirname }}/bin"
```

Leave out `canonicalize` to get the directory the file was reached through
instead of the one it lives in.

`tools` maps each active tool to its installed active version. It is keyed by
the name used in config, such as `npm:prettier`, and by the tool's name within
its backend, such as `prettier`. Versions that are installed but not selected,
and selected versions that are not installed, are left out.

- When one version is active: `tools.node.version` is the resolved version, such
  as `"24.1.0"`, and `tools.node.path` is the install path.
- When several versions are active, such as `python = ["3.14", "3.13"]`, the
  value is a list: `tools.python[0].version`, `tools.python[1].path`, and so on.

## Functions {#functions}

`[]` marks optional arguments.

| Function                                                  | Returns | Description                                                                                                                                                                                                                     |
| --------------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `exec(command, [cache_key], [cache_duration])`            | string  | Runs `command` with the default inline shell and returns its output without the trailing newline. See [`exec()`](#exec).                                                                                                        |
| `read_file(path)`                                         | string  | Returns a file's contents. Relative paths resolve from the template's directory, not from `cwd`; see [`exec()`](#exec).                                                                                                         |
| `get_env(name, [default])`                                | string  | Returns a variable from the environment mise started with, or `default` when it is unset. An empty value is returned as is. It does not see `[env]` values; prefer <span v-pre>`{{ env.NAME \| default(value="...") }}`</span>. |
| `arch([x64=...], [arm64=...])`                            | string  | The CPU architecture: `x64`, `arm64`, or another Rust architecture name. Pass a keyword named after an architecture to rename it, as in <span v-pre>`{{ arch(x64="amd64", arm64="aarch64") }}`</span>.                          |
| `os([linux=...], [macos=...], [windows=...])`             | string  | The operating system: `linux`, `macos`, or `windows`. Keywords rename it the same way, as in <span v-pre>`{{ os(macos="darwin") }}`</span>.                                                                                     |
| `os_family()`                                             | string  | `unix` or `windows`                                                                                                                                                                                                             |
| `num_cpus()`                                              | string  | The number of CPUs. Use <span v-pre>`{{ (num_cpus() \| int) * 2 }}`</span> for arithmetic.                                                                                                                                      |
| `choice(n, alphabet)`                                     | string  | `n` characters sampled from `alphabet`. `choice(n=64, alphabet="0123456789abcdef")` is a random 64-character hex string.                                                                                                        |
| `haiku([words], [separator], [digits])`                   | string  | A random readable name such as `blazing-shadow-46`. Defaults: 2 words, `-`, 2 digits.                                                                                                                                           |
| `now([timezone])`                                         | string  | The current date and time. The time zone defaults to UTC and accepts IANA names such as `America/New_York`. Format it with `date`: <span v-pre>`{{ now() \| date(format="%Y") }}`</span>.                                       |
| `get_random(start, end, [seed])`                          | number  | A random integer in a range. A `seed` makes the result repeatable.                                                                                                                                                              |
| `range(end, [start], [step_by])`                          | list    | Integers from `start` (default 0) up to, but not including, `end`.                                                                                                                                                              |
| `throw(message)`                                          | none    | Stops rendering with an error.                                                                                                                                                                                                  |
| [`task_source_files([only_changed])`](#task-source-files) | list    | A task's source files. Tasks only.                                                                                                                                                                                              |

### exec() and read_file() {#exec}

`exec()` runs, and relative `read_file()` paths resolve, in the directory of the
config file that holds the template. For `.config/mise.toml` that is `.config/`,
not the project root. In task run scripts it is the task's working directory
(its `dir`, which defaults to the config root). It is never the directory you
ran mise from. In `[env]`, `exec()` also sees variables set by earlier `[env]`
entries; elsewhere it sees the environment mise started with.

```toml
[env]
GIT_SHA = "{{ exec(command='git rev-parse --short HEAD') }}"
VERSION = "{{ read_file(path='VERSION') | trim }}"
```

Pass `cache_key` or `cache_duration` to cache the output under
`MISE_CACHE_DIR`. The cache is keyed by the command, its directory, and
`cache_key`, so changing `cache_key` runs the command again. `cache_duration`,
such as `"1h"` or `"1d"`, makes the cached output expire; without it, mise
reuses the output until the cache is cleared:

```toml
[env]
AWS_ACCOUNT_ID = "{{ exec(command='aws sts get-caller-identity --query Account --output text', cache_key='aws-account', cache_duration='1d') }}"
```

Both functions need a [trusted](/security.html#configuration-trust) config. In
[safe mode](/security.html#safe-mode) (`MISE_SAFE=1`), they fail with an error
instead of running, and neither is available in `.miserc.toml`.

::: warning
`exec()` runs every time its template renders, including under `--dry-run`.
Use it only for commands without side effects.
`mise bootstrap --dry-run` renders `[bootstrap.hooks]` commands with `exec()`
disabled, so a hook command that calls `exec()` stops the preview with
`exec() is disabled during dry run`. `[vars]` and `[env]` are still rendered
when the config loads, so their `exec()` calls run even under `--dry-run`.
:::

## Filters {#filters}

`[]` marks optional arguments. Tera's own filters also work; see the
[Tera documentation](https://keats.github.io/tera/#built-in-filters).

### Strings {#string-manipulation}

| Filter                                                                                             | Description                                                                                                                                                                                            |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `quote`                                                                                            | Quotes a string for a POSIX shell. Embedded single quotes use the `'\''` form, so `it's` becomes `'it'\''s'`.                                                                                          |
| `lower`, `upper`                                                                                   | Lowercase or uppercase                                                                                                                                                                                 |
| `capitalize`                                                                                       | Uppercases the first character and lowercases the rest                                                                                                                                                 |
| `title`                                                                                            | Capitalizes each word: `"foo bar"` becomes `Foo Bar`                                                                                                                                                   |
| `replace(from, to)`                                                                                | Replaces every `from` with `to`                                                                                                                                                                        |
| `trim`, `trim_start([pat])`, `trim_end([pat])`                                                     | Removes whitespace, or `pat`, from both ends, the start, or the end: <span v-pre>`{{ "v1.2.3" \| trim_start(pat="v") }}`</span> gives `1.2.3`                                                          |
| `truncate(length, [end])`                                                                          | Cuts a string to `length` characters and appends `end` (default `…`)                                                                                                                                   |
| `split(pat)`                                                                                       | Splits a string into a list                                                                                                                                                                            |
| `regex_replace(pattern, rep)`                                                                      | Replaces regular expression matches                                                                                                                                                                    |
| `default(value, [boolean])`                                                                        | Returns `value` when the input is undefined. A defined but empty or false input is kept unless you pass `boolean=true`, as in <span v-pre>`{{ env.NAME \| default(value="x", boolean=true) }}`</span>. |
| `kebabcase`, `lowercamelcase`, `uppercamelcase`, `snakecase`, `shoutysnakecase`, `shoutykebabcase` | Converts case: `kebab-case`, `lowerCamelCase`, `UpperCamelCase`, `snake_case`, `SHOUTY_SNAKE_CASE`, `SHOUTY-KEBAB-CASE`                                                                                |
| `slug`, `slugify`                                                                                  | Converts a string to a URL-friendly slug                                                                                                                                                               |
| `urlencode`, `urlencode_strict`                                                                    | Percent-encodes a string for URLs; `urlencode_strict` encodes every character that is not alphanumeric                                                                                                 |
| `b64_encode([url_safe], [padded])`, `b64_decode([url_safe])`                                       | Encodes or decodes base64                                                                                                                                                                              |
| `striptags`, `spaceless`                                                                           | Removes HTML tags, or whitespace between HTML tags                                                                                                                                                     |

`quote` always quotes for POSIX shells, not for PowerShell or cmd. The one
exception is a [daemon](/daemons.html) `run` command on Windows, which pitchfork
runs with `cmd /C`; there `quote` quotes for cmd.

Use `quote` when you insert a template value into a shell command. A quoted
value can be joined with unquoted text in the same argument:

```mise-toml
[tasks.create-config]
run = "touch {{ config_root | quote }}/generated.toml"
```

### Paths {#path-manipulation}

| Filter          | Returns | Description                                                                                  |
| --------------- | ------- | -------------------------------------------------------------------------------------------- |
| `absolute`      | string  | Makes a path absolute. The path does not have to exist.                                      |
| `canonicalize`  | string  | Makes a path absolute and resolves symlinks. Fails when the path does not exist.             |
| `dirname`       | string  | `/foo/bar/baz.txt` becomes `/foo/bar`                                                        |
| `basename`      | string  | `/foo/bar/baz.txt` becomes `baz.txt`                                                         |
| `extname`       | string  | `/foo/bar/baz.txt` becomes `txt`, without a leading dot                                      |
| `file_stem`     | string  | `/foo/bar/baz.txt` becomes `baz`                                                             |
| `file_size`     | number  | The file's size in bytes                                                                     |
| `last_modified` | number  | The file's modification time, in seconds since the Unix epoch                                |
| `join_path`     | string  | Joins a list of path parts: <span v-pre>`{{ [config_root, "bar.txt"] \| join_path }}`</span> |

### Hashing {#hash}

| Filter                     | Description                                                                                                                      |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `hash([algorithm], [len])` | Hashes a string with `sha256` (default) or `blake3`. `len` truncates the result: <span v-pre>`{{ "foo" \| hash(len=8) }}`</span> |
| `hash_file([len])`         | The BLAKE3 hash of the file at a path. `len` truncates the result.                                                               |

### Lists, numbers, and formatting {#collections}

| Filter                              | Description                                                                                                                                                                |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `first`, `last`                     | The first or last item of a list                                                                                                                                           |
| `join(sep)`                         | Joins a list into a string: <span v-pre>`{{ ["a", "b"] \| join(sep=", ") }}`</span> gives `a, b`                                                                           |
| `length`                            | The length of a string or list                                                                                                                                             |
| `reverse`                           | Reverses a string or list                                                                                                                                                  |
| `shuffle([seed])`                   | Shuffles a list                                                                                                                                                            |
| `int`, `float`                      | Converts a value to a number                                                                                                                                               |
| `abs`                               | The absolute value of a number                                                                                                                                             |
| `filesize_format`, `filesizeformat` | Formats a number of bytes as a readable size                                                                                                                               |
| `date(format, [timezone])`          | Formats a timestamp: <span v-pre>`{{ ts \| date(format="%Y-%m-%d") }}`</span>. See the [`jiff` format reference](https://docs.rs/jiff/latest/jiff/fmt/strtime/index.html). |
| `format(spec)`                      | Formats a value with Rust-style formatting                                                                                                                                 |
| `json_encode([pretty])`             | Encodes a value as JSON                                                                                                                                                    |

## Tests {#tests}

Use a test with `is`, as in <span v-pre>`{% if os() is starting_with(pat="mac") %}`</span>.
Tests take named arguments.
Tera's own tests also work; see the
[Tera documentation](https://keats.github.io/tera/#built-in-tests).

| Test                                                        | True when                                                                                                                                                            |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `defined`                                                   | The variable is defined                                                                                                                                              |
| `string`, `number`                                          | The value is a string, or a number                                                                                                                                   |
| `starting_with(pat)`, `ending_with(pat)`, `containing(pat)` | The value starts with, ends with, or contains `pat`                                                                                                                  |
| `divisible_by(divisor)`                                     | The number is divisible by `divisor`                                                                                                                                 |
| `matching(pat)`                                             | The string matches the regular expression `pat`                                                                                                                      |
| `dir`, `file`, `exists`                                     | The path is a directory, is a file, or exists                                                                                                                        |
| `before(other, [inclusive])`, `after(other, [inclusive])`   | The date is before or after `other`: <span v-pre>`{% if release_date is after(other="2026-01-01") %}`</span>                                                         |
| `semver_matching(requirement)`                              | The version matches a semver requirement, as in <span v-pre>`{% if v is semver_matching(requirement=">=1.2") %}`</span>. Only meaningful for semver-shaped versions. |

<div v-pre>

```toml
[env]
VENV_STATUS = "{% if config_root ~ '/.venv' is dir %}present{% else %}missing{% endif %}"
```

</div>

## Templates in tasks

Task `run` scripts are rendered when the task runs, so they can use the
[`tools`](#variables) map and the helpers below.

### Task arguments {#task-arguments}

When a task has a [usage spec](/tasks/task-arguments.html), its `run` script can
read the parsed arguments from `usage`. Keys are the argument and flag names in
snake_case, so `--dry-run` is <span v-pre>`{{ usage.dry_run }}`</span>, the same
name as the `$usage_dry_run` environment variable. Values are booleans for flags,
strings, integers for count flags (`count=#true`, so `-vvv` gives `3`), and lists
for variadic arguments.

Values are not quoted for the shell. Double quotes around a template expression
do not keep its value from becoming shell syntax, so pipe strings through
`quote`:

```mise-toml
[tasks.deploy]
usage = '''
arg "<environment>" help="Target environment"
flag "-v --verbose" help="Enable verbose output"
arg "[tags]" var=#true
'''
run = '''
printf 'env=%s\n' {{ usage.environment | quote }}
printf 'verbose=%s\n' {{ usage.verbose | str | quote }}
printf 'tag count=%s\n' {{ usage.tags | length | str | quote }}
{% for tag in usage.tags %}
printf 'tag=%s\n' {{ tag | quote }}
{% endfor %}
'''
```

`mise run deploy prod -v a b` prints:

```text
env=prod
verbose=true
tag count=2
tag=a
tag=b
```

### task_source_files() {#task-source-files}

`task_source_files([only_changed])` returns the files matched by the task's
[`sources`](/tasks/task-configuration.html#sources), with globs and templates
expanded. Patterns that match nothing are left out, and a task without sources
gets an empty list.

```mise-toml
[tasks.list-sources]
sources = ["src/**/*.ts", "package.json"]
run = '''
{% for file in task_source_files() %}
printf 'Processing: %s\n' {{ file | quote }}
{% endfor %}
'''
```

With `only_changed=true`, it returns only the sources modified since the task
last succeeded, which keeps linters and formatters fast:

```mise-toml
[tasks.lint]
sources = ["src/**/*.ts"]
run = "eslint{% for file in task_source_files(only_changed=true) %} {{ file | quote }}{% endfor %}"
```

It returns every source when the task has never succeeded, and when no source
changed but the task runs anyway, such as with `--force` or because a dependency
did work. A failed run does not reset the baseline, so the same files stay in
the list until the task passes. Changes are detected by modification time, so
`touch` and restored caches affect the result, as they do for
[task caching](/tasks/caching.html).

### Secrets in a task's env <Badge type="warning" text="experimental" /> {#secrets}

::: warning Experimental
mise secrets are experimental. Enable them with `experimental = true` under
`[settings]`.
:::

In a task's own `env` values, <span v-pre>`{{ secrets.NAME }}`</span> renders a
mise secret when the task starts and grants that key to the task. `secrets` is
not available anywhere else. See
[Compose values](/environments/secrets/fnox.html#compose-values).

### Parameterized snippets with components {#parameterized-snippets-with-components}

A [Tera component](https://keats.github.io/tera/#components) expands a snippet
several times with different arguments inside one task's script:

```mise-toml
[tasks.restart]
run = """
{% component restart(service, region="us-east-1") %}
aws ecs update-service --region {{ region | quote }} --cluster prod --service {{ service | quote }} --force-new-deployment
{% endcomponent %}
{{ <restart service="api" /> }}
{{ <restart service="worker" /> }}
{{ <restart service="api" region="eu-west-1" /> }}
"""
```

Each call expands to one command, which runs with the task's tools,
environment, working directory, and shell. `service` is required and `region`
defaults to `us-east-1`. String arguments use quotes; pass an expression in
braces, such as `region={vars.region}`.

Define a component in the same template string that calls it; mise has no
shared component library. A component defined in `[vars]` is rendered when the
config loads, not when the task runs (see
[when vars are resolved](/configuration/vars.html#when-vars-are-resolved)). To
share a whole task, use a [task template](/tasks/templates.html); to share shell
logic across scripts, source a shell file. Components need the default Tera v2
engine and do not work with `tera_v1`.

## Templates in .miserc.toml {#miserc-template-support}

mise renders [`.miserc.toml`](/configuration.html#miserc) before it reads
`mise.toml` or settings, so only information from the operating system is
available:

- `env`: the environment mise started with
- `config_root`: the directory that holds the `.miserc.toml` file
- `cwd`
- `xdg_cache_home`, `xdg_config_home`, `xdg_data_home`, `xdg_state_home`
- Functions that need no project context, such as `arch()`, `os()`,
  `os_family()`, `num_cpus()`, `choice()`, and `get_env()`
- All [filters](#filters) and [tests](#tests)

Not available: `mise_env` (which `.miserc.toml` itself sets), `exec()`,
`read_file()`, `mise_bin`, `mise_pid`, `vars`, and `tools`.

<div v-pre>

```toml [~/.config/mise/miserc.toml]
# Stop the config search at your home directory
ceiling_paths = ["{{ env.HOME }}"]

# Ignore a config file under your XDG config directory
ignored_config_paths = ["{{ xdg_config_home }}/mise/shared.toml"]
```

</div>

A block that renders to nothing leaves empty lines, which TOML ignores, so
conditionals work:

<div v-pre>

```text [~/.config/mise/miserc.toml]
{% if os() == "linux" %}
ceiling_paths = ["{{ env.HOME }}/work"]
{% endif %}
```

</div>

If a template fails to render, for example because of an undefined variable or
a call to `exec()`, mise silently uses the file's raw, unrendered content. No
warning is printed, because mise reads `.miserc.toml` before logging starts. To
keep a literal <span v-pre>`{{`</span>, use a [`raw` block](#syntax).

## Migrating from Tera v1 {#tera-v2-migration}

mise renders templates with Tera v2. It still accepts most Tera v1 helpers, but
since mise 2026.10.0 it warns when a template uses one, and mise 2027.4.0
removes them. Replace them as follows:

| Tera v1 pattern                                                                               | Tera v2 replacement                                                           |
| --------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `value \| trim_start_matches(pat="v")`                                                        | `value \| trim_start(pat="v")`                                                |
| `value \| trim_end_matches(pat="-beta")`                                                      | `value \| trim_end(pat="-beta")`                                              |
| `items \| slice(start=0, end=2)`                                                              | `items[0:2]`                                                                  |
| `[base] \| concat(with="file.txt")`                                                           | `[base, "file.txt"]`                                                          |
| `[...items] \| concat(with=extra_items)`                                                      | `[...items, ...extra_items]`                                                  |
| `items \| map(attribute="name")`                                                              | `[item.name for item in items]`                                               |
| `items \| filter(attribute="active")`                                                         | `[item for item in items if item.active]`                                     |
| `value \| as_str`                                                                             | `value \| str`                                                                |
| `value \| escape`                                                                             | `value \| escape_html`                                                        |
| `value \| linebreaksbr`                                                                       | `value \| newlines_to_br`                                                     |
| `value \| addslashes`                                                                         | an explicit `replace(from=..., to=...)`                                       |
| `value is divisibleby(divisor=3)`                                                             | `value is divisible_by(divisor=3)`                                            |
| `value is object`                                                                             | `value is map`                                                                |
| `value \| indent(prefix=">")`                                                                 | `value \| indent(width=1)`, for spaces only                                   |
| `value \| truncate`                                                                           | `value \| truncate(length=255)`                                               |
| `value \| int(default=0)`, `float(default=0)`, or `int`/`float` on input that is not a number | Validate the input first; Tera 2 errors instead of returning the default or 0 |
| `items \| first`, `last`, or `nth(n=...)` on an empty or short list                           | Check `items \| length` first; Tera 2 returns null instead of `""`            |

mise's `unique` filter still compares case-insensitively, as Tera v1 did, until
2027.4.0, when it becomes Tera 2's case-sensitive `unique`.
`unique(case_sensitive=...)` warns and has no replacement that keeps
case-insensitive matching.

`now()` takes `[timezone]` in Tera v2. Under `tera_v1`, the original
`now([timestamp], [utc])` signature applies instead.

Some Tera v1 behavior has no v2 equivalent: v2 fails on undefined variables and
has no macros (use [components](#parameterized-snippets-with-components)).
Until you can update such a template, set
[`tera_v1`](/configuration/settings.html#tera_v1) to render every template with
Tera v1. In a shared `mise.toml`, set it as an environment variable, which older
mise releases ignore instead of rejecting as an unknown setting:

```toml
[env]
MISE_TERA_V1 = true
```

Enabling `tera_v1` prints a deprecation warning, and mise 2027.4.0 removes it.
It does not apply to `.miserc.toml`, which mise renders before it reads
settings.
