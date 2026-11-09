---
description: "Put a project's mise tools and environment variables on the PATH of a coding agent's non-interactive shell, with a tested Claude Code hook."
socialDescription: "Give coding agents a project's mise tools without wrapping every command in mise x."
---

# Coding agents

A coding agent such as Claude Code or Codex runs each command in a
non-interactive shell. `mise activate` in your `.zshrc` or `.bashrc` never runs
there, so the agent does not get the project's tools or `[env]` unless it wraps
the command in [`mise x --`](/cli/exec.html) or the shell finds
[shims](/dev-tools/shims.html) on `PATH`.

Both work, but they have costs. Wrapping every command is noisy and agents
forget to do it. Shims resolve a tool from the directory the command runs in, so
a tool that the current directory's config does not select fails with
`No version is set for shim`.

mise has no agent-specific mode. The pieces it already has are enough:
`mise hook-env` is what `mise activate` runs before each
prompt, and it works without a prompt. Running it before each command gives the
agent the same `PATH` and variables that an activated interactive shell has.

## Claude Code {#claude-code}

Claude Code sources the file named by `$CLAUDE_ENV_FILE` before each Bash
command it runs. Only a `SessionStart` hook can write to it. Add this to
`~/.claude/settings.json`, or to a project's `.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "[ -n \"$CLAUDE_ENV_FILE\" ] && echo 'eval \"$(mise hook-env -s bash)\"' >> \"$CLAUDE_ENV_FILE\"; true"
          }
        ]
      }
    ]
  }
}
```

The hook appends one line:

```sh
eval "$(mise hook-env -s bash)"
```

Use `-s zsh` if Claude Code runs commands in zsh. Because the line runs before
every command, it follows the agent as it changes directory: entering a
subproject adds that project's tools, and leaving removes them. When nothing
changed since the last command, `hook-env` prints nothing and costs a few
milliseconds.

If you prefer a fixed environment for the whole session, write the output of
[`mise env`](/cli/env.html) once instead. It is evaluated at session start, in
the directory the session started in, and does not follow `cd`:

```sh
mise env -s bash >> "$CLAUDE_ENV_FILE"
```

Either way, the agent can then run `node`, `cargo` or `python` directly, and
the project's `[env]` variables are set.

mise does not install missing tools on its own here. Run
[`mise install`](/cli/install.html) in the project first, or have the agent do
so. A config that mise does not [trust](/cli/trust.html) is not loaded.

## Codex {#codex}

Codex runs commands through your login shell by default, so the tools in your
login profile apply. Add [shims to your login profile](/dev-tools/shims.html#how-to-add-mise-shims-to-path)
for the same effect as above, and run `mise install` in the project. Check
Codex's own documentation for how it selects the shell, because mise cannot
change that. This setup has not been tested here the way the Claude Code hook
has.

## Other agents

Any agent that lets you run a command at the start of a session, or point its
shell at an rc file, can use the same line. If it offers neither, the
fallback is [shims on `PATH`](/ide-integration.html#adding-shims-to-path-default-shell)
in the login profile, with the limits described above.

## Worktrees {#worktrees}

Agents often work in a git worktree, such as `.claude/worktrees/<name>`. The
`hook-env` line needs nothing per worktree, because it reads the config of
whichever directory the command runs in. Shims are the likelier trap here: a
worktree is often where `No version is set for shim` appears, since the shim
resolves tools from the worktree's own config.

Installed tools live in mise's data directory, not in the worktree, so a new
worktree does not reinstall them.

## Related

- [AI assistants (MCP)](/mcp.html) lets an assistant read a project's tasks and
  config and run tasks through `mise mcp`.
- [Shims](/dev-tools/shims.html) explains the trade-offs between shims and
  `PATH` activation.
