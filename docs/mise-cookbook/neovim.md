---
description: "Highlight scripts embedded in mise.toml and metadata in file tasks, then add language-server features with otter.nvim."
socialDescription: "Highlight mise task scripts in Neovim and add language-server features with otter.nvim."
---

# Neovim

Highlight the scripts embedded in `mise.toml` and the metadata comments in file
tasks with Treesitter, then add language-server features to embedded scripts
with otter.nvim.

Before adding the queries, install the Treesitter parsers for `toml`, `bash`, and
any injected languages you use (`kdl` for `#USAGE`, for example). Enable Treesitter
highlighting through your Neovim setup. Query files alone do not install parsers
or start highlighting; see [Neovim's Treesitter documentation](https://neovim.io/doc/user/treesitter.html).

Paths below are relative to `stdpath("config")`, normally `~/.config/nvim`. The Lua
plugin specifications assume you already use lazy.nvim.

## Syntax highlighting

### Run commands

Use [Treesitter](https://github.com/nvim-treesitter/nvim-treesitter) to highlight
the code in the `run` commands of your mise files. The screenshot shows the
result:

![run cmd syntax highlighting demo](./run-cmd-syntax-hl.png)

In your Neovim config, create an `after/queries/toml/injections.scm` file with these queries:

```query
; extends

(pair
  (bare_key) @key (#eq? @key "run")
  (string) @injection.content @injection.language

  (#is-mise?)
  (#match? @injection.language "^['\"]{3}\n*#!(/\\w+)+/env\\s+\\w+") ; multiline shebang using env
  (#gsub! @injection.language "^.*#!/.*/env%s+([^%s]+).*" "%1") ; extract lang
  (#offset! @injection.content 0 3 0 -3) ; rm quotes
)

(pair
  (bare_key) @key (#eq? @key "run")
  (string) @injection.content @injection.language

  (#is-mise?)
  (#match? @injection.language "^['\"]{3}\n*#!(/\\w+)+\s*\n") ; multiline shebang
  (#gsub! @injection.language "^.*#!/.*/([^/%s]+).*" "%1") ; extract lang
  (#offset! @injection.content 0 3 0 -3) ; rm quotes
)

(pair
  (bare_key) @key (#eq? @key "run")
  (string) @injection.content

  (#is-mise?)
  (#match? @injection.content "^['\"]{3}\n*.*") ; multiline
  (#not-match? @injection.content "^['\"]{3}\n*#!") ; no shebang
  (#offset! @injection.content 0 3 0 -3) ; rm quotes
  (#set! injection.language "bash") ; default to bash
)

(pair
  (bare_key) @key (#eq? @key "run")
  (string) @injection.content

  (#is-mise?)
  (#not-match? @injection.content "^['\"]{3}") ; not multiline
  (#offset! @injection.content 0 1 0 -1) ; rm quotes
  (#set! injection.language "bash") ; default to bash
)
```

The `is-mise?` predicate restricts the highlighting to mise files instead of all
TOML files. If you do not need this distinction, remove the lines containing
`(#is-mise?)`. Otherwise, define the predicate in your Neovim config, for
example with [`lazy.nvim`](https://github.com/folke/lazy.nvim):

```lua
{
  "nvim-treesitter/nvim-treesitter",
  init = function()
    require("vim.treesitter.query").add_predicate("is-mise?", function(_, _, bufnr, _)
      local filepath = vim.fs.normalize(vim.api.nvim_buf_get_name(tonumber(bufnr) or 0))
      local filename = vim.fn.fnamemodify(filepath, ":t")
      return filename:match("^%.?mise.*%.toml$") ~= nil
        or filepath:match("/%.?mise/config%.toml$") ~= nil
        or filepath:match("/%.?mise/config%.local%.toml$") ~= nil
        or filepath:match("/%.?mise/config%.[^/]+%.toml$") ~= nil
        or filepath:match("/%.config/mise/mise%.toml$") ~= nil
        or filepath:match("/%.config/mise/mise%.local%.toml$") ~= nil
        or filepath:match("/%.?mise/conf%.d/[^/]+%.toml$") ~= nil
    end, { force = true, all = false })
  end,
},
```

This recognizes mise-named files and grouped config files such as
`.config/mise/config.toml`. Adjust the predicate if your project uses a custom
config filename.

The shebang queries handle a direct interpreter path and `/usr/bin/env <name>`.
The extracted name must match an installed Treesitter language; wrappers such as
`env -S uv run` and versioned names such as `python3` need a custom mapping or
query. The Bash fallback controls highlighting only; mise's actual default shell
is described in [TOML tasks](/tasks/toml-tasks.html#shell-shebang).

### `#MISE` and `#USAGE` comments in file tasks {#mise-and-usage-comments-in-file-tasks}

Treesitter can also highlight the `#MISE` and `#USAGE` comments in file tasks.
The screenshot shows the result:

![USAGE spec syntax highlighting demo](./usage-spec-syntax-hl.png)

In your Neovim config, create an `after/queries/bash/injections.scm` file with these queries:

```query
; extends

; ============================================================================
; #MISE comments - TOML injection
; ============================================================================
; This injection captures comment lines starting with "#MISE ", "# MISE ",
; "#[MISE] " or "# [MISE] " and treats them as TOML code blocks for syntax
; highlighting.
;
; #MISE format
; The (#offset!) directive skips the "#MISE " prefix (6 characters) from the source
((comment) @injection.content
  (#lua-match? @injection.content "^#MISE ")
  (#offset! @injection.content 0 6 0 1)
  (#set! injection.language "toml"))

; # MISE format
((comment) @injection.content
  (#lua-match? @injection.content "^# MISE ")
  (#offset! @injection.content 0 7 0 1)
  (#set! injection.language "toml"))

; #[MISE] format
((comment) @injection.content
  (#lua-match? @injection.content "^#%[MISE%] ")
  (#offset! @injection.content 0 8 0 1)
  (#set! injection.language "toml"))

; # [MISE] format
((comment) @injection.content
  (#lua-match? @injection.content "^# %[MISE%] ")
  (#offset! @injection.content 0 9 0 1)
  (#set! injection.language "toml"))

; ============================================================================
; #USAGE comments - KDL injection
; ============================================================================
; This injection captures consecutive comment lines starting with "#USAGE ",
; "# USAGE ", "#[USAGE] " or "# [USAGE] " and treats them as a single KDL code
; block for syntax highlighting.
;
; #USAGE format
((comment) @injection.content
  (#lua-match? @injection.content "^#USAGE ")
  ; Extend the range one byte to the right, to include the trailing newline.
  ; see https://github.com/neovim/neovim/discussions/36669#discussioncomment-15054154
  (#offset! @injection.content 0 7 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))

; # USAGE format
((comment) @injection.content
  (#lua-match? @injection.content "^# USAGE ")
  (#offset! @injection.content 0 8 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))

; #[USAGE] format
((comment) @injection.content
  (#lua-match? @injection.content "^#%[USAGE%] ")
  (#offset! @injection.content 0 9 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))

; # [USAGE] format
((comment) @injection.content
  (#lua-match? @injection.content "^# %[USAGE%] ")
  (#offset! @injection.content 0 10 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))
```

These combined injections work on Neovim 0.11 and 0.12. Neovim 0.12 also accepts
a multi-node pattern, `((comment)+ @injection.content ...)`, without
`injection.combined`. That pattern requires every comment in a run of
consecutive comments to match, so it does not highlight a `#USAGE` block that
directly follows the shebang or a `#MISE` line, and Neovim 0.11 and earlier
reject it with an `#offset!` error.

The same queries work with other grammars that represent `#` comments as
`comment` nodes. Use `:InspectTree` to check the node names in your parser.
Treesitter injections are per language, so add the queries to each language's
query file. For example, put them in `after/queries/python/injections.scm` to
enable them for Python as well as Bash.

For languages with `//` comments, such as JavaScript, create
`after/queries/javascript/injections.scm` with:

```query
; extends

((comment) @injection.content
  (#lua-match? @injection.content "^//MISE ")
  (#offset! @injection.content 0 7 0 1)
  (#set! injection.language "toml"))
((comment) @injection.content
  (#lua-match? @injection.content "^// MISE ")
  (#offset! @injection.content 0 8 0 1)
  (#set! injection.language "toml"))
((comment) @injection.content
  (#lua-match? @injection.content "^//%[MISE%] ")
  (#offset! @injection.content 0 9 0 1)
  (#set! injection.language "toml"))
((comment) @injection.content
  (#lua-match? @injection.content "^// %[MISE%] ")
  (#offset! @injection.content 0 10 0 1)
  (#set! injection.language "toml"))
((comment) @injection.content
  (#lua-match? @injection.content "^//USAGE ")
  (#offset! @injection.content 0 8 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))
((comment) @injection.content
  (#lua-match? @injection.content "^// USAGE ")
  (#offset! @injection.content 0 9 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))
((comment) @injection.content
  (#lua-match? @injection.content "^//%[USAGE%] ")
  (#offset! @injection.content 0 10 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))
((comment) @injection.content
  (#lua-match? @injection.content "^// %[USAGE%] ")
  (#offset! @injection.content 0 11 0 1)
  (#set! injection.combined)
  (#set! injection.language "kdl"))
```

Keep the `; extends` line. Without it, Neovim uses only the first
`injections.scm` on `runtimepath`, and `after/` directories come last, so your
file is ignored whenever Neovim or a plugin already provides injections for that
language. To replace the bundled queries instead, put the file in
`~/.config/nvim/queries/<lang>/` (not `after/`) and leave out `; extends`.

## Add LSP features to embedded scripts {#enable-lsp-for-embedded-lang-in-run-commands}

Use [`otter.nvim`](https://github.com/jmbuhr/otter.nvim) to enable LSP features
and code completion for code embedded in your mise files. With
[`lazy.nvim`](https://github.com/folke/lazy.nvim):

```lua
{
  "jmbuhr/otter.nvim",
  dependencies = {
    "nvim-treesitter/nvim-treesitter",
  },
  config = function()
    vim.api.nvim_create_autocmd({ "FileType" }, {
      pattern = { "toml" },
      group = vim.api.nvim_create_augroup("EmbedToml", {}),
      callback = function()
        require("otter").activate()
      end,
    })
  end,
},
```

This activates otter in every TOML buffer. To limit it to mise files, check the
buffer name with the same patterns as the `is-mise?` predicate before calling
`activate()`.

otter.nvim needs both the [injection queries](#run-commands) and a configured
language server for each embedded language. It creates the embedded buffers and
routes requests; it does not install the language servers. See
[otter.nvim's setup guide](https://github.com/jmbuhr/otter.nvim#how-do-i-use-otternvim).

## Troubleshooting

- Run `:checkhealth vim.treesitter` to check parser availability.
- Use `:InspectTree` to confirm the TOML `run` value or file-task comment matches
  the query's node types. These queries cover string `run` values; task arrays and
  `run_windows` need additional patterns.
- If a predicate is unknown, load the Lua registration before opening the file,
  or remove `(#is-mise?)` to apply the query to all TOML files.
- If highlighting works but LSP features do not, verify that the same language
  server works in an ordinary file before debugging the embedded buffer.
