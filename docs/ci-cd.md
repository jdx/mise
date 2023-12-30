# CI/CD

Using rtx in CI/CD is a great way to synchronize tool versions for dev/build.

### GitHub Actions

rtx is pretty easy to use without an action:

```yaml
jobs:
  build:
    steps:
    - run: |
        curl https://rtx.jdx.dev/install.sh | sh
        echo "$HOME/.local/share/rtx/bin" >> $GITHUB_PATH
        echo "$HOME/.local/share/rtx/shims" >> $GITHUB_PATH
```

Or you can use the custom action [`jdx/rtx-action`](https://github.com/jdx/rtx-action):

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: jdx/rtx-action@v1
      - run: node -v # will be the node version from `.rtx.toml`/`.tool-versions`
```
