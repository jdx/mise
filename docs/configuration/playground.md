---
description: "Write a mise.toml in the browser: hover any key to see what it does, get completions for keys and values, and see mistakes as you type."
---

# mise.toml playground

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

Hover any key or value to see what it does, its type and the values it takes.
Press <kbd>Ctrl</kbd>+<kbd>Space</kbd> for completions; mistakes are marked as
you type. The [mise.toml reference](/configuration/reference.html) has every
key. Editing happens in your browser. **Copy link** puts the config in the
page's address so you can share it, which means anyone who opens that link, and
the docs server that serves it, receives the config. Leave secrets out of it.

To see what each key of a file on disk does, run
[`mise config explain`](/cli/config/explain.html).
