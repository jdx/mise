# `mise generate git-pre-commit`

- **Usage**: `mise generate git-pre-commit [FLAGS]`
- **Aliases**: `pre-commit`
- **Source code**: [`src/cli/generate/git-pre-commit.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/git-pre-commit.rs)

[experimental] Generate a git pre-commit hook

This command generates a git pre-commit hook that runs a mise task like `mise run pre-commit`
when you commit changes to your repository.

Staged files are passed to the task as `STAGED`.

## Flags

### `--hook <HOOK>`

Which hook to generate (saves to .git/hooks/$hook)

### `-t --task <TASK>`

The task to run when the pre-commit hook is triggered

### `-w --write`

write to .git/hooks/pre-commit and make it executable

Examples:

    mise generate git-pre-commit --write --task=pre-commit
    git commit -m "feat: add new feature" # runs `mise run pre-commit`
