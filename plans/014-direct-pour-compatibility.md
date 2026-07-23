# Plan 014: Prove representative direct-pour compatibility

## Status

**IN PROGRESS — 2026-07-23.** Local fork parses and dry-runs all 39 downstream
cask declarations. Real macOS pours pass for Cloudflare WARP, TablePlus, Zoom,
OrbStack, Surge, Plex Media Server, Zed Preview, and Tunnelblick without
Homebrew cask ownership. Latest-build Zed reinstall proves generated
completions and receipt fingerprints. Consumer bootstrap migration is
implemented and locally tested. GitHub-hosted disposable macOS workflow remains
final acceptance gate.

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plans 011 and 013

## Required outcome

Standard `mise bootstrap` installs each selected cask through `brew-cask:`
without a real-Homebrew cask fallback.

## Artifact matrix

| Cask              | Required class                                      |
| ----------------- | --------------------------------------------------- |
| cloudflare-warp   | pkg + checksum-bound lifecycle source               |
| tableplus         | large DMG app                                       |
| zoom              | pkg + postflight                                    |
| orbstack          | app + `$APPDIR` binaries + completions + postflight |
| surge             | app + nested application target + bin/sbin targets  |
| plex-media-server | app + `$APPDIR` binary                              |
| zed@preview       | app + renamed binary + generated completions        |
| tunnelblick       | app + uninstall-only preflight steps                |

Declared shell-completion files and executable-generated completions are
installed, recorded, and included in payload status.

## Acceptance

- [x] Exact current API definitions parse for all eight casks.
- [x] Unit fixtures cover nested `$APPDIR` targets and uninstall-only steps.
- [ ] Disposable GitHub-hosted macOS installs representative app, pkg, hook,
      nested-target, and large-DMG casks through the built fork.
- [x] Installed status succeeds and mise-created targets are verified locally.
- [x] A downstream bootstrap declares all eight casks in TOML.
- [x] Its fallback path contains no cask installation.
- [x] Mise tests/lint/diff check pass; downstream tests/diff check pass (it has
      no lint task; repository-wide rustfmt drift predates this change).
- [x] Plans 001, 011, 013, 014 and the index match current verified evidence.
