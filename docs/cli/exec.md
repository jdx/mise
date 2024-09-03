## `mise exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]`

**Aliases:** `x`

```text
Execute a command with tool(s) set

use this to avoid modifying the shell session or running ad-hoc commands with mise tools set.

Tools will be loaded from .mise.toml/.tool-versions, though they can be overridden with <RUNTIME> args
Note that only the plugin specified will be overridden, so if a `.tool-versions` file
includes "node 20" but you run `mise exec python@3.11`; it will still load node@20.

The "--" separates runtimes from the commands to pass along to the subprocess.

Usage: exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]

Arguments:
  [TOOL@VERSION]...
          Tool(s) to start e.g.: node@20 python@3.10

  [COMMAND]...
          Command string to execute (same as --command)

Options:
  -c, --command <C>
          Command string to execute

  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]
          
          [env: MISE_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

Examples:

    $ mise exec node@20 -- node ./app.js  # launch app.js using node-20.x
    $ mise x node@20 -- node ./app.js     # shorter alias

    # Specify command as a string:
    $ mise exec node@20 python@3.11 --command "node -v && python -V"

    # Run a command in a different directory:
    $ mise x -C /path/to/project node@20 -- node ./app.js
```
