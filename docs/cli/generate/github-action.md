# `mise generate github-action`

- **Usage**: `mise generate github-action [FLAGS]`
- **Source code**: [`src/cli/generate/github-action.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/github-action.rs)

[experimental] Generate a GitHub Action workflow file

This command generates a GitHub Action workflow file that runs a mise task like `mise run ci`
when you push changes to your repository.

## Flags

### `--name <NAME>`

the name of the workflow to generate

### `-t --task <TASK>`

The task to run when the workflow is triggered

### `-w --write`

write to .github/workflows/$name.yml

Examples:

```
mise generate github-action --write --task=ci
git commit -m "feat: add new feature"
git push # runs `mise run ci` on GitHub
```
