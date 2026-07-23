# brew-cask native handoff gate (Plan 012)

## Status: DONE — unsupported; mise-only retained

GitHub Actions run
[`29979380126`](https://github.com/donbeave/mise/actions/runs/29979380126)
completed the final deterministic matrix on a fresh GitHub-hosted `macos-15`
runner against Homebrew commit
`6bd951d96e7ebc54787799dba77bfb26ec956c4c`. Isolation assertions, a
scenario sentinel, local checksum-pinned tap/archive fixtures, and exact
before/after state were required.

## Decision

| Outcome                              | Selected                    |
| ------------------------------------ | --------------------------- |
| Proven class-limited handoff         | no                          |
| Native reinstall only                | documented, not productized |
| **Unsupported — mise-only retained** | **yes**                     |

Production code must not expose transfer. Native Homebrew reinstall remains an
intentional manager/payload replacement, not a preserving handoff.

## Evidence and classification

| Class / condition                    | Result     | Reason                                                                                                         |
| ------------------------------------ | ---------- | -------------------------------------------------------------------------------------------------------------- |
| identical app, same-version Caskroom | ineligible | happy path and Homebrew lifecycle pass, but rollback after target mutation is not deterministically observable |
| different app                        | ineligible | `--adopt` succeeds and preserves arbitrary differing bytes; it does not prove equality                         |
| `auto_updates` app                   | ineligible | differing bytes are accepted                                                                                   |
| binary                               | ineligible | existing target fails; retry only succeeds after removing it, which is native install                          |
| mixed app + binary                   | ineligible | binary conflict fails after Homebrew removes the adopted app                                                   |
| formula dependency + binary          | ineligible | failed attempt installs and retains dependency state                                                           |
| pkg / flight hook                    | ineligible | rejected before mutation by the mise eligibility gate                                                          |
| checksum failure before stage        | retry-safe | payload unchanged, marker absent, corrected retry succeeds                                                     |

Runs `29977074980`, `29978638514`, `29978779650`,
`29979070381`, and `29979380126` provide incremental evidence. The final run
artifact contains exact digests, markers, dependency results, sentinel rows,
and logs.

Failure injection after target action and before/after tab cannot be observed
or controlled through Homebrew's supported CLI. Per Plan 012's STOP rule, that
ambiguity excludes even app-only handoff; it is not deferred as a production
experiment.

## Safe product boundary

- Mise-owned pours publish only `.mise-cask.toml`.
- Homebrew `.metadata` is never synthesized or repaired.
- Foreign Homebrew metadata is preserved byte-for-byte.
- Any Homebrew marker blocks mise mutation across versions.
- Use Homebrew from the start when Homebrew lifecycle is required.
- Plan 009 records the smallest missing supported capability; no upstream
  contact occurred.
