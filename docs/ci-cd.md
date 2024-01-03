# CI/CD

Using mise in CI/CD is a great way to synchronize tool versions for dev/build.

### GitHub Actions

mise is pretty easy to use without an action:

```yaml
jobs:
  build:
    steps:
    - run: |
        curl https://mise.jdx.dev/install.sh | sh
        echo "$HOME/.local/bin" >> $GITHUB_PATH
        echo "$HOME/.local/share/mise/shims" >> $GITHUB_PATH
```

Or you can use the custom action [`jdx/mise-action`](https://github.com/jdx/mise-action):

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: jdx/mise-action@v1
      - run: node -v # will be the node version from `.mise.toml`/`.tool-versions`
```
