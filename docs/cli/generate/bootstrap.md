# `mise generate bootstrap`

- **Usage**: `mise generate bootstrap [FLAGS]`
- **Source code**: [`src/cli/generate/bootstrap.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/bootstrap.rs)

[experimental] Generate a script to download+execute mise

This is designed to be used in a project where contributors may not have mise installed.

## Flags

### `-l --localize`

Sandboxes mise internal directories like MISE_DATA_DIR and MISE_CACHE_DIR into a `.mise` directory in the project

This is necessary if users may use a different version of mise outside the project.

### `--localized-dir <LOCALIZED_DIR>`

Directory to put localized data into

### `-V --version <VERSION>`

Specify mise version to fetch

### `-w --write <WRITE>`

instead of outputting the script to stdout, write to a file and make it executable

Examples:

```
mise generate bootstrap >./bin/mise
chmod +x ./bin/mise
./bin/mise install – automatically downloads mise to .mise if not already installed
```
