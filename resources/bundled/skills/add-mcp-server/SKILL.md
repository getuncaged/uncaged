---
name: agent-add-mcp
description: Use this skill when helping users add MCP servers to their Uncaged configuration.
---

# Adding MCP Servers to Uncaged

Uncaged supports MCP servers via native config files. Follow these steps when helping a user add an MCP server.

## Step 1: Determine Scope

If the user hasn't specified, ask whether they want to configure the server **globally** (for all projects) or **project-scoped** (for a specific repository only).

Config file paths:
- **Global (user-scoped):** `{{mcp_config_file_path}}`
- **Project-scoped:** `{repo_root}/.warp/.mcp.json` (the project directory keeps the `.warp` name)

Use the global path verbatim — it depends on the platform and the running build.

## Step 2: Gather Server Details

If the user hasn't provided the server's connection details, use WebSearch to find the correct configuration for the named server.

If it's unclear whether the server should be run as a local CLI process (stdio transport) or connected to via URL (HTTP/SSE streaming transport), ask the user which they prefer.

## Step 3: Check and Prepare the Config File

Check whether the target config file exists.

- **If it does not exist**, create it with `mkdir -p` for the directory and initialize it with an empty `mcpServers` wrapper key:
  ```json
  {
    "mcpServers": {}
  }
  ```

- **If it exists**, read it to determine which top-level wrapper key is already in use. Recognized wrapper keys (in order of preference):
  - `mcpServers` *(preferred)*
  - `mcp_servers`
  - `servers`
  - `mcp.servers` (nested under a `mcp` key)

  Servers must sit under one of these keys: a config file whose top-level keys are server names is not read. Preserve the existing wrapper key when writing. If the existing key is unrecognized, switch to `mcpServers`.

  **Never remove existing server entries** — only add or update the new server.

## Step 4: Write the Server Configuration

### Command-based server (stdio transport)

```json
{
  "mcpServers": {
    "server-name": {
      "command": "npx",
      "args": ["-y", "@scope/package-name"],
      "env": {
        "API_KEY": "${API_KEY}"
      }
    }
  }
}
```

By default, Uncaged spawns stdio servers from the directory the config was discovered in:
- Project-scoped configs (`{repo_root}/.warp/.mcp.json`) run from the repo root.
- Global configs (`{{mcp_config_file_path}}`, `~/.claude.json`, etc.) run from the home directory.

If the server's `command` or `args` are relative paths (e.g. `./tooling/mcp/server.js`) or the server expects a specific cwd, set `working_directory` to override the default:

```json
{
  "mcpServers": {
    "server-name": {
      "command": "node",
      "args": ["./tooling/mcp/server.js"],
      "working_directory": "/absolute/path/to/repo"
    }
  }
}
```

### URL-based server (HTTP/SSE streaming transport)

URL servers take `url` and optional `headers`; they have no `env`:

```json
{
  "mcpServers": {
    "server-name": {
      "url": "https://example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${API_KEY}"
      }
    }
  }
}
```

For secrets, use `${VAR_NAME}` syntax — Uncaged substitutes the value from the user's environment when it starts the server. If a referenced variable is missing or empty, the server does not start.

## Notes

- Uncaged auto-detects changes to these config files on save — no restart required.
- Configured servers appear in Uncaged's Settings → MCP servers, under **"Detected from Uncaged"**.
- Servers in the global config start automatically. Project-scoped servers never start on their own: the user enables each one in that section of the MCP servers settings, and it only applies when working inside that repository.
