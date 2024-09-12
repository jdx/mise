## `mise generate github-action [OPTIONS]` <Badge type="warning" text="experimental" />

```text
[experimental] Generate a GitHub Action workflow file

This command generates a GitHub Action workflow file that runs a mise task like `mise run ci`
when you push changes to your repository.

Usage: generate github-action [OPTIONS]

Options:
  -n, --name <NAME>
          the name of the workflow to generate
          
          [default: ci]

  -t, --task <TASK>
          The task to run when the workflow is triggered
          
          [default: ci]

  -w, --write
          write to .github/workflows/$name.yml

Examples:

    $ mise generate github-action --write --task=ci
    $ git commit -m "feat: add new feature"
    $ git push # runs `mise run ci` on GitHub
```
