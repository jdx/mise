# mise-dotenv

A dotenv parser used by mise for `.env` files. Vendored and trimmed from
[`dotenv-ng-core`](https://github.com/cachix/dotenv-ng) 1.0.0 (itself a fork of
[`dotenvy`](https://github.com/allan2/dotenvy)), MIT licensed; see `LICENSE`.

Unlike upstream's loader, `mise_dotenv::parse` never reads the process environment, so a file's own
assignments are not shadowed by ambient variables. Process-environment mutation was removed.

Unlike upstream, a value is read like a shell word, as `dotenvy` read it: adjacent unquoted,
single-quoted, and double-quoted parts are joined with their quotes removed, so `'it'\''s'` is
`it's`. Single-quoted parts are fully literal.
