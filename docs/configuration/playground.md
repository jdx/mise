---
description: "Write a mise.toml in the browser: hover any key to see what it does, get completions for keys and values, and see mistakes as you type."
---

# mise.toml playground

Write a `mise.toml` here and the editor checks it against the same schema mise
and your editor use. Hover any key or value to see what it does, its type and
the values it takes, with a link to the
[mise.toml reference](/configuration/reference.html). Start a new line, or press
<kbd>Ctrl</kbd>+<kbd>Space</kbd>, for the keys a table can take, and type
`[` at the start of a line for the sections. A key mise does not read is
underlined with the key it most likely meant, and a value of the wrong kind is
marked.

Nothing leaves your browser. **Copy link** puts the config in the page's
address, so you can share it.

```toml editor share
min_version = "2026.10.0"

[tools]
node = "24"
python = "3.13"
"github:cli/cli" = "latest"

[env]
NODE_ENV = "development"
_.path = ["./node_modules/.bin"]

[tasks.build]
description = "Build the app"
run = "npm run build"
depends = ["install"]

[tasks.install]
run = "npm install"
sources = ["package.json", "package-lock.json"]
outputs = ["node_modules/.package-lock.json"]

[settings]
experimental = true
```

To see what each key of a file on disk does, run
[`mise config explain`](/cli/config/explain.html).
