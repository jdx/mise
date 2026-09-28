# Showreel captures

Every terminal line in the landing-page showreel is real output, recorded
here. This directory records it: two pinned Debian machines, a released mise
binary checked against its pinned sha256, and a list of keystrokes.

```sh
mise run docs:showreel-capture                        # record everything, scan, check
mise run docs:showreel-capture -- --resolve-only      # print the cache key JSON (seconds)
mise run docs:showreel-capture -- --versions-only     # do the captures need redoing? (exit 3)
mise run docs:showreel-capture -- --runs 2            # two cold runs, compared
mise run docs:showreel-capture -- --update-reference  # record, then refresh test/captures
mise run docs:showreel-capture -- --rebuild           # build the images from nothing too
```

Output goes to `out/` (gitignored), or to `--out DIR`; the downloaded mise
release is cached in `out/.cache` either way (`SHOWREEL_CAPTURE_CACHE`
moves it). A run records into
`out/.next` and replaces `out/runs`, `out/versions.json` and the reports only
when every capture has the shape the scenes expect. Otherwise the last good
captures stay in `out/`, and the failed run moves to `out/failed`.

## Running it, on a laptop or in CI

The host needs Linux x86_64, Docker, `python3`, `curl`, `tar` and `xz`
(and `gpg`, to check the release's signed checksums). Nothing needs a
TTY, and the Docker daemon need not see the checkout: files go in and out
of the containers with `docker cp`. The same command runs on a GitHub
Actions or Namespace runner:

```yaml
- run: mise run docs:showreel-capture -- --versions-only --out docs/.vitepress/theme/showreel/test/captures
  id: key # exit 3: the committed reference set is out of date
  continue-on-error: true
- run: mise run docs:showreel-capture
  if: steps.key.outcome == 'failure'
  timeout-minutes: 45
```

Measured on a Linux x86_64 host (Docker 29, cgroup v2), from nothing (no
images, no layer cache, no cached binary):

| Step                                                                 | Time          |
| -------------------------------------------------------------------- | ------------- |
| `--resolve-only`                                                     | 5 s cold, 1 s |
| Download and check mise, pull the base, build both images            | 70 s          |
| Record machine 1 and machine 2 (one run, 18 captures) and check them | 250–260 s     |
| The whole run from nothing                                           | 5m30s–5m40s   |
| The same with the images already built                               | 4m40s–5m      |
| `--machine2-only` (C18, C19 again)                                   | 32 s          |
| A shape failure found with `--until C2`                              | 27 s          |

| Exit | Meaning                                                                                                       |
| ---- | ------------------------------------------------------------------------------------------------------------- |
| 0    | every capture has its expected shape (`--versions-only`: the key matches)                                     |
| 1    | the run failed: Docker, the network, a download or an off-camera step                                         |
| 2    | bad arguments                                                                                                 |
| 3    | `--versions-only`: the recorded captures are out of date                                                      |
| 4    | a capture's shape changed: it ran, but no longer shows what the scenes are built around (`failed/shape.json`) |

Under GitHub Actions a failure is also printed as an `::error` annotation,
and the systemd fallback as a `::warning`.

Each run gets `--attempts` cold tries (default 2), each from fresh
containers, so a flaky mirror or a slow download does not sink it. Every
download is retried: `curl --retry-all-errors` for the mise release, apt's
`Acquire::Retries` in the images, three tries for each image build, and
`MISE_HTTP_RETRIES=5` for mise itself. `out/run.json` records how long each
try took and how it ended.

## The machines

- **Images:** `Dockerfile` has two targets. Both start from Debian
  trixie-slim pinned by digest, and install their packages from the Debian
  snapshot that image was built from (`snapshot.debian.org/…/20260918T000000Z`,
  named in the image's own `debian.sources`), so two builds months apart
  install the same package versions. `machine1` has zsh, git, tmux, curl,
  ca-certificates, python3, pyte and libatomic1. `machine2` adds systemd,
  so bootstrap can start the history watcher as a user service. The images
  hold no mise and no rig files; each run copies them in.
- **mise:** a release binary, never a dev build (its progress header would
  say `-DEBUG`). The default is the newest release (`latest`, read from
  `https://mise.jdx.dev/VERSION`), so a change to mise's node `lts` alias
  reaches the reel once it is released. It is downloaded once into
  `out/.cache/` and checked against the release's `SHASUMS256.asc`, whose
  signature must be the release key in `SECURITY.md`; 2026.9.15 is also
  pinned by sha256 in `xtasks/docs/showreel-capture`, so it needs no gpg.
  `--mise-version V` records with a given release and `--mise PATH` with a
  local binary. The run refuses a binary whose `--version` differs from the
  version asked for.
- **The user:** `you`, `HOME=/home/you`. The shell gets a PTY whose size is
  set before zsh starts (the same as hk's `script -c 'stty cols 80 rows 50;
…'`).
  - Machine 1: `~/.zshrc` holds only `PROMPT='%~ $ '`, `compinit` and
    `eval "$(mise activate zsh)"`.
  - Machine 2 is fresh: mise is installed and the home directory is what
    `useradd` made. The prompt comes from `/etc/zsh/zshrc`, and zsh's
    new-user wizard is removed. Its shell is never activated.
- **The terminal answers queries**, as a real one does at once: a cursor
  position report, device status and attributes, and the default colours
  (OSC 10 and 11, answered with the reel's own terminal colours). Without
  an answer, `gh --version` waited 5 s for its timeout before printing.
  Each capture's `replies.json` lists what was answered.
- **Environment, off camera:** `MISE_EXPERIMENTAL=1`,
  `MISE_TASK_TIMINGS=0`, `MISE_DISABLE_UPDATE_WARNING=1`,
  `MISE_HTTP_RETRIES=5`, and `MISE_MINIMUM_RELEASE_AGE` set to one cutoff
  for the whole invocation (24 hours before it started, rounded down to the
  hour, or `--cutoff`), so `latest` means the same thing in every capture.
  `env -i` starts from nothing, so `CLAUDECODE`, `AI_AGENT`, `CI` and any
  token are unset. No config file on screen carries any of this: daemons are
  enabled only by `MISE_EXPERIMENTAL`, and the shop project has no
  `[settings]`.
- **Staged off camera, so a take shows no wait it would otherwise have**
  (steps.json `setup`; no caption claims the install or start it skips):
  - machine 1: a global `node@<lts_major>`, and api's `mise use
node@<lts_major>` (the file the pitch's card shows starts as that
    command wrote it);
  - C6: `mise install github:cli/cli@<gh_version>`, so `mise use
github:cli/cli` prints its config line without a download (the scene
    shows no install rows for it);
  - C7: dashboard's `mise use node@<other_major>`;
  - C8: `mise install hk@<hk_old> hk@<hk_new>`, so the upgrade shows no
    download (its progress header is cut), and the one-time skills hint;
  - C14: `mise use pitchfork@<pitchfork_version>`, and a Postgres pre-roll
    (`mise run db && mise daemons stop postgres`: the install, `initdb` and a
    first start), so the take's `mise run db` shows a warm start;
  - C15 to C16: the history watcher in tmux, and C16 whole (tracking the
    global config and connecting you/setup).
- **tmux:** machine 1's off-camera terminal. The history watcher,
  `mise dot watch`, runs in its foreground from C15 to C16, so
  `mise dot track` honestly prints "watcher running".
- **Placeholder repos:** `/etc/gitconfig` (never `~/.gitconfig`) maps
  `https://github.com/you/api` and `https://github.com/you/setup` to local
  bare repos with `url.insteadOf`. Machine 1 pushes to them, and the run
  copies them to machine 2.

### Machine 2 and systemd

Machine 2 boots `/sbin/init` in a privileged container. The user manager for
`you` starts at boot (the image enables lingering), and the recording shell
gets the `XDG_RUNTIME_DIR` a login session would have. Then
`mise bootstrap --adopt you/setup` really applies the watcher service
(`dev.mise.mise-history.service`), and C18 shows `mise bootstrap: user
services`.

Whether that works is decided once per invocation, before anything is
recorded: a probe container boots systemd and waits up to 60 s for the user
manager. Every run then records the same variant, so runs compare and
machine 1 matches its machine 2:

- **systemd:** each run's machine 2 must boot the same way; one that does
  not fails its try.
- **plain (the fallback):** when the runner refuses privileged containers or
  systemd does not come up (`--machine2 auto`, the default). Machine 1
  does not declare `[bootstrap.services.mise-history]` in the global config
  it tracks, C18 has no user-services step, and the scene drops the watcher
  tile. `--machine2 systemd` fails instead, and `--machine2 plain` never
  tries.

`out/run.json`, `runs/<run>/run-machine2.json` and every capture's
`meta.json` say which one ran. `SHOWREEL_SYSTEMD_UNAVAILABLE=1` makes the
probe fail, to try the fallback on a machine where systemd works.

## Versions and the cache key

`versions.py` asks the pinned mise, in a throwaway HOME with every mise
directory, the system config directory and the config search ceiling
inside it:

- **LTS major:** the value of mise's own `lts` alias
  (`mise tool-alias ls node`). It is never worked out from odd or even
  numbers.
- **Other major:** the newest released Node major above the LTS major, if
  there is one (`mise latest node`). Otherwise it is the previous LTS major
  from mise's `lts-<codename>` aliases. Today that gives 24 and 26. After
  mise moves `lts` to 26 it gives 26 and 24, and after Node 27.0.0 it gives
  26 and 27.
- **Full versions:** `mise latest` for both Node majors, jq, npm:prettier,
  pitchfork and github:cli/cli. hk is pinned to 2.1.0 and 2.2.0, because
  only 2.2.0 has `--junit-xml`.
- **Postgres (C14):** the major of `mise latest postgres`, and the newest
  release of that major. The shop fixture's `postgres = "…"` reads it.

Scenes and captions read these values from `versions.json`
(`placeholders`), so they never hard-code a version.

**The key.** The captures are recorded again, and the reel re-rendered, only
when something the reel is built around changes:

- **Keyed:** `schema` (bump it in `versions.py` to force a re-record),
  `rig` (the sha256 of the files that decide what the captures show:
  `Dockerfile`, `entry.sh`, `record.py`, `frames.py`, `fixtures.py`,
  `versions.py` and `steps.json`, which hold the Debian snapshot, the hk
  pair and the fixture date), and the two Node majors. When jdx moves
  mise's `lts` alias, or Node ships a new major, the key changes and the
  reel follows by itself.
- **Not keyed:** the mise version and binary, Node's full versions, jq,
  npm:prettier, hk, pitchfork, gh, Postgres and the cutoff. A patch release
  changes no scene and no caption, and a reel recorded before it still shows
  real output from a real run, so keying on them would re-record for noise
  about every week (mise alone releases several times a week). They are
  recorded, and refresh whenever the key does, or when `schema` is bumped.

`key` is the sha256 of the `keyed` object as
`json.dumps(keyed, sort_keys=True, separators=(",", ":"))`.
`--resolve-only` prints exactly the JSON the key is made from, and what it
leaves out, installing nothing but the mise binary (about 1 s once the
binary is cached, 5 s from nothing):

```json
{
  "key": "…",
  "keyed": {
    "schema": 1,
    "rig": "…",
    "node_lts_major": 24,
    "node_other_major": 26
  },
  "not_keyed": {
    "cutoff": "2026-09-27T01:00:00Z",
    "mise": "2026.9.15",
    "mise_sha256": "…",
    "node_lts_version": "24.21.0",
    "node_other_version": "26.10.0",
    "node_other_rule": "newest-major-above-lts",
    "jq": "1.8.2",
    "npm:prettier": "3.9.9",
    "hk": ["2.1.0", "2.2.0"],
    "pitchfork": "2.27.0",
    "github:cli/cli": "2.101.0",
    "postgres": "18.6"
  }
}
```

A docs build compares `key` with the one in `out/versions.json` (or the
reference set's); `--versions-only` does that, lists what moved, and exits 3
when the key differs. The recorded runs resolve inside machine 1 and must
agree with the host; if a release lands mid-run, the run says so.

## What is fixed, and what may vary

Fixed, so two runs record the same thing: the image (digest and Debian
snapshot), the mise binary (sha256), the cutoff (one per invocation), the
fixture files and their modification times (`fixtures.py` `FIXTURE_DATE`,
also the author and committer date of every off-camera commit), the typing
speed, and the terminal size.

Allowed to vary, and masked when two runs are compared: durations and
transfer rates, wall-clock times (Postgres's log, the `When` column of
`mise dot history`), Postgres's process ids, the order of the parallel
`lint`, `test` and `build` lines, and the order of `mise lock`'s
provenance downloads.

## Fixtures and trust

`fixtures.py` writes the hand-edited files from `versions.json`.
`steps.json` lists, in order, every action on both machines (top level:
machine 1; `machine2`: machine 2):

- the off-camera `setup` for the machine and each capture, and `after` and
  `teardown` actions;
- the keys typed on camera;
- the lines each capture must show (`expect`) and its `shape` rules.

A project whose config is not "safe" starts as the file `mise use` writes
(api: `mise use node@<lts major>`; shop: `mise use pitchfork@<version>`),
and only then gets the rest by hand. `MiseToml::save` trusts the root it
writes, and trust is per root, so no trust prompt or warning ever prints.
dashboard and tools hold only tools. Machine 2's `mise run ci` runs in a
shell that is not activated, and `mise run` trusts silently.

## Recording

`record.py` types each step at a fixed speed. A command counts as finished
when the output has been quiet for 0.4 s and the cursor sits after a fresh
prompt (`%~ $` and a space), so a slow download changes when a step finishes, never what
is typed. A step can instead wait for text on screen (`"wait": "screen"`),
and can answer confirm prompts along the way (`"answer"`): each prompt is
answered with Enter once it has been on screen for a second, and marked
`confirm-N`.

- `offcam` captures (C16) record nothing and only run their setup.
- `same_shell` captures continue the previous capture's shell: C19 is typed
  into C18's shell. Both directories hold the whole session's bytes, and
  `meta.json` `window` says which part is theirs; C19's window starts once
  its Ctrl-L has cleared the screen. Times are session times.
- Off-camera `sh` output goes to the machine's setup log, and to
  `<capture>/offcam/*.txt` where a step says `save`. `copy` copies a file
  the machine holds into the capture set: C8's `offcam/packslip-hk-<v>.json`
  are the packslip statements mise saved as
  `installs/hk/<v>/.mise-packslip.json`. An `expect` or `reject` on an
  off-camera step stops the run if the output is wrong.
- `snapshot_start` also saves the files as the capture starts
  (`files-start/`): C8's is the `hk = "2.1.0"` file before the upgrade.
- Machine 1's first step saves `mise registry --hide-aliased` from the
  pinned binary as `C4/registry.txt`. `C4.json` (the rivers' names, and the
  count behind "1,000+") is built from it, never from the checkout's
  `registry/`.

Each capture in `out/runs/<run>/<id>/` has:

| File           | Contents                                                          |
| -------------- | ----------------------------------------------------------------- |
| `raw.bin`      | every byte the terminal received, unmodified                      |
| `events.json`  | `[t, offset, length]` for each read                               |
| `input.json`   | `[t, text]` for each key sent                                     |
| `replies.json` | `[t, text]` for each answer the terminal gave a program's query   |
| `marks.json`   | the moment each step's command finished                           |
| `meta.json`    | size, directory, argv, the full environment, errors, expectations |
| `files/`       | the mise config files as they were after the capture              |
| `files-start/` | the same files as the capture started (`snapshot_start`)          |
| `offcam/`      | saved off-camera output and copied files (C8, C14 to C19)         |
| `frames.json`  | every distinct screen (below)                                     |
| `final.txt`    | the last screen, as plain text                                    |
| `marks.txt`    | the screen at each mark, as plain text                            |
| `lines.txt`    | each distinct row, with the time it first appeared                |

`frames.py` replays `raw.bin` through pyte. Following hk, it takes a frame
at every `ESC[?2026l` and at the end of every read outside a
synchronized-output block, and merges identical frames. Its `Screen` keeps
the attributes pyte drops: dim, blink, and bright colours without bold.
`frames.json` holds:

- `styles`: each `{fg, bg, bold, dim, italics, underline, strikethrough, reverse, blink}`;
- `rows`: each distinct row, as `[text, style]` runs;
- `frames`: `{t, offset, rows: [row per screen line], cursor, cursor_hidden}`;
- `marks`: each mark with its frame index;
- `window`, for a capture that shares its shell.

A colour is `default`, one of the 16 names (`red`, `brightblue`, …),
`256:N`, or `#rrggbb`.

Off-camera output goes to `out/runs/<run>/setup/` and `setup-machine2/`.
The one-time skills hint is printed there by a real `mise use hk@2.1.0` in
a PTY, and the watcher's terminal is saved as `setup/watch.txt`.

## Checks

`shape.py` is the gate, run on every try and again over the whole run. For
every capture it checks, against `steps.json`:

- the recorder reported no error (no step timed out);
- every `expect` line showed, in order where `shape.order` says so;
- no `shape.reject` line showed (C14: no initdb output);
- `shape.tail`: the screen at a mark ends with these rows (C7 and C14:
  `cd` prints nothing);
- forbidden text (`trust`, `asdf`, `.tool-versions`, `experimental`,
  `mise WARN`, `Finished in`, `installed … in`, update and release-age
  notices, `-DEBUG`) shows only where an `allow` rule says the scene cuts
  around it: below a screen row (`rows_from`, the C1 crop) or after a line
  the take already has (`after`);
- no row still on screen at a mark carries a duration or a rate, unless an
  `allow` rule covers it;
- C4 lists at least `min_count` (1000) names, none twice.

It writes `out/shape.json`, including every cut: for each row an `after`
rule lets through, the last clean frame before it and its time. A failure
exits 4 with each difference and what to do about it.

`scan.py` reports, and edits nothing:

- forbidden text, with when it shows;
- timing text: every duration or rate on screen, and how long each stays
  unchanged;
- with two or more runs, the differences between the first two: the screens
  at each mark, the order of the settled lines, and each line's colours.

`scan.py --compare DIR DIR` compares two runs from different invocations the
same way, and also their files, snapshots and off-camera files.

## The reference set

PR CI has no Docker and no network, so the checkout carries one real run:
`docs/.vitepress/theme/showreel/test/captures/`. `export.py` trims a
published run to what `load.ts` reads (frames without byte offsets, marks,
keys, and each take's files bundled into one `files.json`, so no captured
`.gitignore` or `mise.toml` acts on the checkout or is reformatted by its
linters). The loader reads `SHOWREEL_CAPTURES`, else `out/` once a run has
published there, else the reference set, so the reel's tests and renders
use real captures everywhere.

`test/captures.test.ts` fails when a rig file changed since the reference
set was recorded; `--update-reference` records and refreshes it (commit the
result). With a capture run on the machine, the test also checks that the
loader reads the run and the reference set into facts of the same shape,
and, for the run it was exported from, into the same facts.

## Known results that the scenes must cut around

With mise 2026.9.15:

- **C1:** the help text says `[experimental]` on screen rows 17, 21 and 49,
  and `trust`/`untrust` on rows 84 and 88. The reel shows only rows 1–7.
- **C2, C11, C12, C19:** `mise run` prints `Finished in …` after its tasks
  whenever more than one task runs. `MISE_TASK_TIMINGS=0` does not remove
  it; only the `--no-timings` flag does.
- **C3, C5, C18, C19:** the install summary `✓ installed N tools in …`.
  While a download is slow, the progress header also shows a rate such as
  `128 kB/s`.
- **C17:** `mise lock` ends with `mise WARN  missing: npm:prettier@3.9.9`,
  printed by the shell hook at the next prompt: the lock's aube sidecar
  moves prettier's install key, and the copy `mise use` installed no
  longer counts. This is a mise bug; the scene cuts at
  `✓ Lockfile written to ~/work/api/mise.lock`.
- **C17:** `✓ Lockfile written to ~/work/api/mise.lock` is sometimes erased
  in the same instant it prints (1 of the 2 final runs): the progress
  display's last clear takes its row, and the `mise WARN` lands there. The
  shape check still sees it first; a scene that holds the finished screen
  must cut on the frame it shows.
- **C2, C11, C12, C19:** the order of the parallel `lint`, `test` and
  `build` lines changes from run to run, and so does the order of C17's
  provenance downloads.
