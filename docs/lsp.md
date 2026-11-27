---
description: "Run the mise lsp language server in your editor for diagnostics, completion, hover, formatting and task navigation in mise config files."
---

# Language server

`mise lsp` runs a [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
server for mise config files. An editor that starts it gets diagnostics,
completion, hover documentation, quick fixes, formatting, semantic tokens and
navigation between tasks in `mise.toml`. Use it to catch a misspelled key or
task name while you edit, before you run mise.

The server reads the same schema mise validates configs against, so what it
reports follows the installed mise. It serves `mise.toml`, `.mise.toml`,
`mise.<env>.toml`, `mise.local.toml`, `mise/config.toml` and the files of a
`conf.d`. It ignores every other file, so attaching it to the TOML language
leaves `Cargo.toml` and `pyproject.toml` alone. Files that
`[task_config] includes` lists are tasks, not configs, so the server leaves them
alone too, even when they are named like `mise.tasks.toml`.

The server only parses a config. It never renders templates or runs anything,
so opening an untrusted file is safe.

## Start it from your editor {#language-server-setup}

Configure your editor to start `mise lsp` for mise config files. The command
takes no arguments and talks to the editor over stdin and stdout. For Neovim
0.11 or later:

```lua
vim.lsp.config("mise", {
  cmd = { "mise", "lsp" },
  filetypes = { "toml" },
  root_markers = { "mise.toml", ".mise.toml" },
})
vim.lsp.enable("mise")
```

In Zed, the [mise-zed](/ide-integration.html#zed) extension starts it. Other
editors with a generic LSP client, such as Helix and Emacs, take the same
command.

### Run it beside another TOML server {#language-server-options}

If another TOML server, such as Tombi or Even Better TOML, runs on the same
files, hover, completion and formatting show up twice. Turn them off in the
client's `initializationOptions` (`init_options` in Neovim) to keep only what
mise adds:

```json
{ "formatting": false, "hover": false, "completion": false }
```

Each option is a boolean that defaults to `true`:

| Option           | What `false` turns off                                |
| ---------------- | ----------------------------------------------------- |
| `formatting`     | Formatting                                            |
| `hover`          | Hover on keys, tool names and task names              |
| `completion`     | Completion of keys, values, tool names and task names |
| `definition`     | Go to definition                                      |
| `references`     | Find references                                       |
| `rename`         | Rename                                                |
| `semanticTokens` | [Semantic tokens](#language-server-semantic-tokens)   |

Diagnostics and quick fixes are always on. The server advertises a capability
to the client only while it is on.

## Features {#language-server-features}

| Feature         | What the server provides                                                                                                                                                                                               |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Diagnostics     | TOML syntax errors, unknown keys with a "did you mean" suggestion, values of the wrong type, deprecated keys, task names that no task defines, and dependency cycles. See [Diagnostics](#language-server-diagnostics). |
| Completion      | Keys, including inside inline tables, and values that have a fixed set, such as booleans and enums. Deprecated keys and keys a table already has are not offered.                                                      |
| Hover           | The type and description of a key.                                                                                                                                                                                     |
| Quick fixes     | Replace an unknown key or task name with the suggested one, and rewrite a deprecated key to its replacement.                                                                                                           |
| Formatting      | The same as [`mise fmt`](/cli/fmt.html).                                                                                                                                                                               |
| Tasks           | Completion, hover, go to definition, find references and rename for task and template names. See [Tasks](#language-server-tasks).                                                                                      |
| Tools           | Completion and hover for tool names from the [registry](/registry.html). See [Tools](#language-server-tools).                                                                                                          |
| Semantic tokens | Colors for what a key is, which a TOML grammar cannot tell. See [Semantic tokens](#language-server-semantic-tokens).                                                                                                   |

## Tasks {#language-server-tasks}

A task reference is a name in `depends`, `depends_post` or `wait_for`, or in a
`run` or `run_windows` entry written as `{ task = "..." }` or
`{ tasks = [...] }`. The server completes a reference, and hovering one shows
the task's description and command.

```toml
[tasks.build]
description = "compile"
run = "cargo build"

[tasks.test]
depends = ["bu"] # completes to "build"; hover on "build" shows the description and command
```

- Go to definition on a reference jumps to its `[tasks.<name>]`, in another
  file when that is where it is defined. A task is found by its name or by one
  of its `alias` names.
- Find references lists the places in the open file that name a task, by its
  name or by an alias.
- Rename changes the `[tasks.<name>]` key (including `[tasks.<name>.env]` and
  dotted spellings) and every reference to it in the same file. It keeps the
  arguments after a name (`"build --release"`), including ones that start with
  environment assignments (`"PROFILE=release build"`).
- `extends` names a template, `[task_templates.<name>]`, so it completes, jumps
  and renames within templates.

The server does not edit other files, so it refuses a rename when:

- Another config or include file also refers to the task, or some config or
  include file could not be read (a glob or remote include, or more than 200
  files).
- The task is defined in another file, or the name under the cursor is an
  alias.
- A task with the new name already exists.
- The rename would leave the file meaning something else, such as one that
  meets a spelling the server does not follow.

### Where tasks come from {#language-server-task-sources}

The server reads tasks from:

- The file you edit and the configs in the directories above it.
- The user's and system's configs, including the directories that
  `MISE_CONFIG_DIR` and `MISE_SYSTEM_CONFIG_DIR` name.
- The TOML files those configs name in `[task_config] includes`: a file, or a
  directory of TOML files. Globs, `git::` and `oci::` includes are not read.
- File tasks: scripts in `mise-tasks/` and the other task directories, and the
  executable files in directories that `includes` names.

A file task is known only by name: its path with or without the extension, and
the `#MISE alias=` lines in its header. That is enough for the
[`unknown-task`](#language-server-diagnostics) check, but the server does not
complete a file task or describe it on hover.

## Tools {#language-server-tools}

Under `[tools]` and a task's `tools`, a key completes to the short names in the
registry, the same list `mise registry` prints, with each tool's description.
Hovering a tool name shows its description and the backends that install it.
Aliases the registry declares, such as `rg` for `ripgrep`, are recognized; other
names that mise resolves some other way (`nodejs`, `golang`) are not. Deprecated
tools are not offered, and the tools that `enable_tools` and `disable_tools`
filter out are left out, as in `mise registry`.

<!-- schema: skip -->

```toml
[tools]
no # completes to node; hover on `node` shows its description and `Backends: core:node`
```

The list is whatever `mise lsp` read when it started, from the registry built
into mise (or the cached floating registry when `registry_floating` is on). The
server looks up nothing: it does not list or resolve versions, and does not
touch the network. A tool written with a backend (`"npm:prettier"`) is not in
the registry, so it only gets the schema's hover.

## Diagnostics {#language-server-diagnostics}

Each diagnostic carries a `code`, and a suggestion is in the diagnostic's
`data`:

| Code             | Severity | Reported for                                                               |
| ---------------- | -------- | -------------------------------------------------------------------------- |
| `syntax`         | Error    | TOML that does not parse                                                   |
| `unknown-key`    | Error    | A key the schema does not have. A warning under `[settings]`               |
| `invalid-value`  | Error    | A value of the wrong type                                                  |
| `deprecated-key` | Warning  | A deprecated key                                                           |
| `unknown-task`   | Warning  | A [task reference](#language-server-tasks) or `extends` that names no task |
| `task-cycle`     | Error    | A `depends` that is part of a dependency cycle                             |

An unknown `[settings]` key is a warning because mise still loads the file.
While a file does not parse, the diagnostics of the parts you did not edit
stay.

### Task references

`unknown-task` and `task-cycle` check task references, the way
[`mise tasks validate`](/cli/tasks/validate.html) does.

`unknown-task` marks a task reference that names no task from
[the places the server reads](#language-server-task-sources), and a template in
`extends` that no `[task_templates]` entry defines, with a "did you mean" fix.
It skips patterns with `*`, `?`, `[` or `{` (such as `lint:*`), monorepo
references (`//app:build`, `:build`) and `{ task = "x", optional = true }`.

The server reports no `unknown-task` rather than guess when tasks can come from
somewhere it does not read:

- A glob or a `git::` or `oci::` entry in `includes`.
- A `task_config.dir`.
- A `node_modules` or `target` directory below a directory that `includes`
  names.
- A `mise.<env>.toml` for an environment that was not in `MISE_ENV` when the
  server started.
- More directory entries than the server scans for file tasks (5000 in all), or
  more than 200 config and include files.

Only the first 100 unknown names in a file get a "did you mean" fix.

`task-cycle` marks each `depends` that is part of a dependency cycle among the
tasks the file defines, with the cycle in the message. A task that depends on
itself is not reported, because mise ignores that. The cycle check does a
limited amount of work for one file: when very many `depends` are on long
cycles, the first of them in the file are reported and the rest are not.

### Deprecated keys

A deprecated key gets a quick fix only when its schema description names the
replacement as a key (a description that only says `deprecated alias for run in
hooks` names none) and rewriting is safe, checked again when you apply it: the
replacement is a key the schema declares, the value fits the schema there,
nothing is already at that key, and the file still parses to the same document.
For example `env_path = ["bin"]` becomes `env._.path = ["bin"]` and
`[settings] task_output = "prefix"` becomes `task.output = "prefix"`. Anything
else is left for you to rewrite.

## Semantic tokens {#language-server-semantic-tokens}

Semantic tokens color what a key is, which a TOML grammar cannot tell:

| Element                                                                               | Token                                                              |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Task and template names, where defined and where [referenced](#language-server-tasks) | `function`, with `declaration` where defined                       |
| Tools                                                                                 | `type`                                                             |
| Env vars                                                                              | `variable`                                                         |
| Vars                                                                                  | `variable`, with `readonly`                                        |
| The `_` of env directives                                                             | `keyword`                                                          |
| The `{{ }}`, `{% %}` and `{# #}` of templates                                         | `macro`                                                            |
| Deprecated keys                                                                       | The `deprecated` modifier, on `property` when no row above applies |

The server sends them for the whole file and for a range. A file that does not
parse gets none, so the client keeps the colors it has.

## See also

- [`mise lsp`](/cli/lsp.html) is the command reference.
- [Editors and IDEs](/ide-integration.html) covers the other half of editor
  setup: making an editor's terminal, language servers and debugger find the
  tools mise manages.
- [Editor schema](/configuration.html#mise-toml-schema) gives the JSON schema
  URL for an editor that validates `mise.toml` against a schema instead.
- [mise.toml reference](/configuration/reference.html) describes the keys the
  server completes and checks.
- [`mise tasks validate`](/cli/tasks/validate.html) runs the task checks from
  the command line.
