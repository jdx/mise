---
name: update-aube
description: Procedure for bumping the aube package manager that mise embeds for its npm backend (the `aube` and `aube-registry` crates). Use when updating aube, reviewing an aube Renovate PR, or when mise needs a newer aube feature.
---

# Updating Embedded aube

- Update `aube` and `aube-registry` together and refresh all aube workspace crates in `Cargo.lock`.
- Review the upstream changes for embedder API or behavior changes and make any required mise integration changes.
- Update the standalone `aube` development tool entry in `mise.lock` to the same version so local and CI workflows exercise the version mise embeds.
- Run these focused checks:
  - `cargo check --locked`
  - `cargo test --locked --bin mise aube`
  - `cargo test --locked --bin mise task::workspace::node::tests`
  - `mise run test:e2e e2e/backend/test_npm_aube`
