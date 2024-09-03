## `mise watch [OPTIONS] [ARGS]...` <Badge type="warning" text="experimental" />

**Aliases:** `w`

```text
[experimental] Run a tasks watching for changes

Usage: watch [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...
          Extra arguments

Options:
  -t, --task <TASK>
          Tasks to run
          
          [default: default]

  -g, --glob <GLOB>
          Files to watch
          Defaults to sources from the tasks(s)

Examples:
    $ mise watch -t build
    Runs the "build" tasks. Will re-run the tasks when any of its sources change.
    Uses "sources" from the tasks definition to determine which files to watch.

    $ mise watch -t build --glob src/**/*.rs
    Runs the "build" tasks but specify the files to watch with a glob pattern.
    This overrides the "sources" from the tasks definition.

    $ mise run -t build --clear
    Extra arguments are passed to watchexec. See `watchexec --help` for details.
```
