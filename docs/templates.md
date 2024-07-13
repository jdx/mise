# Templates

Templates are used in the following locations:

- `.tool-versions` files
- `.mise.toml` files for most configuration
- _(Submit a ticket if you want to see it used elsewhere!)_

The following context objects are available inside templates:

- `env: HashMap<String, String>` – current environment variables
- `config_root: PathBuf` – directory containing the `.mise.toml` file

As well as these functions:

- `exec(command: &str) -> String` – execute a command and return the output

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
