---
description: "Connect an AI assistant to a project's mise tools, tasks, environment, and config through the mise MCP server."
socialDescription: "Connect an AI assistant to a project's mise tools, tasks, and environment."
---

# AI assistants (MCP)

[`mise mcp`](/cli/mcp.html) runs a
[Model Context Protocol](https://modelcontextprotocol.io/docs/getting-started/intro)
server that lets an AI assistant read a project's tools, tasks, environment
variables and config files, and run its tasks. The client starts `mise mcp` as
a local process and talks to it over stdin and stdout; nothing listens on a
port.

## Connect a client {#integration-with-ai-assistants}

Configure the client to start mise with the project directory. Without `--cd`,
the server uses the client's working directory, which may be your home directory
or another workspace. For clients that use an `mcpServers` JSON configuration:

```json
{
  "mcpServers": {
    "mise": {
      "command": "/absolute/path/to/mise",
      "args": ["--cd", "/absolute/path/to/project", "mcp"]
    }
  }
}
```

Use the absolute path to mise, because GUI clients often do not inherit your
shell's `PATH`. Find it with `command -v mise`, or `(Get-Command mise).Source`
in PowerShell. On Windows, use the path to `mise.exe` and escape backslashes in
JSON.

The file location and key names depend on the client. VS Code, for example,
uses a `servers` key in `.vscode/mcp.json`. Clients with a command-line setup
take the same command and arguments, such as Claude Code:

```sh
claude mcp add mise -- mise --cd /absolute/path/to/project mcp
```

Restart or reconnect the server after changing its configuration or switching
projects.

## Security {#access-and-execution}

Connect the server only to a project and a client you trust.

- Reading `mise://env` evaluates the project's environment config and returns
  the values, which can include secrets. Resource reads can also evaluate
  templates and environment directives, so a read is not a sandbox for config
  you have not reviewed.
- Resource reads follow mise's
  [trust rules](/security.html#configuration-trust), and the server cannot show
  the trust prompt. If the project's config needs trust, for example because
  it has `[env]`, templates or tool options, reads fail with
  `Failed to load config: error parsing config file: <path>` until you run
  `mise trust` in the project.
- `run_task` runs the project's commands with your account's access, and
  `install_tool` installs tools. Unlike `mise run` and `mise install` in a
  terminal, neither trusts the project for you: both refuse a project whose
  config files are not trusted, and list the `mise trust` commands to run after
  you review the files. The `mise` they start trusts nothing on its own either,
  so config it reaches in a subdirectory must be trusted too.
- `run_task` runs without stdin and sets `MISE_YES=1`, which answers mise's
  other confirmation prompts. Use your client's tool approval settings to decide
  which tasks may run, and review task definitions before allowing them.

To let an assistant inspect a project you have not trusted, add
`"env": { "MISE_SAFE": "1" }` to the server entry. In
[safe mode](/security.html#safe-mode), `mise://env` returns no project values
and `run_task` fails, while the other resources still list the project's tools,
tasks and config files.

## Resources {#available-resources}

Resources return JSON text. The server loads the project's configuration once
and keeps it until it exits, so after you edit `mise.toml` or task files,
restart the server from your client to refresh the resources. `run_task` starts
a new `mise run` each time and always uses the current files.

| URI                                  | Contents                                                                                                                                                                                                                                       |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mise://tools`                       | Active tool versions, requested versions, install paths, whether each is installed, and the config file that requested it.                                                                                                                     |
| `mise://tools?include_inactive=true` | Active tools plus other installed versions.                                                                                                                                                                                                    |
| `mise://tasks`                       | Task definitions, including monorepo subproject tasks: commands, descriptions, dependencies, source files and options. The `env` field lists the task's environment directives as written, such as `"NODE_ENV=test"`, without evaluating them. |
| `mise://env`                         | Resolved environment variable names and values.                                                                                                                                                                                                |
| `mise://config`                      | Active config file paths and the project root. It does not include settings.                                                                                                                                                                   |

Reading `mise://tools` resolves each version request, which can fetch version
lists over the network when no installed version matches. A trimmed example:

```json
{
  "node": [
    {
      "version": "24.21.0",
      "requested_version": "24",
      "install_path": "/home/me/.local/share/mise/installs/node/24.21.0",
      "installed": true,
      "active": true,
      "source": { "type": "mise.toml", "path": "/home/me/src/app/mise.toml" }
    }
  ]
}
```

## Tools {#available-tools}

| Tool            | Arguments                                   | What it does                                                                                                                                                                                         |
| --------------- | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `list_commands` | `include_hidden` (boolean, default `false`) | Lists mise commands with their help and declared effect: `read`, `write` or `destructive`. A command with no effect is unclassified, not safe. It runs nothing.                                      |
| `run_task`      | `task` (string), `args` (array of strings)  | Runs a task with its dependencies and environment, and returns its output when it finishes.                                                                                                          |
| `install_tool`  | `tool` (string), `version` (string)         | Installs a tool, as `mise install` does. `version` defaults to the configured version, or latest. Returns JSON with `tool`, the installed `version`, `install_path` and the install log as `output`. |

`list_commands` returns entries like:

```json
{
  "bin": "mise",
  "commands": [
    {
      "command": "tasks ls",
      "help": "List available tasks",
      "effect": "read",
      "hidden": false
    }
  ]
}
```

An assistant calls `run_task` with arguments like:

```json
{
  "task": "build",
  "args": ["--verbose"]
}
```

`--verbose` here goes to the task, not to mise. The result holds the task's
stdout and stderr after it finishes; output is not streamed, and a task cannot
read terminal input. A nonzero exit returns a tool error with the exit code and
output. The [`task.timeout`](/configuration/settings.html#task.timeout) setting
limits how long a task can run.

## Example prompts {#examples}

Once connected to the intended project, ask the assistant to:

- Show the active Node.js version and whether it is installed.
- List available tasks and the dependencies of `build`.
- Run a named task you have reviewed.
- Show which config files are active.

## Troubleshooting {#troubleshooting}

- Unexpected tasks or tools: check the path after `--cd` in the server entry.
- `Failed to load config`: run `mise trust` in the project, or `mise doctor` to
  find the problem.
- The server does not start: check the absolute mise path in the client's MCP
  log. Run `mise --cd /absolute/path/to/project mcp` in a terminal; it prints
  `Starting mise MCP server...` to stderr and waits for input.
- Stale resources after editing config: restart the server from the client.
