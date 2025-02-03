# `mise generate task-stubs`

- **Usage**: `mise generate task-stubs [-m --mise-bin <MISE_BIN>] [-d --dir <DIR>]`
- **Source code**: [`src/cli/generate/task_stubs.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/task_stubs.rs)

[experimental] Generates shims to run mise tasks

By default, this will build shims like ./bin/&lt;task>. These can be paired with `mise generate bootstrap`
so contributors to a project can execute mise tasks without installing mise into their system.

## Flags

### `-m --mise-bin <MISE_BIN>`

Path to a mise bin to use when running the task stub.

Use `--mise-bin=./bin/mise` to use a mise bin generated from `mise generate bootstrap`

### `-d --dir <DIR>`

Directory to create task stubs inside of

Examples:

```
$ mise task add test -- echo 'running tests'
$ mise generate task-stubs
$ ./bin/test
running tests
```
