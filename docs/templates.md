# Templates

Templates in mise provide a powerful way to configure different aspects of
your environment and project settings.
You can define and use templates in the following locations:

- Most `mise.toml` configuration values
  - The `mise.toml` file itself is not templated and must be valid toml
- `.tool-versions` files
- _(Submit a ticket if you want to see it used elsewhere!)_

## Template Rendering

Mise uses [tera](https://keats.github.io/tera/docs/) to provide the template feature.
In the template, there are 3 kinds of delimiters:

- <span v-pre>`{{`</span> and <span v-pre>`}}`</span> for expressions
- <span v-pre>`{%`</span> and <span v-pre>`%}`</span> for statements
- <span v-pre>`{#`</span> and <span v-pre>`#}`</span> for comments

Additionally, use `raw` block to skip rendering tera delimiters:

<div v-pre>

```
{% raw %}
  Hello {{ name }}
{% endraw %}
```

</div>

This will become <span v-pre>`Hello {{name}}`</span>.

Tera supports [literals](https://keats.github.io/tera/docs/#literals), including:

- booleans: `true` (or `True`) and `false` (or `False`)
- integers
- floats
- strings: text delimited by `""`, `''` or <code>\`\`</code>
- arrays: a comma-separated list of literals and/or ident surrounded by
  `[` and `]` (trailing comma allowed)

You can render a variable by using the <span v-pre>`{{ name }}`</span>.
For complex attributes, use:

- dot `.`, e.g. <span v-pre>`{{ product.name }}`</span>
- square brackets `[]`, e.g. <span v-pre>`{{ product["name"] }}`</span>

Tera also supports powerful [expressions](https://keats.github.io/tera/docs/#expressions):

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
- concatenation `~`, e.g. <code v-pre>{{ "hello " ~ 'world' ~ \`!\` }</code>
- in checking, e.g. <span v-pre>`{{ some_var in [1, 2, 3] }}`</span>

Tera also supports control structures such as <span v-pre>`if`</span> and
<span v-pre>`for`</span>. Read more [here](https://keats.github.io/tera/docs/#control-structures).

### Tera Filters

You can modify variables using [filters](https://keats.github.io/tera/docs/#filters).
You can filter a variable by a pipe symbol (`|`) and may have named arguments
in parentheses. You can also chain multiple filters.
e.g. <span v-pre>`{{ "Doctor Who" | lower | replace(from="doctor", to="Dr.") }}`</span>
will output `Dr. who`.

### Tera Functions

[Functions](https://keats.github.io/tera/docs/#functions) provide
additional features to templates.

### Tera Tests

You can also uses [tests](https://keats.github.io/tera/docs/#tests) to examine variables.

```
{% if my_number is not odd %}
  Even
{% endif %}
```

## Mise Template Features

Mise provides additional variables, functions, filters and tests on top of tera features.

### Variables

Mise exposes several [variables](https://keats.github.io/tera/docs/#variables).
These variables offer key information about the current environment:

- `env: HashMap<String, String>` – Accesses current environment variables as
  a key-value map.
- `cwd: PathBuf` – Points to the current working directory.
- `config_root: PathBuf` – Locates the directory containing your `mise.toml` file, or in the case of something like `~/src/myproj/.config/mise.toml`, it will point to `~/src/myproj`.
- `mise_bin: String` - Points to the path to the current mise executable
- `mise_pid: String` - Points to the pid of the current mise process
- `xdg_cache_home: PathBuf` - Points to the directory of XDG cache home
- `xdg_config_home: PathBuf` - Points to the directory of XDG config home
- `xdg_data_home: PathBuf` - Points to the directory of XDG data home
- `xdg_state_home: PathBuf` - Points to the directory of XDG state home

### Functions

Tera offers many [built-in functions](https://keats.github.io/tera/docs/#built-in-functions).
`[]` indicates an optional function argument.
Some functions:

- `range(end, [start], [step_by])` - Returns an array of integers created
  using the arguments given.
  - `end: usize`: stop before `end`, mandatory
  - `start: usize`: where to start from, defaults to `0`
  - `step_by: usize`: with what number do we increment, defaults to `1`
- `now([timestamp], [utc])` - Returns the local datetime as string or
  the timestamp as integer.
  - `timestamp: bool`: whether to return the timestamp instead of the datetime
  - `utc: bool`: whether to return the UTC datetime instead of
    the local one
  - Tip: use date filter to format date string.
    e.g. <span v-pre>`{{ now() | date(format="%Y") }}`</span> gets the current year.
- `throw(message)` - Throws with the message.
- `get_random(end, [start])` - Returns a random integer in a range.
  - `end: usize`: upper end of the range
  - `start: usize`: defaults to 0
- `get_env(name, [default])`: Returns the environment variable value by name.
  Prefer `env` variable than this function.
  - `name: String`: the name of the environment variable
  - `default: String`: a default value in case the environment variable is not found.
    Throws when can't find the environment variable and `default` is not set.

Tera offers more functions. Read more on [tera documentation](https://keats.github.io/tera/docs/#functions).

Mise offers additional functions:

- `exec(command) -> String` – Runs a shell command and returns its output as a string.
- `arch() -> String` – Retrieves the system architecture, such as `x86_64` or `arm64`.
- `os() -> String` – Returns the name of the operating system,
  e.g. linux, macos, windows.
- `os_family() -> String` – Returns the operating system family, e.g. `unix`, `windows`.
- `num_cpus() -> usize` – Gets the number of CPUs available on the system.
- `choice(n, alphabet)` - Generate a string of `n` with random sample with replacement
  of `alphabet`. For example, `choice(64, HEX)` will generate a random
  64-character lowercase hex string.

An example of function using `exec`:

```toml
[alias.node.versions]
current = "{{ exec(command='node --version') }}"
```

### Filters

Tera offers many [built-in filters](https://keats.github.io/tera/docs/#built-in-filters).
`[]` indicates an optional filter argument.
Some filters:

- `str | lower -> String` – Converts a string to lowercase.
- `str | upper -> String` – Converts a string to uppercase.
- `str | capitalize -> String` – Converts a string with all its characters lowercased
  apart from the first char which is uppercased.
- `str | replace(from, to) -> String` – Replaces a string with all instances of
  `from` to `to`. e.g., <span v-pre>`{{ name | replace(from="Robert", to="Bob")}}`</span>
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
- `str | urlencode -> String` – Encodes a string to be safely used in URLs,
  converting special characters to percent-encoded values.
- `str | map(attribute) -> Array` – Extracts an attribute from each object
  in an array.
- `str | concat(with) -> Array` – Appends values to an array.
- `str | abs -> Number` – Returns the absolute value of a number.
- `str | filesizeformat -> String` – Converts an integer into
  a human-readable file size (e.g., 110 MB).
- `str | date(format) -> String` – Converts a timestamp to
  a formatted date string using the provided format,
  such as <span v-pre>`{{ ts | date(format="%Y-%m-%d") }}`</span>.
  Find a list of time format on [`chrono` documentation][chrono-doc].
- `str | split(pat) -> Array` – Splits a string by the given pattern and
  returns an array of substrings.
- `str | default(value) -> String` – Returns the default value
  if the variable is not defined or is empty.

Tera offers more filters. Read more on [tera documentation](https://keats.github.io/tera/docs/#built-in-filters).

#### Hash

- `str | hash([len]) -> String` – Generates a SHA256 hash for the input string.
  - `len: usize`: truncates the hash string to the given size
- `path | hash_file([len]) -> String` – Returns the SHA256 hash of the file
  at the given path.
  - `len: usize`: truncates the hash string to the given size

#### Path Manipulation

- `path | canonicalize -> String` – Converts the input path into
  absolute input path version. Throws if path doesn't exist.
- `path | basename -> String` – Extracts the file name from a path,
  e.g. `/foo/bar/baz.txt` becomes `baz.txt`.
- `path | file_size -> String` – Returns the size of a file in bytes.
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

For example, you can use `split()`, `concat()`, and `join_path` filters to
construct a file path:

```toml
[env]
PROJECT_CONFIG = "{{ config_root | concat(with='bar.txt') | join_path }}"
```

#### String Manipulation

- `str | quote -> String` – Quotes a string. Converts `'` to `\'` and
  then quotes str, e.g `'it\'s str'`.
- `str | kebabcase -> String` – Converts a string to kebab-case
- `str | lowercamelcase -> String` – Converts a string to lowerCamelCase
- `str | uppercamelcase -> String` – Converts a string to UpperCamelCase
- `str | snakecase -> String` – Converts a string to snake_case
- `str | shoutysnakecase -> String` – Converts a string to SHOUTY_SNAKE_CASE

### Tests

Tera offers many [built-in tests](https://keats.github.io/tera/docs/#built-in-tests).
Some tests:

- `defined` - Returns `true` if the given variable is defined.
- `string` - Returns `true` if the given variable is a string.
- `number` - Returns `true` if the given variable is a number.
- `starting_with` - Returns `true` if the given variable is a string and starts with
  the arg given.
- `ending_with` - Returns `true` if the given variable is a string and ends with
  the arg given.
- `containing` - Returns `true` if the given variable contains the arg given.
- `matching` - Returns `true` if the given variable is a string and matches the regex
  in the argument.

Tera offers more tests. Read more on [tera documentation](https://keats.github.io/tera/docs/#built-in-tests).

Mise offers additional tests:

- `if path is dir` – Checks if the provided path is a directory.
- `if path is file` – Checks if the path points to a file.
- `if path is exists` – Checks if the path exists.

### Example Templates

#### A Node.js Project

```toml
min_version = "2024.9.5"

[env]
# Use the project name derived from the current directory
PROJECT_NAME = "{{ cwd | basename }}"

# Set up the path for node module binaries
BIN_PATH = "{{ cwd }}/node_modules/.bin"

NODE_ENV = "{{ env.NODE_ENV | default(value='development') }}"

[tools]
# Install Node.js using the specified version
node = "{{ env['NODE_VERSION'] | default(value='lts') }}"

# Install npm packages globally if needed
"npm:typescript" = "latest"
"npm:eslint" = "latest"
"npm:jest" = "latest"

# Install npm dependencies
[tasks.install]
alias = "i"
run = "npm install"

# Run the development server
[tasks.start]
alias = "s"
run = "npm run start"

# Run linting
[tasks.lint]
alias = "l"
run = "eslint src/"

# Run tests
[tasks.test]
alias = "t"
run = "jest"

# Build the project
[tasks.build]
alias = "b"
run = "npm run build"

# Print project info
[tasks.info]
run = '''
echo "Project: $PROJECT_NAME"
echo "NODE_ENV: $NODE_ENV"
'''
```

#### A Python Project with virtualenv

```toml
min_version = "2024.9.5"

[env]
# Use the project name derived from the current directory
PROJECT_NAME = "{{ cwd | basename }}"

# Automatic virtualenv activation
_.python.venv = { path = ".venv", create = true }

[tools]
# Install the specified Python version
python = "{{ get_env(name='PYTHON_VERSION', default='3.11') }}"

# Install dependencies
[tasks.install]
alias = "i"
run = "pip install -r requirements.txt"

# Run the application
[tasks.run]
run = "python app.py"

# Run tests
[tasks.test]
run = "pytest tests/"

# Lint the code
[tasks.lint]
run = "ruff src/"

# Print environment information
[tasks.info]
run = '''
echo "Project: $PROJECT_NAME"
echo "Virtual Environment: $VIRTUAL_ENV"
'''
```

#### A C++ Project with CMake

```toml
min_version = "2024.9.5"

[env]
# Project information
PROJECT_NAME = "{{ cwd | basename }}"

# Build directory
BUILD_DIR = "{{ cwd }}/build"

[tools]
# Install CMake and make
cmake = "latest"
make = "latest"

# Configure the project
[tasks.configure]
run = "mkdir -p $BUILD_DIR && cd $BUILD_DIR && cmake .."

# Build the project
[tasks.build]
alias = "b"
run = "cd $BUILD_DIR && make"

# Clean the build directory
[tasks.clean]
alias = "c"
run = "rm -rf $BUILD_DIR"

# Run the application
[tasks.run]
alias = "r"
run = "$BUILD_DIR/bin/$PROJECT_NAME"

# Print project info
[tasks.info]
run = '''
echo "Project: $PROJECT_NAME"
echo "Build Directory: $BUILD_DIR"
'''
```

#### A Ruby on Rails Project

```toml
min_version = "2024.9.5"

[env]
# Project information
PROJECT_NAME = "{{ cwd | basename }}"

[tools]
# Install Ruby with the specified version
ruby = "{{ get_env(name='RUBY_VERSION', default='3.3.3') }}"

# Install gem dependencies
[tasks."bundle:install"]
run = "bundle install"

# Run the Rails server
[tasks.server]
alias = "s"
run = "rails server"

# Run tests
[tasks.test]
alias = "t"
run = "rails test"

# Lint the code
[tasks.lint]
alias = "l"
run = "rubocop"
```

[chrono-doc]: https://docs.rs/chrono/0.4.38/chrono/format/strftime/index.html
