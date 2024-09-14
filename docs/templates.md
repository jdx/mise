# Templates

Templates are used in the following locations:

- `.tool-versions` files
- `.mise.toml` files for most configuration
- _(Submit a ticket if you want to see it used elsewhere!)_

The following context objects are available inside templates:

- `env: HashMap<String, String>` – current environment variables
- `cwd: PathBuf` – current working directory
- `config_root: PathBuf` – directory containing the `mise.toml` file or directory containing
  `.mise` directory with config file.

As well as these functions:

- `exec(command) -> String` – execute a command and return the output
- `arch() -> String` – return the system architecture, e.g. `x86_64`, `arm64`
- `os() -> String` – return the operating system, e.g. `linux`, `macos`, `windows`
- `os_family() -> String` – return the operating system family, e.g. `unix`, `windows`
- `num_cpus() -> usize` – return the number of CPUs on the system
- `uuid() -> String` – return a random UUIDv4

And these filters:

- `str | hash -> String` – return the SHA256 hash of the input string
- `str | hash(len=usize) -> String` – return the SHA256 hash of the input string truncated to `len`
  characters
- `str | hash(function="blake3") -> String` – return the BLAKE3 hash of the input string
- `path | hash_file -> String` – return the SHA256 hash of the file at the input path
- `path | hash_file(len=usize) -> String` – return the SHA256 hash of the file at the input path
  truncated to `len` characters
- `path | hash_file(function="blake3") -> String` – return the BLAKE3 hash of the file at the input path
- `path | canonicalize -> String` – return the canonicalized path
- `path | dirname -> String` – return the directory path for a file, e.g. `/foo/bar/baz.txt` ->
  `/foo/bar`
- `path | basename -> String` – return the base name of a file, e.g. `/foo/bar/baz.txt` -> `baz.txt`
- `path | extname -> String` – return the extension of a file, e.g. `/foo/bar/baz.txt` -> `.txt`
- `path | file_stem -> String` – return the file name without the extension, e.g.
  `/foo/bar/baz.txt` -> `baz`
- `path | file_size -> String` – return the size of a file in bytes
- `path | last_modified -> String` – return the last modified time of a file
- `path[] | join_path -> String` – join an array of paths into a single path
- `str | quote -> String` – quote a string
- `str | kebabcase -> String` – convert a string to kebab-case
- `str | lowercamelcase -> String` – convert a string to lowerCamelCase
- `str | uppercamelcase -> String` – convert a string to UpperCamelCase
- `str | shoutycamelcase -> String` – convert a string to ShoutyCamelCase
- `str | snakecase -> String` – convert a string to snake_case
- `str | shoutysnakecase -> String` – convert a string to SHOUTY_SNAKE_CASE

And these testers:

- `if path is dir` – if the path is a directory
- `if path is file` – if the path is a file
- `if path is exists` – if the path exists

Templates are parsed with [tera](https://keats.github.io/tera/docs/)—which is quite powerful. For
example, this snippet will get the directory name of the project:

```toml
[env]
PROJECT_NAME = "{{config_root | split(pat='/') | last}}"
```

Here's another using `exec()`:

```toml
[alias.node]
current = "{{exec(command='node --version')}}"
```

Or one that uses [`get_env()`](https://keats.github.io/tera/docs/#get-env):

```toml
[plugins]
my-plugin = "https://{{ get_env(name='GIT_USR', default='empty') }}:{{ get_env(name='GIT_PWD', default='empty') }}@github.com/foo/my-plugin.git"
```
