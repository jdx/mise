# Model Context Protocol (MCP)

The mise MCP server lets an AI assistant inspect a project's tools, tasks, environment, and
configuration, and run mise tasks. It uses the [Model Context Protocol](https://modelcontextprotocol.io/docs/getting-started/intro)
over stdin/stdout. The client launches a local `mise mcp` process; no HTTP server or listening
port is required.

::: warning Experimental
The server requires `MISE_EXPERIMENTAL=1`. Its resources and tools may change.
:::

## Usage

Select the project directory when starting the server. Otherwise it uses the client's working
directory, which may be your home directory or an unrelated workspace:

```sh
MISE_EXPERIMENTAL=1 mise --cd /absolute/path/to/project mcp
```

Replace the path with your project. This command waits for MCP input; it does not open an
interactive prompt. Usually you configure an MCP client to start it rather than running it
in a terminal yourself.

## Integration with AI Assistants

Configure your client with the mise executable, the project directory, and the experimental
environment variable. For clients that use a `mcpServers` JSON configuration:

```json
{
  "mcpServers": {
    "mise": {
      "command": "/absolute/path/to/mise",
      "args": ["--cd", "/absolute/path/to/project", "mcp"],
      "env": {
        "MISE_EXPERIMENTAL": "1"
      }
    }
  }
}
```

Replace both paths. On Windows, use the path to `mise.exe` and escape backslashes in JSON.
An absolute executable path is useful for GUI clients that do not inherit your shell's `PATH`.
The configuration file location and key names depend on the client; use its MCP setup guide.
Restart or reconnect the server after changing its configuration or switching projects.

### Access and execution

Connect the server only to a project and client you trust. Reading `mise://env` evaluates the
project's environment configuration and returns its values, which can include secrets.
Resource reads can also evaluate configuration templates and environment directives; a
read-only query is not a sandbox for untrusted project configuration.

`run_task` executes the project's commands with your account's access. It runs without
interactive stdin and sets `MISE_YES=1`, so use your client's tool approval controls to decide
which tasks may run. Review task definitions before allowing an assistant to execute them.
See [security](/security.html) for mise's configuration trust model.

## Available Resources

Resources return JSON text. They describe the project selected when the server starts.
Restart the server after editing configuration if the client continues to show cached results.

| URI                                  | Contents                                                                                        |
| ------------------------------------ | ----------------------------------------------------------------------------------------------- |
| `mise://tools`                       | Active tool versions, requested versions, installation paths/status, and configuration sources. |
| `mise://tools?include_inactive=true` | Active tools plus other installed versions.                                                     |
| `mise://tasks`                       | Task definitions, commands, descriptions, dependencies, source files, and execution options.    |
| `mise://env`                         | Resolved mise environment variable names and values.                                            |
| `mise://config`                      | Active configuration file paths and the project root.                                           |

`mise://config` does not return a full settings dump. In `mise://tasks`, the `env` field is
currently an empty object; it does not expose task-specific environment values. Use the
source configuration when you need details that a resource does not provide.

## Available Tools

### `list_commands`

Lists mise commands with their help text and declared effect: `read`, `write`, or `destructive`.
An absent effect means the command is unclassified. These declarations describe commands;
they do not execute them or enforce client approval policy.

The optional boolean `include_hidden` defaults to `false`. For example:

```json
{
  "include_hidden": false
}
```

### `run_task`

Runs a task, including its normal mise dependencies and environment:

| Parameter | Type             | Required | Meaning                                                           |
| --------- | ---------------- | -------- | ----------------------------------------------------------------- |
| `task`    | string           | Yes      | Task name, such as `build`.                                       |
| `args`    | array of strings | No       | Arguments passed after the task name. Defaults to an empty array. |

For a task that accepts a `--verbose` flag, pass:

```json
{
  "task": "build",
  "args": ["--verbose"]
}
```

These are tool arguments, not a complete JSON-RPC request. `--verbose` is passed to the task;
it does not enable mise's own verbose logging.

The response contains captured stdout and stderr after the task finishes. A nonzero exit
status produces a tool error with the exit code and output. Output is not streamed, and tasks
that require terminal input cannot prompt through this tool. The
[`task.timeout`](/configuration/settings.html#task.timeout) setting limits execution time.

### `install_tool`

The server advertises `install_tool`, but calling it currently returns a “not yet implemented”
error. Install required tools with `mise install` outside this MCP tool before running tasks,
or use a client's separate command execution facility if it provides one.

## Examples

Once connected to the intended project, ask the assistant to:

- Show the active Node.js version and whether it is installed.
- List available tasks and inspect the dependencies of `build`.
- Run a named task you have reviewed.
- Show which configuration files are active.

If the tool list or project root is unexpected, check the server's `--cd` argument. If it
cannot start, verify the absolute mise path and `MISE_EXPERIMENTAL=1` in the client logs.

## Technical Details

The implementation is in [`src/cli/mcp.rs`](https://github.com/jdx/mise/blob/main/src/cli/mcp.rs).
It uses the `rmcp` crate for MCP resource listing, resource reads, and tool calls over stdio.
Clients need to support MCP; raw JSON-RPC support alone does not establish an MCP session.
