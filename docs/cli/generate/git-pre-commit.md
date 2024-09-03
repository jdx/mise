## `mise generate git-pre-commit [OPTIONS]` <Badge type="warning" text="experimental" />

**Aliases:** `pre-commit`

```text
[experimental] Generate a git pre-commit hook

This command generates a git pre-commit hook that runs a mise task like `mise run pre-commit`
when you commit changes to your repository.

Usage: generate git-pre-commit [OPTIONS]

Options:
      --hook <HOOK>
          Which hook to generate (saves to .git/hooks/$hook)
          
          [default: pre-commit]

  -t, --task <TASK>
          The task to run when the pre-commit hook is triggered
          
          [default: pre-commit]

  -w, --write
          write to .git/hooks/pre-commit and make it executable

Examples:

    $ mise generate git-pre-commit --write --task=pre-commit
    $ git commit -m "feat: add new feature" # runs `mise run pre-commit`
```
