# Mise + Neovim Cookbook

Here are some tips for an improved mise workflow with [Neovim](https://github.com/neovim/neovim).

## Code highlight for run commands

Use [Treesitter](https://github.com/nvim-treesitter/nvim-treesitter) to highlight code in the run commands of your mise files as shown on the left side of the image:

![image](https://github.com/user-attachments/assets/2961163b-1e6b-4ff6-b2e6-29eb53afc7e5)

In your neovim config, create a `after/queries/toml/injections.scm` file with these queries:

```scm
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

To only apply the highlighting on mise files instead of all toml files, the `is-mise?` predicate is used. If you don't care for this distinction, the lines containing `(#is-mise?)` can be removed.
Otherwise, make sure to also create the predicate somewhere in your neovim config.

For example, using [`lazy.nvim`](https://github.com/folke/lazy.nvim):

```lua
{
  "nvim-treesitter/nvim-treesitter",
  init = function()
    require("vim.treesitter.query").add_predicate("is-mise?", function(_, _, bufnr, _)
      local filepath = vim.api.nvim_buf_get_name(tonumber(bufnr) or 0)
      local filename = vim.fn.fnamemodify(filepath, ":t")
      return string.match(filename, ".*mise.*%.toml$") ~= nil
    end, { force = true, all = false })
  end,
},
```

This will consider any `toml` file containing `mise` in its name as a mise file.

## Enable LSP for embedded lang in run commands

Use [`otter.nvim`](https://github.com/jmbuhr/otter.nvim) to enable LSP features and code completion for code embedded in your mise files.

Again using [`lazy.nvim`](https://github.com/folke/lazy.nvim):

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

This will only work if the [TS injection queries](#code-highlight-for-run-commands) are also set up.
