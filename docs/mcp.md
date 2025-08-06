# Model Context Protocol (MCP)

The Model Context Protocol (MCP) is a standard protocol that enables AI assistants to interact with development tools and access project context. Mise provides an MCP server that allows AI assistants to query information about your development environment.

## Overview

When you run `mise mcp`, it starts a server that AI assistants can connect to and query information about your mise-managed development environment. The server communicates over stdin/stdout using JSON-RPC protocol.

::: warning
The MCP feature is experimental and requires enabling experimental features with `MISE_EXPERIMENTAL=1`.
:::

## Usage

The MCP server is typically launched by AI assistants automatically, but you can also run it manually for testing:

```bash
# Enable experimental features
export MISE_EXPERIMENTAL=1

# Start the MCP server (it will wait for JSON-RPC input on stdin)
mise mcp
```

## Available Resources

The MCP server exposes the following read-only resources that AI assistants can query:

### `mise://tools`
Lists all tools managed by mise in your project, including:
- Tool names and versions
- Installation status
- Configuration source

### `mise://tasks`
Shows all available mise tasks with:
- Task names and descriptions
- Task dependencies
- Command definitions

### `mise://env`
Displays environment variables defined in your mise configuration:
- Variable names and values
- Environment-specific overrides

### `mise://config`
Provides information about mise configuration:
- Active configuration files
- Project root directory
- Settings and preferences

## Available Tools

The following tools are available for AI assistants to call (currently stubbed for future implementation):

### `install_tool`
Install a specific tool version (not yet implemented)

### `run_task`
Execute a mise task (not yet implemented)

## Integration with AI Assistants

### Claude Desktop

To use mise with Claude Desktop, add the following to your Claude configuration file:

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`  
**Windows**: `%APPDATA%\Claude\claude_desktop_config.json`  
**Linux**: `~/.config/claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "mise": {
      "command": "mise",
      "args": ["mcp"],
      "env": {
        "MISE_EXPERIMENTAL": "1"
      }
    }
  }
}
```

After adding this configuration and restarting Claude Desktop, the assistant will be able to:
- Query your installed tools and versions
- List available tasks in your project
- Access environment variables from your mise configuration
- View your mise configuration structure

### Other AI Assistants

The MCP server uses standard JSON-RPC 2.0 over stdio, making it compatible with any AI assistant that supports the Model Context Protocol. Consult your AI assistant's documentation for specific integration instructions.

## Examples

When integrated with an AI assistant, you can ask questions like:

- "What version of Node.js is this project using?"
- "List all the tasks available in this project"
- "What environment variables are set by mise?"
- "Show me the mise configuration for this project"

The AI assistant will query the MCP server to provide accurate, up-to-date information about your development environment.

## Technical Details

The MCP server implementation can be found in [`src/cli/mcp.rs`](https://github.com/jdx/mise/blob/main/src/cli/mcp.rs). It implements the ServerHandler trait from the rmcp crate to handle:

- Resource listing and reading
- Tool invocation (planned)
- JSON-RPC communication over stdio

For more information about the Model Context Protocol, visit the [official MCP documentation](https://modelcontextprotocol.io/).
