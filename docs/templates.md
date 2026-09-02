# Templates

Templates in mise provide a powerful way to configure different aspects of
your environment and project settings.

A template is a string that contains variables, expressions, and control structures.
When rendered, the template engine (`tera`) replaces the variables with their values.

You can define and use templates in the following locations:

- Most `mise.toml` configuration values
  - The `mise.toml` file itself is not templated and must be valid TOML
- `.tool-versions` files
- `.miserc.toml` files (limited context — see [Template Support in .miserc.toml](#miserc-template-support))

## Example

Here is an example of a `mise.toml` file that uses templates:

```toml
[env]
PROJECT_NAME = "{{ cwd | basename }}"
TERRAFORM_VERSION = "1.0.0"

[tools]
# refers to env variable defined in this file
terraform = "{{ env.TERRAFORM_VERSION }}"
# refers to external env variable
node = "{{ get_env(name='NODE_VERSION', default='20') }}"
```

You will find more examples in the [cookbook](./mise-cookbook/index.md).

## Template Rendering

mise uses [tera](https://keats.github.io/tera/) to provide the template feature.
Templates use three kinds of delimiters:

- <span v-pre>`{{`</span> and <span v-pre>`}}`</span> for expressions
- <span v-pre>`{%`</span> and <span v-pre>`%}`</span> for statements
- <span v-pre>`{#`</span> and <span v-pre>`#}`</span> for comments

Use a `raw` block to keep tera delimiters from being rendered:

<div v-pre>

```
{% raw %}
  Hello {{ name }}
{% endraw %}
```

</div>

This renders as <span v-pre>`Hello {{ name }}`</span>.

Tera supports [literals](https://keats.github.io/tera/#literals), including:

- booleans: `true` (or `True`) and `false` (or `False`)
- integers
- floats
- strings: text delimited by `""`, `''` or <code>\`\`</code>
- arrays: a comma-separated list of literals and/or identifiers surrounded by
  `[` and `]` (trailing comma allowed)

Render a variable with <span v-pre>`{{ name }}`</span>.
For nested attributes, use:

- dot `.`, e.g. <span v-pre>`{{ product.name }}`</span>
- square brackets `[]`, e.g. <span v-pre>`{{ product["name"] }}`</span>

Tera also supports powerful [expressions](https://keats.github.io/tera/#expressions):

- mathematical expressions
  - `+`
  - `-`
  - `/`
  - `*`
  - `%`
- comparisons
  - `==`
  - `!=`
  - `>=`
  - `<=`
  - `<`
  - `>`
- logic
  - `and`
  - `or`
  - `not`
- concatenation `~`, e.g. <code v-pre>{{ "hello " ~ 'world' ~ \`!\` }}</code>
- `in` membership checks, e.g. <span v-pre>`{{ some_var in [1, 2, 3] }}`</span>

Tera also supports [control structures such as <span v-pre>`if`</span> and
<span v-pre>`for`</span>](https://keats.github.io/tera/#control-structures).

### Tera v2 Migration

mise uses Tera v2. Some Tera v1 syntax and built-ins changed in Tera v2. mise
can still render many older templates for compatibility. Tera v1 compatibility
helpers will start warning in mise 2026.10.0 and are scheduled for removal in
mise 2027.4.0.

Prefer these Tera v2 forms in new templates:

| Tera v1 pattern                          | Tera v2 replacement                        |
| ---------------------------------------- | ------------------------------------------ |
| `value \| trim_start_matches(pat="v")`   | `value \| trim_start(pat="v")`             |
| `value \| trim_end_matches(pat="-beta")` | `value \| trim_end(pat="-beta")`           |
| `items \| slice(start=0, end=2)`         | `items[0:2]`                               |
| `[base] \| concat(with="file.txt")`      | `[base, "file.txt"]`                       |
| `[...items] \| concat(with=extra_items)` | `[...items, ...extra_items]`               |
| `items \| map(attribute="name")`         | `[item.name for item in items]`            |
| `items \| filter(attribute="active")`    | `[item for item in items if item.active]`  |
| `value \| as_str`                        | `value \| str`                             |
| `value \| escape`                        | `value \| escape_html`                     |
| `value \| linebreaksbr`                  | `value \| newlines_to_br`                  |
| `value is divisibleby(divisor=3)`        | `value is divisible_by(divisor=3)`         |
| `value is object`                        | `value is map`                             |
| `value \| indent(prefix=">")`            | `value \| indent(width=1)` for spaces only |
| `value \| truncate`                      | `value \| truncate(length=255)`            |

Tera v2 also adds useful syntax that replaces many old helper filters:

- array and string slices, such as `parts[0:2]`, `parts[-1]`, and `name[::-1]`
- array and map spread, such as `[first, ...rest]` and `{...base, key: value}`
- list comprehensions, such as `[tool.name for tool in tools if tool.active]`
- optional chaining, such as `env?.NODE_ENV or "development"`
- ternaries, such as `"prod" if release else "dev"`

Not every Tera v1 behavior can be made compatible. Undefined variable access is
stricter in Tera v2, and Tera v1 macros are not supported by mise templates.
As a temporary escape hatch, set `MISE_TERA_V1=1` before running mise to render
templates with Tera v1. In shared `mise.toml` files, prefer the backward-compatible
env form because older mise releases treat it as a normal environment variable
instead of failing on an unknown setting:

```toml
[env]
MISE_TERA_V1 = true
```

The newer `[settings] tera_v1 = true` form also works in mise releases that
support it, but is less compatible with older releases. When enabled, all regular
config and task templates use the actual Tera v1 engine and its original syntax
and built-ins. Without it, templates use Tera v2 and the helpers described below.
This escape hatch is scheduled for removal in mise 2027.4.0. Because miserc files
are rendered before settings are loaded, it does not apply while loading miserc
itself.

### Tera Filters

You can modify variables with [filters](https://keats.github.io/tera/#filters).
Apply a filter with a pipe symbol (`|`); filters may take named arguments
in parentheses, and multiple filters can be chained.
For example, <span v-pre>`{{ "Doctor Who" | lower | replace(from="doctor", to="Dr.") }}`</span>
outputs `Dr. who`.

### Tera Functions

[Functions](https://keats.github.io/tera/#functions) provide
additional features to templates.

### Tera Tests

You can also use [tests](https://keats.github.io/tera/#tests) to examine variables.

```
{% if my_number is not odd %}
  Even
{% endif %}
```

## Mise Template Features

mise provides additional variables, functions, filters, and tests on top of tera's.

### Variables

mise exposes several [variables](https://keats.github.io/tera/#variables)
with information about the current environment:

- `env: HashMap<String, String>` – Accesses current environment variables as
  a key-value map.
- `vars: HashMap<String, String>` – Accesses user-defined [configuration variables](/configuration/vars).
- `cwd: PathBuf` – Points to the current working directory.
- `config_root: PathBuf` – Points to the directory containing your `mise.toml` file; for a config such as `~/src/myproj/.config/mise.toml`, it points to `~/src/myproj`.
- `config_source: String` – The config file the template itself is written in, as an absolute path. Unlike `config_root` this is the file, not the project it belongs to, and it is **not** resolved through symlinks — pipe it through `canonicalize` when you want the location of the real file. Available in `mise.toml`, `.tool-versions`, `[env]` directives and `[settings.age]`; task file templates and `.miserc.toml` only carry `config_root`.

  With it, a shared config symlinked into `conf.d` can add its own `bin` directory
  to the path:

  ```toml
  [env]
  _.path = "{{ config_source | canonicalize | dirname }}/bin"
  ```

  Leave `canonicalize` out to get the directory the file was reached through
  rather than the one it lives in.

- `mise_bin: String` - Points to the current mise executable
- `mise_pid: String` - The PID of the current mise process
- `mise_env: Vec<String>` - The configuration environment as specified by `MISE_ENV`, `-E`, or `--env`. Undefined if no configuration environment is set.
- `xdg_cache_home: PathBuf` - Points to the XDG cache home directory
- `xdg_config_home: PathBuf` - Points to the XDG config home directory
- `xdg_data_home: PathBuf` - Points to the XDG data home directory
- `xdg_state_home: PathBuf` - Points to the XDG state home directory
- `tools: HashMap<String, ToolInfo | ToolInfo[]>` – Maps installed tool names to their info.
  Available in task templates and env directives with `tools = true`.
  - When a single version is installed:
    - `tools.<name>.version: String` – The resolved version (e.g., `"22.1.0"`)
    - `tools.<name>.path: String` – The install path
  - When multiple versions are installed, it becomes an array:
    - `tools.<name>[0].version: String` – The first version
    - `tools.<name>[0].path: String` – The first install path
    - `tools.<name>[1].version: String` – The second version, etc.

In **task run scripts**, mise also exposes a `usage` map when the task has a usage
specification (see [Task Arguments](/tasks/task-arguments#usage-field)):

- `usage: HashMap<String, Value>` – Parsed task arguments and flags, keyed by their
  names. Values are **not shell-escaped or quoted** and may be:
  - booleans (for flags and boolean args)
  - strings
  - arrays of booleans/strings for variadic args/flags

The keys are the argument/flag names as written in the usage spec. If the name
contains `-`, use bracket access, e.g. <span v-pre>`{{ usage["dry-run"] }}`</span>.
Examples:

```mise-toml
[tasks.deploy]
usage = '''
arg "<environment>" help="Target environment"
flag "-v --verbose" help="Enable verbose output"
arg "[tags]" var=#true
'''
run = '''
echo "env={{ usage.environment }}"
echo "verbose={{ usage.verbose }}"
echo "tag count={{ usage.tags | length }}"
{% for tag in usage.tags %}
  echo "tag={{ tag }}"
{% endfor %}
'''
```

### Functions

#### Tera Built-In Functions

Tera offers many [built-in functions](https://keats.github.io/tera/#built-in-functions).
`[]` indicates an optional function argument.
Some functions:

- `range(end, [start], [step_by])` - Returns an array of integers created
  using the arguments given.
  - `end: usize`: stop before `end`, mandatory
  - `start: usize`: the starting value, defaults to `0`
  - `step_by: usize`: the increment, defaults to `1`
- `now([timezone])` - In the default Tera v2 mode, returns the current datetime
  as a string. The timezone defaults to UTC and accepts IANA names such as
  `America/New_York`.
  - Tip: use the date filter to format the result,
    e.g. <span v-pre>`{{ now() | date(format="%Y") }}`</span> gets the current year.
  - With `tera_v1 = true`, the original `now([timestamp], [utc])` signature remains
    available instead.
- `throw(message)` - Throws an error with the given message.
- `get_random(start, end, [seed])` - Returns a random integer in a range.
  Providing `seed` makes the result reproducible.

The `before` and `after` tests compare dates and accept `other` and an optional
`inclusive` argument:

<span v-pre>`{% if release_date is after(other="2026-01-01") %}...{% endif %}`</span>

Tera offers more functions. Read more in the [tera documentation](https://keats.github.io/tera/#functions).

#### Additional Mise Functions

mise offers many useful functions in addition to tera's built-ins.

##### General Functions

These functions are available in all tasks and always behave the same way regardless
of the task definition they are used in. In other words, their return values are consistent
across task definitions.

- `exec(command) -> String` – Runs a shell command and returns its output as a string.
- `get_env(name, [default]) -> String` – Returns the original process environment
  variable value by name. This helper is provided by mise for compatibility with
  older Tera templates. Prefer the `env` variable in new templates when possible.
  The `default` value is used when the environment variable is not present; empty
  environment variables are returned as-is.
- `arch() -> String` – Returns the system architecture, such as `x64` or `arm64`.
- `os() -> String` – Returns the name of the operating system,
  e.g. linux, macos, windows.
- `os_family() -> String` – Returns the operating system family, e.g. `unix`, `windows`.
- `num_cpus() -> usize` – Returns the number of CPUs available on the system.
- `choice(n, alphabet)` - Generates a string of `n` characters sampled with replacement
  from `alphabet`. For example, `choice(n=64, alphabet='0123456789abcdef')` generates a random
  64-character lowercase hex string.
- `read_file(path) -> String` – Reads the contents of a file at the given path and returns
  it as a string.

::: warning
`exec()` runs whenever its template is rendered, including during `--dry-run`
operations that evaluate configuration templates. Dry-run mode suppresses the
planned mise operation; it does not sandbox or suppress commands executed by
template functions. Keep commands passed to `exec()` free of side effects.
:::

##### Task-Specific Functions

These functions are task-specific and behave differently depending on the task they are used
in. In other words, their return values **_may_** (but are not guaranteed to) be consistent
across executions of a given _task_, and should be expected to differ across
task definitions.

For example, `task_source_files()` returns a different set of file paths depending on the [`sources`](https://mise.jdx.dev/tasks/task-configuration.html#sources) of the task it's called from.

- <span id="task-source-files">`task_source_files() -> Vec<String>`</span> – Returns the task's [`sources`](https://mise.jdx.dev/tasks/task-configuration.html#sources)
  as an array of resolved file paths. Glob patterns and Tera template strings in the task's sources
  are expanded into actual file paths. Patterns that match no files are omitted from the result.
  Returns an empty array if no sources are configured or no files match.

  Pass `only_changed=true` to narrow the result to the sources written since mise last considered
  this task up to date. This is useful for linters and formatters that are much faster when given a
  small set of files. A task mise has never seen up to date has no baseline to compare against, so
  every source is returned. A run that _fails_ does not advance the baseline, so the same files stay
  in the list until the task passes. Like mise's own source freshness checking, this compares
  modification times, so it inherits the same caveats around `touch` and restored caches.

  Filtering never narrows the result all the way to nothing: if no source changed and yet the task
  is running — `--force`, a dependency that did work, an output deleted while the sources stood
  still — every source is returned instead, because a task handed no files does none of the work it
  was run to do.

#### Examples

```toml
# Using exec to get command output
[alias.node.versions]
current = "{{ exec(command='node --version') }}"

# Using read_file to include content from a file
[env]
VERSION = "{{ read_file(path='VERSION') | trim }}"

# Access resolved source files in task scripts
[tasks.example]
sources = ["src/**/*.ts", "package.json"]
run = '''
{% for file in task_source_files() %}
  echo "Processing: {{ file }}"
{% endfor %}
'''

# Only lint what changed since this task last succeeded. Each path goes through
# `quote`, so a filename containing a space or a shell metacharacter stays one
# argument (POSIX shells — see the quote filter's note).
[tasks.lint]
sources = ["src/**/*.ts"]
run = "eslint{% for file in task_source_files(only_changed=true) %} {{ file | quote }}{% endfor %}"
```

### Exec Options

The `exec` function supports the following options:

- `command: String` – [required] The command to run.
- `cache_key: String` – The cache key under which to store the result.
  When provided, the result is cached and reused for subsequent calls.
- `cache_duration: String` – How long to cache the result, in seconds,
  minutes, hours, days, or weeks.
  e.g. `cache_duration="1d"` caches the result for 1 day.

### Filters

Tera offers many [built-in filters](https://keats.github.io/tera/#built-in-filters).
`[]` indicates an optional filter argument.
Some Tera v1 filters that were removed or renamed in Tera v2 are still supported
for compatibility until mise 2027.4.0. mise starts emitting deprecation warnings
for them in mise 2026.10.0. Helpers provided by `tera-contrib` are supported
without deprecation warnings.
Some filters:

- `str | lower -> String` – Converts a string to lowercase.
- `str | upper -> String` – Converts a string to uppercase.
- `str | capitalize -> String` – Lowercases a string except for its first character,
  which is uppercased.
- `str | replace(from, to) -> String` – Replaces all instances of `from` with `to`,
  e.g., <span v-pre>`{{ name | replace(from="Robert", to="Bob")}}`</span>
- `str | title -> String` – Capitalizes each word inside a sentence.
  e.g., <span v-pre>`{{ "foo bar" | title }}`</span> becomes `Foo Bar`.
- `str | trim -> String` – Removes leading and trailing whitespace.
- `str | trim_start -> String` – Removes leading whitespace.
- `str | trim_end -> String` – Removes trailing whitespace.
- `str | truncate -> String` – Truncates a string to the indicated length.
- `str | first -> String` – Returns the first element in an array or string.
- `str | last -> String` – Returns the last element in an array or string.
- `str | join(sep) -> String` – Joins an array of strings with a separator,
  such as <span v-pre>`{{ ["a", "b", "c"] | join(sep=", ") }}`</span>
  to produce `a, b, c`.
- `str | length -> usize` – Returns the length of a string or array.
- `str | reverse -> String` – Reverses the order of characters in a string or
  elements in an array.
- `str | urlencode -> String` – Encodes a
  string to be safely used in URLs,
  converting special characters to percent-encoded values.
- `arr | map(attribute) -> Array` – Deprecated compatibility filter. Extracts
  an attribute from each object in an array.
- `arr | concat(with) -> Array` – Deprecated compatibility filter. Appends
  values to an array. Prefer array literals and spread syntax.
- `num | abs -> Number` – Returns the absolute value of a number.
- `num | filesize_format -> String` – Converts
  an integer into
  a human-readable file size. `filesizeformat` is also available as an alias.
- `str | date(format, [timezone]) -> String` – Converts a timestamp to
  a formatted date string using the provided format,
  such as <span v-pre>`{{ ts | date(format="%Y-%m-%d") }}`</span>.
  Find a list of time formats in the
  [`jiff` documentation](https://docs.rs/jiff/latest/jiff/fmt/strtime/index.html).
- `str | b64_encode([url_safe], [padded]) -> String` – Encodes a string as base64.
- `str | b64_decode([url_safe]) -> String` – Decodes a base64 string.
- `value | format(spec) -> String` – Formats a value with Rust-style formatting.
- `value | json_encode([pretty]) -> String` – Encodes a value as JSON.
- `array | shuffle([seed]) -> Array` – Randomly shuffles an array.
- `str | regex_replace(pattern, rep) -> String` – Replaces regex matches.
- `str | striptags -> String` – Removes HTML tags.
- `str | spaceless -> String` – Removes whitespace between HTML tags.
- `str | slug -> String` – Converts a string to a URL-friendly slug.
  `slugify` is also available as an alias.
- `str | urlencode_strict -> String` – Percent-encodes all non-alphanumeric characters.
- `str | split(pat) -> Array` – Splits a string by the given pattern and
  returns an array of substrings.
- `str | default(value) -> String` – Returns the default value
  if the variable is not defined or is empty.

Tera offers more filters. Read more in the [tera documentation](https://keats.github.io/tera/#built-in-filters).

#### Hash

- `str | hash([algorithm], [len]) -> String` – Generates a hash for the input string.
  - `algorithm: "sha256" | "blake3"`: hash algorithm to use (default: `"sha256"`)
  - `len: usize`: truncates the hash string to the given size
  - Examples:
    - <span v-pre>`{{ "foo" | hash }}`</span> – SHA256 hash (default)
    - <span v-pre>`{{ "foo" | hash(algorithm="blake3") }}`</span> – BLAKE3 hash
    - <span v-pre>`{{ "foo" | hash(len=8) }}`</span> – SHA256 hash truncated to 8 characters
- `path | hash_file([len]) -> String` – Returns the BLAKE3 hash of the file
  at the given path.
  - `len: usize`: truncates the hash string to the given size

#### Path Manipulation

- `path | absolute -> String` – Converts the input path into
  an absolute path. Does not require the path to exist.
- `path | canonicalize -> String` – Converts the input path into its
  canonical absolute form. Throws if the path doesn't exist.
- `path | dirname -> String` – Returns the directory path for a file,
  e.g. `/foo/bar/baz.txt` becomes `/foo/bar`.
- `path | basename -> String` – Returns the base name of a file,
  e.g. `/foo/bar/baz.txt` becomes `baz.txt`.
- `path | extname -> String` – Returns the extension of a file,
  e.g. `/foo/bar/baz.txt` becomes `.txt`.
- `path | file_stem -> String` – Returns the file name without the extension,
  e.g. `/foo/bar/baz.txt` becomes `baz`.
- `path | file_size -> String` – Returns the size of a file in bytes.
- `path | last_modified -> String` – Returns the last modified time of a file.
- `path[] | join_path -> String` – Joins an array of paths into a single path.

For example, you can use an array literal and `join_path` to construct a file
path:

```toml
[env]
PROJECT_CONFIG = "{{ [config_root, 'bar.txt'] | join_path }}"
```

#### String Manipulation

- `str | quote -> String` – Quotes a string for a POSIX shell. Embedded single
  quotes use the POSIX-safe `'\''` form, e.g. `'it'\''s str'`. This filter does
  not adapt its output for PowerShell, cmd, or other non-POSIX shells.
- `str | kebabcase -> String` – Converts a string to kebab-case
- `str | lowercamelcase -> String` – Converts a string to lowerCamelCase
- `str | uppercamelcase -> String` – Converts a string to UpperCamelCase
- `str | snakecase -> String` – Converts a string to snake_case
- `str | shoutysnakecase -> String` – Converts a string to SHOUTY_SNAKE_CASE

Use `quote` when inserting a template value into a POSIX shell command. Quoted
and unquoted segments can be concatenated into the same argument:

```toml
[tasks.create-config]
run = "touch {{ config_root | quote }}/generated.toml"
```

### Tests

Tera offers many [built-in tests](https://keats.github.io/tera/#built-in-tests).
Some tests:

- `defined` - Returns `true` if the given variable is defined.
- `string` - Returns `true` if the given variable is a string.
- `number` - Returns `true` if the given variable is a number.
- `starting_with` - Returns `true` if the given variable is a string and starts with
  the given argument.
- `ending_with` - Returns `true` if the given variable is a string and ends with
  the given argument.
- `containing` - Returns `true` if the given variable contains the given argument.
- `matching` - Returns `true` if the given variable is a string and matches the regex
  in the argument.

Tera offers more tests. Read more in the [tera documentation](https://keats.github.io/tera/#built-in-tests).

mise offers additional tests:

- `if path is dir` – Checks whether the path is a directory.
- `if path is file` – Checks whether the path is a file.
- `if path is exists` – Checks whether the path exists.

## Template Support in .miserc.toml {#miserc-template-support}

`.miserc.toml` files support Tera templates, but with a **limited context**: `.miserc.toml`
is loaded very early — before `mise.toml`, settings, and the main config are parsed — so
only information available at the OS level can be used.

### Available context

- `env: HashMap<String, String>` – OS environment variables (same as in `mise.toml`)
- `config_root: PathBuf` – Directory containing the `.miserc.toml` file
- `cwd: PathBuf` – Current working directory
- `xdg_cache_home`, `xdg_config_home`, `xdg_data_home`, `xdg_state_home` – XDG base directories
- All [functions](#functions): `arch()`, `os()`, `os_family()`, `num_cpus()`, `choice()`, etc.
- All [filters](#filters): `absolute`, `dirname`, `basename`, `hash`, etc.

### Not available

- `mise_env` – This is what `.miserc.toml` defines; it cannot reference itself
- `exec()` – Requires settings, which are not yet loaded
- `read_file()` – Not registered in the early-init context (needs per-file directory resolution that is not set up at this stage)
- `mise_bin`, `mise_pid` – Not meaningful at this stage

### miserc.toml Examples

<div v-pre>

```toml
# /workspaces/vcs/.config/miserc.toml

# Use $HOME to set a ceiling path (stops config search at home directory)
ceiling_paths = ["{{ env.HOME }}"]

# Paths are relative to the directory containing this miserc file.
# Recursive glob patterns are supported.
ignored_config_paths = ["../vendor/**/mise.toml"]
```

</div>

Conditionals work too — `{% if %}` blocks at the top level produce empty lines when the
condition is false, which TOML ignores:

<div v-pre>

```toml
# ~/.config/mise/miserc.toml
{% if os() == "linux" %}
ceiling_paths = ["{{ env.HOME }}/work"]
{% endif %}
```

</div>

::: tip
If a template fails to render (e.g. due to an undefined variable), mise logs a warning
and falls back to the raw content.
:::

::: warning
If your `.miserc.toml` values contain literal <span v-pre>`{{`</span>, `{%`, or `{#` characters
(not intended as templates), wrap them in a `{% raw %}...{% endraw %}` block to prevent Tera
from interpreting them.
:::
