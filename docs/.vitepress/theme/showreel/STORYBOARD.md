# mise showreel storyboard

This is the storyboard the showreel's scenes are built from. It follows plan v3, jdx's decisions of 2026-09-27, and the retime of 2026-09-28 ("Pacing" below). The same data, machine-readable, is `sections.json` (`id`, `act`, `actLabel`, `beats`, `start`, `end`, `captions`, `captures`, `visual`, `plan`). Generate `timeline.ts` from it or check the timeline against it; do not retype times.

## Rules that apply everywhere

- **Grid:** 60 BPM. A beat is 1 s (60 frames at 60 fps), a 4/4 bar is 4 s. A ticket is one 2/4 bar (2 s); every other section is whole 4/4 bars, or whole bars and a closing 2/4 bar where a whole bar would only add dead time. The end card's events are keyed to measured onsets in the recording, not to the grid.
- **Captions:** one or two lines of at most 36 visible characters (backticks mark code spans and are not drawn). A caption starts to write in half a beat after the event it explains is on screen and still (its `after`, from the scene's events; without them, on its anchor time), at least 4 s after the caption before it started and not before that one has been read. It stays up at least max(words/3 + 1 s, 3 s) from its first word, at least words/3 s once whole, and never under 4 s. Then it holds on: a section's last caption until two beats before the section ends (or, where its reading time runs later, until it has been read, and never past one beat before the end), so its wipe is over before the closing rest, and an earlier one until the next caption's anchor when less than 4 s would be bare between them, so the caption area changes at most once in 4 s. A caption's `until` in `sections.json` overrides that: a time (global seconds, still at most one beat before the section ends), or `"read"` to leave once read (`storyboard.ts` `captionTimes`, `captionsFor`). `morph` and `end` have no captions.
- **Version tokens:** `{node.lts_major}`, `{node.lts_version}`, `{node.other_major}` and `{node.other_version}` come from the capture run's `versions.json`. Today they are 24, 24.21.0, 26 and 26.10.0. Never write these numbers into a scene or caption: `forbidden.test` scans every scene, diagram and kit module for version numbers and Node majors, and `storyboard.test` fills the captions from the tokens. Recorded terminal text keeps the versions it printed.
- **Every terminal line is real captured output.** Terminal text comes from the capture set, never from a string in a scene.
- **Never on screen:** asdf or `.tool-versions`; `mise trust`, trust prompts or warnings; experimental status (no badge, no `experimental = true`, no `[settings]` table, no wording, and none of the help screen's `[experimental]` rows); benchmarks or speed comparisons; durations or rates, except the elapsed times inside install rows while they move under the time-lapse badge (below); an install row with a ✓ or a total; `Finished in`.
- **Copy:** literal and feature-naming. No slogans, no em-dashes, no sincerity filler.
- **Install progress:** only while it is moving, at 3× real time or faster under the "time-lapse" badge, so no elapsed time a row prints stays on screen unchanged for more than 0.1 s; never with a ✓ or a total, and never held still after it stops moving (`timing.test`).
- **Persistent layers:** the station card, the terminal's window and its screen (the frame the incoming scene opens on, the one the outgoing scene holds last, or, where one shell carries on, the very frame both show), the chef, switch's folder diagram, and the file cards and rail a neighbour shares hold through a bar line at rest (`kit/rest.ts`). Diagrams, chips, threads, badges off the chrome bar and marks are gone by it, and no pulse, glow or overshoot crosses it. The whip is the one bar line that keeps nothing. The handoff test holds both sides of every bar line to the same frame under the capture set's facts.
- **Tickets hand the stage on:** a ticket's first bar line holds the outgoing section's stage at rest, as any bar line does, and the ticket takes it down: from beat 0 its layers fall 16 px and fade (leave), a sixteenth apart, gone by beat 0.5, while the rail comes down and the paper prints. From 1.3 the next section's layers rise 24 px and fade in (arrive), at rest by 1.85, under the lifting ticket, so its last bar line holds them (`kit/grey.ts` `ticketStage`, `TICKET.cue`). The stage is empty only while the ticket hangs. The section before a ticket never clears its own stage for it. A ticket scene may move the stage its own way inside those two windows (dotfiles slides the shop card off and `~/.zshrc` in); `new` has nothing to clear, because the whip cleared it.
- **Placeholders:** `you/api` and `you/setup` carry the "illustration" badge on every frame a row shows them (`timing.test`), from the first frame a `you/` is typed until the screen is cleared, so it never blinks as the name scrolls away and back (`kit/rest.ts` `screenBadges`).
- **Rests never show an empty window:** a terminal a bar line keeps shows the outgoing take's last screen or the incoming take's opening prompt, with its cwd title; the other side dissolves its own screen into that one (`kit/rest.ts` `HOLDS`, `OPENS`, `kit/grey.ts` `restScreen`). A terminal never scrolls back up when a prompt widget collapses: the rows at the bottom go blank, as a real terminal leaves them.
- **Staged off camera:** anything a take would otherwise wait on (an install, `initdb` and a first Postgres start, the history watcher) is done off camera and listed in `showreel-capture/README.md`. A take then shows only what its command prints, and no caption claims the install or start that was staged: backends' `mise use github:cli/cli` shows its config line, not an install (gh is installed beforehand), and its caption names where backends install from, not how long it takes.
- **Terminal:** never fills the frame for more than about 4 s. While prose, tables or long paths print, the pane dims and shrinks and a diagram carries the frame.

## Pacing

jdx on the 3:26 cut at 75 BPM: "it's good but it goes WAY too fast, i can hardly follow along". These rules are the acceptance test; `kit/style.ts` `PACE` holds the numbers, `kit/pace.ts` the helpers, `test/pace.test.ts` the rules.

1. **Tempo** 60 BPM: a beat is 1 s, a 4/4 bar 4 s. Tickets stay 2-beat sections.
2. **Typing** plays at `TYPE_RATE` (0.5 × real time: the rig's 45 ms a key shows at about 11 a second, never over 12); the Enter key waits `ENTER_PAUSE` (a quarter beat) on the typed command. The output after Enter plays at 1x. `step()` and `install()` do both by default (`typeRate: 1, pause: 0` for the take's own pace).
3. **Reading output:** a command's output, once printed, stays on screen unmoved for at least 2 s, plus 0.5 s for each line past 3 the viewer reads (texture dimmed by `lineAlpha` is not counted): `readHold`. The next command may type during that hold, but its Enter (which scrolls or replaces the screen) waits for it, and nothing clears, cuts or takes the pane away before it (`Pace.term`, `Pace.readTerm`). Install time-lapse rows stay at 3x or faster; where a take's rows run shorter than 1.5 s at 3x, the tail keeps the entered command and the time-lapse badge up for the rest, so the install beat is up at least 1.5 s (`Pace.lapse`, `lapseUp`).
4. **One focal change at a time:** a new focal motion (terminal output, a line seating, a diagram building, a chip lifting, kinetic type, a caption writing in) starts at least half a beat after the last one stopped (`Pace`, `GAP`). The order is: the terminal acts, the result lands on the card or diagram, then the caption that explains it. Whatever is not the focus dims to 55 % (`focusOf`, `unfocus`, `FOCUS_DIM`). A cascade of like items (three headers lighting, file chips ticking) is one motion.
5. **Captions** appear only once the thing they describe is on screen (`after`), hold at least max(words/3 + 1 s, 3 s), and change at most once every 4 s; they keep their exact text.
6. **Every section ends at rest:** its last result still for at least 1.5 s before the next section's first motion (`restOut`), tickets excepted. The section after a non-ticket may move from its beat 0; after a ticket, from beat 0.5.
7. **Kinetic slams** hold at full size at least 1 s (`DUR.bigHold` 1.25 beats; `slamHold`) before they shrink into place.
8. The opening (`open`) is brisker (its name card holds 2 s) but keeps rules 2-5.
9. The length is what the rules need: every section's beats come from its event plan (`Pace.need`).

Footnotes (40 px) are desktop detail the story never depends on: they take their turn as a motion but get no reading hold.

## Sections

| id          | act               | beats | start  | end    | captures |
| ----------- | ----------------- | ----- | ------ | ------ | -------- |
| `open`      | 0 mise            | 14    | 0:00.0 | 0:14.0 | C1       |
| `pitch`     | 0 mise            | 28    | 0:14.0 | 0:42.0 | C2       |
| `tools`     | I Dev tools       | 2     | 0:42.0 | 0:44.0 | -        |
| `use`       | I Dev tools       | 20    | 0:44.0 | 1:04.0 | C3       |
| `registry`  | I Dev tools       | 12    | 1:04.0 | 1:16.0 | C4       |
| `backends`  | I Dev tools       | 34    | 1:16.0 | 1:50.0 | C5, C6   |
| `versions`  | II Versions       | 2     | 1:50.0 | 1:52.0 | -        |
| `switch`    | II Versions       | 22    | 1:52.0 | 2:14.0 | C7       |
| `packslip`  | II Versions       | 36    | 2:14.0 | 2:50.0 | C8       |
| `env`       | III Environments  | 2     | 2:50.0 | 2:52.0 | -        |
| `vars`      | III Environments  | 20    | 2:52.0 | 3:12.0 | C9       |
| `redact`    | III Environments  | 18    | 3:12.0 | 3:30.0 | C10      |
| `tasks`     | IV Tasks          | 2     | 3:30.0 | 3:32.0 | -        |
| `depends`   | IV Tasks          | 20    | 3:32.0 | 3:52.0 | C11      |
| `skip`      | IV Tasks          | 16    | 3:52.0 | 4:08.0 | C12      |
| `args`      | IV Tasks          | 22    | 4:08.0 | 4:30.0 | C13      |
| `daemons`   | IV Tasks          | 18    | 4:30.0 | 4:48.0 | C14      |
| `dotfiles`  | V Dotfiles        | 2     | 4:48.0 | 4:50.0 | -        |
| `track`     | V Dotfiles        | 38    | 4:50.0 | 5:28.0 | C15, C16 |
| `machines`  | VI Everywhere     | 2     | 5:28.0 | 5:30.0 | -        |
| `lock`      | VI Everywhere     | 22    | 5:30.0 | 5:52.0 | C17      |
| `new`       | VII A new machine | 2     | 5:52.0 | 5:54.0 | -        |
| `bootstrap` | VII A new machine | 24    | 5:54.0 | 6:18.0 | C18      |
| `breath`    | VII A new machine | 6     | 6:18.0 | 6:24.0 | C19      |
| `clone`     | VII A new machine | 30    | 6:24.0 | 6:54.0 | C19      |
| `morph`     | VIII Install mise | 8     | 6:54.0 | 7:02.0 | -        |
| `end`       | VIII Install mise | 14    | 7:02.0 | 7:16.0 | -        |

Full cut: 7:16.0 (436 s), 436 beats, 27 sections, 29 captions. The short cut drops `registry`, `redact`, `args` and `daemons` (6:06.0, 23 sections, 23 captions); every section next to one of those ends at rest.

## Captions

Times are anchors (where the first word starts to rise) with the event plans' timing; a scene that reports its events (`greyScene` `events`) anchors each caption half a beat after its `after` event instead.

| at      | section     | after                | caption                                                                              |
| ------- | ----------- | -------------------- | ------------------------------------------------------------------------------------ |
| 0:17.00 | `pitch`     | `tables`             | One `mise.toml` per project: / tools, env vars, and tasks.                           |
| 0:30.62 | `pitch`     | `envLift`            | With `mise activate`, `cd` in, / and its tools and env vars load.                    |
| 0:36.50 | `pitch`     | `ci`                 | `mise run ci` runs the tasks / in that environment.                                  |
| 0:53.75 | `use`       | `seat`               | No `jq`? `mise use jq` installs it / and adds it to `mise.toml`.                     |
| 1:08.00 | `registry`  | `lit` (until read)   | 1,000+ tools by short name, / from `node` to `terraform`.                            |
| 1:24.62 | `backends`  | `seat`               | `npm:` installs npm CLIs per project, / even without a `package.json`.               |
| 1:44.38 | `backends`  | `backendChips`       | Other backends install from / PyPI, crates.io, GitHub, and Go.                       |
| 2:00.62 | `switch`    | `dashVersion`        | `cd` switches `node` to the version / each project's `mise.toml` sets.               |
| 2:17.75 | `packslip`  | `slip`               | hk's signed packslip releases carry / completions and agent skills.                  |
| 2:31.88 | `packslip`  | `junit`              | Upgrade hk, and Tab completes / the new version's flags.                             |
| 2:41.38 | `packslip`  | `link` (until read)  | `mise skills sync` links that / version's skills for your agent.                     |
| 2:58.62 | `vars`      | `envEmpty`           | Leave, and mise restores / the environment.                                          |
| 3:07.00 | `vars`      | `port`               | `.env` files load too, / with `_.file`.                                              |
| 3:23.00 | `redact`    | `stamp`              | Mark a `.env` file `redact = true`: / task output shows `[redacted]`.                |
| 3:38.62 | `depends`   | `lanes`              | `ci` depends on lint, test, build. / They can run in parallel.                       |
| 3:44.00 | `depends`   | `labels`             | Each output line is labeled / with its task.                                         |
| 4:00.75 | `skip`      | `hero` (until read)  | With `sources` and `outputs`, / up-to-date work is skipped.                          |
| 4:14.25 | `args`      | `help`               | Scripts in `mise-tasks/` are tasks. / `#USAGE` adds args and `--help`.               |
| 4:22.75 | `args`      | `tie` (until read)   | Bad input stops the task / before it runs.                                           |
| 4:34.38 | `daemons`   | `card`               | Tasks can declare the daemons / they need, like `postgres`.                          |
| 4:41.75 | `daemons`   | `tie`                | `mise run db` starts Postgres / and waits until it's ready.                          |
| 4:58.00 | `track`     | `cp1` (until 5:08)   | `mise dot track` keeps a history. / A watcher saves each edit.                       |
| 5:22.88 | `track`     | `cp3`                | `mise dot rollback` restores / an earlier version.                                   |
| 5:38.25 | `lock`      | `thread`             | `mise.toml` asks for `{node.lts_major}`. / `mise.lock` records `{node.lts_version}`. |
| 5:44.62 | `lock`      | `run`                | Commit both. CI and other machines / can use the same versions.                      |
| 6:05.62 | `bootstrap` | `tick1`              | `mise bootstrap --adopt` restores / your dotfiles and mise config.                   |
| 6:10.12 | `bootstrap` | `tick2` (until read) | Then it starts the watcher / and installs your global tools.                         |
| 6:36.75 | `clone`     | `toolsLit`           | On a fresh clone, `mise run ci` / installs what `mise.lock` pins.                    |
| 6:48.88 | `clone`     | `envLit`             | Then its tasks run with / the project's env vars.                                    |

`bootstrap`'s second caption becomes "Then it installs your global tools." if machine 2 has no systemd (no Watcher tile).

## Event plans

Each section's focal motions in order, in section beats (start–still), from `sections.json` `plan` (the choreographers' brief, with each event's description). Captions are `captionN`; a caption's `after` names the event it explains.

| section     | beats | events (start–still, section beats)                                                                                                                                                                                                                                                                 |
| ----------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `open`      | 14    | 0.5–1.74 help; 3.74–5.24 lift; 5.74–6.49 tagline; 6.99–7.61 name; 10.11–10.61 leave; 11.11–11.61 pitchTerm                                                                                                                                                                                          |
| `pitch`     | 28    | 0–1 card; 1.5–2.5 tables; 3–4.25 caption1; 4.75–6.17 cd; 6.67–8.81 node; 9.31–10.81 nodeLift; 11.31–14.06 env; 14.56–16.06 envLift; 16.62–18.25 caption2; 18.75–20.27 ci; 22.5–23.75 caption3; 24.25–24.75 chipsOut                                                                                 |
| `tools`     | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `use`       | 20    | 0.5–3.18 notFound; 3.68–5.62 slam; 6.12–7.98 install; 8.48–9.23 seat; 9.75–11.5 caption1; 12–12.88 clear; 13.38–16.04 ok; 18.04–18.41 leave                                                                                                                                                         |
| `registry`  | 12    | 0–2 rivers; 2.5–3.5 lit; 4–5.25 caption1; 9.25–10 riversOut                                                                                                                                                                                                                                         |
| `backends`  | 34    | 0–4.5 npm; 5–6.86 install; 7.36–8.11 seat; 8.62–10 caption1; 10.5–11.38 clear; 11.88–15.9 check; 16.4–17.15 verdict; 17.65–18.4 chips; 18.9–19.4 foot1; 19.9–20.9 toTools; 21.4–24.1 gh; 24.6–26.58 ghVersion; 27.08–27.83 backendChips; 28.38–29.75 caption2; 30.25–30.75 foot2; 31.25–32.5 narrow |
| `versions`  | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `switch`    | 22    | 0.5–2.73 cdDashboard; 3.23–3.98 toDashboard; 4.48–6.6 nodeDashboard; 7.1–8.1 dashVersion; 8.62–10 caption1; 10.5–12.19 cdApi; 12.69–13.44 toApi; 13.94–16.06 nodeApi; 16.56–17.56 apiVersion; 18.25–19 threads; 19.5–20.38 fold                                                                     |
| `packslip`  | 36    | 0–0.25 dissolve; 0.75–1.75 pin; 2.25–3.25 slip; 3.75–5 caption1; 5.5–7.35 tabOld; 9.35–11.48 use; 11.98–12.73 flip; 13.23–13.98 restamp; 14.48–16.32 tabNew; 16.82–17.32 junit; 17.88–19 caption2; 19.5–20 foot; 21.5–23.86 sync; 24.36–26.86 link; 27.38–28.75 caption3; 33.25–33.75 linkOut       |
| `env`       | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `vars`      | 20    | 0.5–1.83 cdOut; 2.33–2.83 strip; 3.33–6.08 envEmpty; 6.62–7.5 caption1; 8–8.75 seatFile; 9.25–9.75 envCard; 10.25–11.67 cdIn; 12.17–14.38 port; 15–15.75 caption2; 16.25–17 envCardOut; 17.5–18.12 stripOut                                                                                         |
| `redact`    | 18    | 0–0.5 dissolve; 1–1.75 zoom; 2.25–3 rewrite; 3.5–4 fold; 4.5–5.25 files; 5.75–8.7 deploy; 9.2–10.45 stamp; 11–12.5 caption1; 13–13.5 foot; 14–16.25 foldBack                                                                                                                                        |
| `tasks`     | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `depends`   | 20    | 0.5–2.02 ci; 4.02–6.02 lanes; 6.62–8.25 caption1; 8.75–9 mark; 9.5–10.25 labels; 12–13.25 caption2; 16.5–17.44 settle; 17.94–18.44 skipPrompt                                                                                                                                                       |
| `skip`      | 16    | 0–1.12 seat; 1.62–3.15 ci; 5.15–6.52 lanes; 7.03–8.15 hero; 8.75–9.75 caption1; 13.75–14.25 back                                                                                                                                                                                                    |
| `args`      | 22    | 0–0.88 slide; 1.38–2.38 usage; 2.88–5.74 help; 6.25–7.75 caption1; 9.11–11.79 bad; 12.29–12.89 shake; 13.39–14.14 tie; 14.75–16 caption2; 19.75–20.5 out                                                                                                                                            |
| `daemons`   | 18    | 0–1.78 cd; 2.28–3.78 card; 4.38–5.5 caption1; 6–8.41 db; 8.91–9.41 lit; 9.91–11.16 tie; 11.75–13.25 caption2; 16–16.5 tieOut                                                                                                                                                                        |
| `dotfiles`  | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `track`     | 38    | 0.5–3.85 confirm; 5.4–6.39 answer; 6.89–7.39 cp1; 8–9.5 caption1; 10–10.5 foot1; 11–15.19 edit; 15.69–16.94 alias; 17.44–17.94 cp2; 18.44–22.21 history; 22.71–26.39 rollbackPlan; 27.94–29.3 rollback; 29.8–30.55 aliasOut; 31.05–32.3 cp3; 32.88–34 caption2; 34.5–35 foot2; 35.5–36 undim        |
| `machines`  | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `lock`      | 22    | 0.5–1.77 lock; 2.27–3.52 request; 4.02–4.52 ledger; 5.02–6.4 entry; 6.9–7.65 thread; 8.25–9.25 caption1; 9.75–10.62 panel; 11.12–13.12 drop; 13.62–14.12 run; 14.62–16.25 caption2; 16.75–19 push                                                                                                   |
| `new`       | 2     | 0–0.5 clear; 0.75 tear; 1.2–1.8 lift; 1.3–1.85 bring                                                                                                                                                                                                                                                |
| `bootstrap` | 24    | 0.5–4.42 adopt; 4.92–5.8 zshrc; 6.3–6.99 tiles; 7.49–9.53 yes1; 10.03–11.09 tick1; 11.62–13 caption1; 13.5–14.05 yes2; 14.55–15.55 tick2; 16.12–17.5 caption2; 21.5–22.12 tilesOut                                                                                                                  |
| `breath`    | 6     | 0–0.75 dim; 1.25–3.25 clear; 3.75–4.5 up                                                                                                                                                                                                                                                            |
| `clone`     | 30    | 0–8.83 type; 9.34–11.14 rows; 11.64–12.14 toolsLit; 12.75–14.25 caption1; 14.75–17.06 lanes; 19.56–20.62 tasksLit; 21.12–21.88 fold; 22.38–23.38 payoff; 23.88–24.38 envLit; 24.88–26 caption2; 26.81–28.5 home                                                                                     |
| `morph`     | 8     | 0–2 lift; 4–5 settle; 6–8 chord                                                                                                                                                                                                                                                                     |
| `end`       | 14    | 0–0.68 mise; 1.48–1.98 dev; 2.78–4.5 curl; 6.25–6.75 platform; 7.52–7.9 site; 8.27–8.77 glint; 11.2–14 silence                                                                                                                                                                                      |

## Visuals

The order of events; their timing is the section's event plan.

### Act 0 · mise (0:00.0–0:42.0)

- **`open`** (0:00.0–0:14.0, 14 beats; captures: C1). `~ $ mise` is typed; rows 1-7 of the real 80-column help screen print beside the green block chef. Then the block chef's cells lift and resolve into the white chef. The white chef resolves on M1's F; then the tagline lifts out of the help screen into the name card as the pane falls away, and the name writes on above it. Name card: "mise-en-place" / "Dev tools, env vars, and tasks in one CLI". The name card gives way to the pitch's terminal at `~/work $`, which holds through the bar line.
- **`pitch`** (0:14.0–0:42.0, 28 beats; captures: C2). The terminal waits at `~/work $`. The card writes `node = "{node.lts_major}"`, `APP_ENV = "api"` and `[tasks.ci] depends = ["lint", "test", "build"]` (its long `run` line folds to a count); [tools] lights pink, [env] gold, [tasks] sage in turn, then caption 1 names them. `~/work $ cd api` (cwd dot hops), `node --version` -> `v{node.lts_version}`, `echo APP_ENV=$APP_ENV` -> `APP_ENV=api`. `mise run ci` streams prefixed lines; holds on the last frame before `Finished in` (`[ci] api ready: node v{node.lts_version}, APP_ENV=api`). `v{node.lts_version}` and `APP_ENV=api` lift out of the terminal onto the card rows that asked for them, with threads; the chips and threads are gone by the bar line, and the card (its three tables lit) and the terminal on that last frame hold into the ticket.

### Act I · Dev tools (0:42.0–1:50.0)

- **`tools`** (0:42.0–0:44.0, 2 beats; captures: none). Ticket: "Dev tools" with `[tools]`, pink. The pitch's card and terminal leave under it as it prints; as it lifts off the rail, use's stage comes in under it: the api card and the terminal at `~/work/api $`.
- **`use`** (0:44.0–1:04.0, 20 beats; captures: C3). `jq .status resp.json` -> `zsh: command not found: jq`. `mise use jq` slams in as big type and shrinks into the pane; progress rows move at 3× under the time-lapse badge and fade while they still move; the entered command holds under the badge, then the terminal dims and `jq = "latest"` seats in the card. ⌃L keycap as the terminal brightens on a fresh prompt, then `jq .status resp.json` -> `"ok"`. The card holds through both bar lines.
- **`registry`** (1:04.0–1:16.0, 12 beats; captures: C4). Three rivers of real registry names; `node` and `terraform` glow. The card stays at rest through the section and both its bar lines.
- **`backends`** (1:16.0–1:50.0, 34 beats; captures: C5, C6). `npm:` types into a slot in front of `prettier` in pink kinetic type beside the card, then shrinks into `mise use npm:prettier`. Rows move at 3× (with the `↳ 1/1 pkgs` sub-row); cut to the card before any ✓; `"npm:prettier" = "latest"` seats. ⌃L, then `prettier --check README.md .github` prints `Checking formatting...` / `All matched files use Prettier code style!`, which zooms out of its row and holds about 2 s; then file chips README.md and .github/workflows/ci.yml come up and tick in turn. Footnote: "Runs on the project's `node`." One pane at `~/work/tools $` (gh installed beforehand): `mise use github:cli/cli` -> `mise ~/work/tools/mise.toml tools: github:cli/cli@<v>` (the progress header it flashes is cut); `gh --version` -> two real lines. Text chips `pypi:`, `cargo:`, `go:`. Footnote: "Some backends need their ecosystem's tool, like `uv`." The pane narrows back into the left column with gh's lines on it, and the card slides back beside it; both hold into the Versions ticket, which takes them down.

### Act II · Versions (1:50.0–2:50.0)

- **`versions`** (1:50.0–1:52.0, 2 beats; captures: none). Ticket: "Versions" with `tool@version`, pink. The api card and gh's pane leave under it as it prints; as it lifts off the rail, the switch's folder diagram (api/ current) and terminal come in under it.
- **`switch`** (1:52.0–2:14.0, 22 beats; captures: C7). Folder diagram: `api/` card `node = "{node.lts_major}"`, `dashboard/` card `node = "{node.other_major}"`; the cards and the terminal hold from the ticket. `~/work/api $ cd ../dashboard` moves the key to D♭; `node --version` -> `v{node.other_version}`; `cd ../api` returns home; `node --version` -> `v{node.lts_version}`. Both versions hold on their folder cards, set in the terminal's face. Poster frame: just after both have landed (reel.ts POSTER_TIME); then each version's thread draws from the row that printed it to its chip. The diagram folds away; the terminal's last screen holds through the bar line into packslip.
- **`packslip`** (2:14.0–2:50.0, 36 beats; captures: C8). Switch's last screen dissolves into C8's prompt over the first quarter beat. `hk = "2.1.0"` lights in [tools]; the terminal shortens and a slip tucks under it ("Excerpt of hk's packslip · real keys, values abridged"): 28 px rows read from the packslip statement mise saved when it installed each version (C8, off camera): `version`, `resources` (the completion shells and the skill names) and `identity`. `hk check --j` + Tab keycap lists 2 rows. `mise use hk@2.2.0` (its progress header cut) -> `mise ~/work/api/mise.toml tools: hk@2.2.0`; the card flips to "2.2.0"; the slip re-stamps from 2.2.0's statement. Tab lists 3 rows, `--junit-xml` lights; footnote "In a shell with `mise activate`." `mise skills sync` prints `linked .claude/skills/...` rows, dimmed; the drawn link takes the slip's place and lights only `2.2.0`. Caption 3 is read, then the drawn link folds away; the card and the dimmed, shortened terminal hold into the Environments ticket.

### Act III · Environments (2:50.0–3:30.0)

- **`env`** (2:50.0–2:52.0, 2 beats; captures: none). Ticket: "Environments" with `[env]`, gold. Packslip's card and dimmed terminal leave under it as it prints; as it lifts off the rail, the api card (at [env]) and the terminal come in under it.
- **`vars`** (2:52.0–3:12.0, 20 beats; captures: C9). `cd ..` folds values off the shell-env strip; `echo APP_ENV=$APP_ENV` -> `APP_ENV=`. `_.file = ".env"` seats; a `.env` file card shows `PORT=3000`. `cd api`, `echo PORT=$PORT` -> `PORT=3000`. The card and the terminal, on its last screen, hold from the ticket and into redact. Ends at rest.
- **`redact`** (3:12.0–3:30.0, 18 beats; captures: C10). The card and vars' last screen hold from vars; the screen dissolves into C10's prompt as it comes up. Its `_.file` line zooms out of it to full width and is rewritten in place as `_.file = [".env", { path = ".env.deploy", redact = true }]`, and the card folds away. `.env.deploy` card ("gitignored") shows the fixture token; `mise-tasks/deploy` card shows its body line. `mise run deploy staging` prints its real line with `[redacted]`; a thread runs from the token to `[redacted]` and stamps it. Footnote while `[redacted]` is visible: "Masks captured task output. Does not encrypt the file." The file cards fold back into the card, [env] folded; the card, the terminal (the `[redacted]` line) and the footnote hold into the Tasks ticket.

### Act IV · Tasks (3:30.0–4:48.0)

- **`tasks`** (3:30.0–3:32.0, 2 beats; captures: none). Ticket: "Tasks" with `[tasks]`, sage. Redact's folded card, terminal and footnote leave under it as it prints; as it lifts off the rail, the api card (at [tasks.ci]) and the terminal come in under it.
- **`depends`** (3:32.0–3:52.0, 20 beats; captures: C11). `mise run ci`: three lanes fill in the capture's order and ANSI colours, then `ci`. Badge: "Order from one real run. Not to scale." The lanes settle into [tasks], and the terminal comes back for skip with skip's own prompt in it. The card holds through both bar lines.
- **`skip`** (3:52.0–4:08.0, 16 beats; captures: C12). `sources`/`outputs` seat on the build task. Only the second `mise run ci` is on camera, from its own prompt: `[build] sources up-to-date, skipping` is the hero. The lanes refill with build greyed, its line set large and stamped "skipped" (badge: "Order from one real run. Not to scale."). Holds on the last frame before `Finished in`; the terminal comes back with args' prompt in it, and the card holds through the bar line; ends at rest.
- **`args`** (4:08.0–4:30.0, 22 beats; captures: C13). Skip's card slides out right as the deploy card comes in (a title change). The deploy card unfolds `#USAGE arg "<env>"` and `choices "staging" "production"`. `mise run deploy --help` prints; its top 7 rows show, a viewport crop down to `[possible values: staging, production]` (its two Flags rows stay below the crop). `mise run deploy prod` prints its four real `mise ERROR` lines; `prod` shakes (not on the impact frame); a dissonant stab; a thread ties `prod` to the card's `choices` line. The terminal holds through both bar lines, its text dissolving into daemons' prompt at the end.
- **`daemons`** (4:30.0–4:48.0, 18 beats; captures: C14). `cd ../shop`. The shop card: [tools] (pitchfork) folded, `[daemons] postgres = "18"`, [daemons_settings] folded, `[tasks.db] daemons = "postgres"` with `run = 'psql -Atc "show server_version"'`. No [settings] table, no badge. `mise run db` plays at real time: dimmed Postgres log lines under pitchfork's own readiness spinner (`waiting for command … pg_isready …`), then `✔ [shop/postgres] started on port 5432`, with caption 2; [daemons] lights sage; `[db] $ psql -Atc "show server_version"` prints the real server version, and a thread ties it to `postgres = "18"`. Postgres's log lines and their wrapped rows are dimmed to texture. The card and the terminal, on the server's version, hold into the Dotfiles ticket.

### Act V · Dotfiles (4:48.0–5:28.0)

- **`dotfiles`** (4:48.0–4:50.0, 2 beats; captures: none). Ticket: "Dotfiles" with `[dotfiles]`, terracotta. The shop card, held from daemons, slides off and daemons' terminal falls away under the printing paper; the `~/.zshrc` file card slides in (prompt, compinit, `eval "$(mise activate zsh)"`); an empty checkpoint rail sits under it; the terminal comes up at `~ $` for track.
- **`track`** (4:50.0–5:28.0, 38 beats; captures: C15, C16). `~ $ mise dot track ~/.zshrc`, its confirm prompt answered on camera; three lines print; the rail gains checkpoint 1 (baseline). Footnote: "Autosave needs the history watcher, `mise dot watch`." `echo "alias ll='ls -lah'" >> ~/.zshrc`; the alias lands in the card and the rail gains checkpoint 2 as the watcher saves the edit, labelled with the real trigger; `mise dot history --path ~/.zshrc` (dimmed) then lists baseline and the watcher's save, confirming it. `mise dot rollback ~/.zshrc` (plan table dimmed, prompt answered) -> `mise history: rolled back ~/.zshrc to checkpoint 1`; the alias leaves the card, and the rail gains the checkpoint the rollback records (its id and trigger from C15's history listing after it, off camera), restored from the baseline; the pointer moves to it. Footnote: "Share the history with `mise dot origin set`." Then the hold under caption 2, the tape-rewind cue on the rollback's checkpoint; the card, the rail (its three checkpoints) and the terminal hold into the Everywhere ticket.

### Act VI · Everywhere (5:28.0–5:52.0)

- **`machines`** (5:28.0–5:30.0, 2 beats; captures: none). Ticket: "Everywhere" with `mise.lock`, terracotta. Track's `~/.zshrc` card, rail and terminal leave under it as it prints; as it lifts off the rail, the api card returns and the terminal comes up at `~/work/api $` for lock.
- **`lock`** (5:30.0–5:52.0, 22 beats; captures: C17). The card held from the ticket gives way to the ledger. `~/work/api $ mise lock` prints the platforms it targets and the tools it will process, and the shot holds there, dimmed, before its progress rows (they carry byte counts and elapsed times at real speed and end on checked rows and totals). The ledger unfolds only node: `version = "{node.lts_version}"`, `specifiers = ["{node.lts_major}"]`, one `sha256:` row per platform, clipped. npm:prettier (its aube sidecar) and hk (its platforms and signer) fold to one line each. A thread runs from `node = "{node.lts_major}"` to the version row. A provenance badge sits under each entry whose lock records provenance. The ledger shrinks to a chip and drops into the fixture `.github/workflows/ci.yml` panel (`jdx/mise-action@v4`, `install_args: --locked`, `run: mise run ci`), which pushes in slowly; `install_args: --locked` lights, then `run: mise run ci`. No log. Ends on the reel's one whip.

### Act VII · A new machine (5:52.0–6:54.0)

- **`new`** (5:52.0–5:54.0, 2 beats; captures: none). The whip lands on an empty terminal at `~ $`, badged "fresh machine · mise installed"; the ticket "A new machine" with `mise bootstrap` (terracotta) comes down on the narrow rail beside it, in place over the whip (it is not part of the whipping stage), and prints. The terminal and its badge hold into bootstrap.
- **`bootstrap`** (5:54.0–6:18.0, 24 beats; captures: C18). `~ $ mise bootstrap --adopt you/setup` ("illustration" badge while you/setup is on screen), at real time across bars 1-3. Real output in order: repository line; two prose paragraphs, dimmed while the tiles take focus; plan table (the `~/.zshrc` card flies in off its create row); prompt answered; `Wrote 2 file(s) from ...` (Dotfiles and Config tiles tick); `mise bootstrap: user services` and its prompt answered (Watcher tile); `mise bootstrap: dotfiles` / `mise files: all files are applied`; `mise bootstrap: tools`. The shot holds there, before the tools step's first progress row (a transfer rate shows within 0.2 s), and the Tools tile shows the step started (a ring, not a tick). Without systemd: no Watcher tile and caption 2 becomes "Then it installs your global tools."
- **`breath`** (6:18.0–6:24.0, 6 beats; captures: C19). Lights dim over bootstrap's last frame, held in the same terminal. A ⌃L keycap as a real Ctrl-L clears that same, not activated, shell (C19 starts on the screen it cleared). The api card rests unlit, `[tools]` and `[env]` open. The lights come back up. Near silence.
- **`clone`** (6:24.0–6:54.0, 30 beats; captures: C19). `~ $ git clone https://github.com/you/api && cd api && mise run ci` typed at pane size ("illustration" on you/api); only `mise run ci` lifts into big type, gone before git's first line; git's real lines print, dimmed to texture. Install rows for jq, npm:prettier and hk move at 3× under the time-lapse badge; npm:prettier and hk flash pink; [tools] lights pink; cut while rows still run; the window holds its command, dimmed, until the lanes take its column, and does not return. Lanes in capture order; [tasks] lights sage; the card folds `[tools]` and `[env]` away to its three headers. Then `[ci] api ready: node v{node.lts_version}, APP_ENV=api` lands as big type in the capture's colours, two lines at 88 px, centred; a bell; [env] lights gold, then caption 2; the line's pieces fly home and ignite their headers. Tables stay folded.

### Act VIII · Install mise (6:54.0–7:16.0)

- **`morph`** (6:54.0–7:02.0, 8 beats; captures: none). b1: the card held from the climax; its three lit headers lift off and become the toque's three lobes as the score's F lands on the downbeat; the card body folds away. b2: the toque settles on the chef, which takes its end-card place; nothing else moves. From beat 6 (7:00.0) the score holds B♭ minor on the organ, no drums. The recording enters at 7:01.48 on the pickup "It's": the held chord crossfades out at equal power under its 20 ms fade-in, and the score is silent from 7:01.50 (see the end section's keyed times). The chef holds into the end card.
- **`end`** (7:02.0–7:16.0, 14 beats; captures: none). No captions. Keyed to the recording's measured onsets, not the grid: "mise-en-place" writes on with the sung "mise", "en", "place"; the tagline "Dev tools, env vars, and tasks in one CLI" fades in on "dev"; `$ curl https://mise.run | sh` types out across "precise and operational"; the 40 px platform line "macOS & Linux · Windows: `winget install jdx.mise`" fades in after the voice; "mise.jdx.dev" lands on the first button hit; the glint crosses the hat on the second (no eye motion). The chef holds from the morph, and the stack sits high enough that mise.jdx.dev stays clear of a player's controls. The card holds, silent, after the ring-out.

## End card timing

The recording is `score/mise-en-place.mp3` (the original theme song, kept here for the reel; the site now plays **mise run**), cut from the decoded audio (t = 0 is the first decoded sample). Segment A is chorus 1's line, 71.78–79.245 s (a 20 ms fade-in, and a 0.1 s fade-out on the decay of the extra bar's beat-3 hit, 5 ms before the band's pickup into verse 3); segment B is the button and ring-out, 209.08–212.9 s (a 10 ms fade-in). Both are mixed at a fixed −2.0 dB after the master chain (`score/song.ts`). The score's held B♭ minor sounds whole until segment A enters, crossfades out at equal power under its fade-in, and is silent from 6:51.50. Draw these events at the measured onsets that `song.test` reports; the values below are the plan's measurements (about ±0.1 s for words).

| time    | +E0       | event                                                                                                              |
| ------- | --------- | ------------------------------------------------------------------------------------------------------------------ |
| 7:01.48 | -0.520 s  | recording in (segment A 71.78 s, 20 ms fade-in); the score's held B♭m crossfades out under it, silent from 7:01.50 |
| 7:01.57 | -0.430 s  | "It's" (pickup)                                                                                                    |
| 7:02.00 | +0.000 s  | "mise" = end-card downbeat                                                                                         |
| 7:02.36 | +0.360 s  | "en"                                                                                                               |
| 7:02.68 | +0.680 s  | "place"                                                                                                            |
| 7:03.48 | +1.480 s  | "dev"                                                                                                              |
| 7:04.78 | +2.780 s  | "precise"                                                                                                          |
| 7:05.80 | +3.800 s  | "operational"                                                                                                      |
| 7:08.25 | +6.250 s  | voice below -50.7 dBFS                                                                                             |
| 7:08.94 | +6.945 s  | segment A out (79.245 s, 0.1 s fade)                                                                               |
| 7:09.48 | +7.480 s  | segment B in (209.08 s, 10 ms fade-in)                                                                             |
| 7:09.52 | +7.515 s  | button hit 1 (209.115 s)                                                                                           |
| 7:10.26 | +8.264 s  | button hit 2 (209.864 s)                                                                                           |
| 7:13.20 | +11.200 s | ring-out at -62 dBFS (212.8 s)                                                                                     |

## Captures

Recorded in the capture container: zsh, `PROMPT='%~ $ '`, 80 columns, `HOME=/home/you`, a published mise release, `MISE_TASK_TIMINGS=0`, `MISE_DISABLE_UPDATE_WARNING=1` and the release-age cutoff (`MISE_MINIMUM_RELEASE_AGE`, the job start minus 24 h). `MISE_EXPERIMENTAL=1` is set off camera in every take's environment, never typed and never shown; of what the reel shows, only daemons need it. `{L}` and `{O}` are the resolved majors.

| id  | on camera                                                                                                                                                                                  | used by              |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------- |
| C1  | `mise` (rows 1–7 of an 80×200 help screen)                                                                                                                                                 | `open`               |
| C2  | `cd api`, `node --version`, `echo APP_ENV=$APP_ENV`, `mise run ci`                                                                                                                         | `pitch`              |
| C3  | `jq .status resp.json`, `mise use jq`, ⌃L, `jq .status resp.json`                                                                                                                          | `use`                |
| C4  | none: the registry names                                                                                                                                                                   | `registry`           |
| C5  | `mise use npm:prettier`, ⌃L, `prettier --check README.md .github`                                                                                                                          | `backends`           |
| C6  | `mise use github:cli/cli`, `gh --version` in `~/work/tools`                                                                                                                                | `backends`           |
| C7  | `cd ../dashboard`, `node --version`, `cd ../api`, `node --version` (dashboard made by `mise use node@{O}`)                                                                                 | `switch`             |
| C8  | `hk check --j`⇥, `mise use hk@2.2.0`, ⇥, `mise skills sync`; off camera, the packslip statement mise saved for each hk version (`offcam/packslip-hk-<v>.json`)                             | `packslip`           |
| C9  | `cd ..`, `echo APP_ENV=$APP_ENV`, `cd api`, `echo PORT=$PORT`                                                                                                                              | `vars`               |
| C10 | `mise run deploy staging`                                                                                                                                                                  | `redact`             |
| C11 | `mise run ci`                                                                                                                                                                              | `depends`            |
| C12 | `mise run ci` with `sources`/`outputs`                                                                                                                                                     | `skip`               |
| C13 | `mise run deploy --help`, `mise run deploy prod`                                                                                                                                           | `args`               |
| C14 | `cd ../shop`, `mise run db`                                                                                                                                                                | `daemons`            |
| C15 | `mise dot track ~/.zshrc`, `echo … >> ~/.zshrc`, `mise dot history --path ~/.zshrc`, `mise dot rollback ~/.zshrc`; off camera, the history after the rollback (`offcam/history-after.txt`) | `track`              |
| C16 | off camera: `mise dot track ~/.config/mise/config.toml`, `mise dot origin set …`                                                                                                           | `track` (state only) |
| C17 | `mise lock`                                                                                                                                                                                | `lock`               |
| C18 | `mise bootstrap --adopt you/setup`                                                                                                                                                         | `bootstrap`          |
| C19 | ⌃L, `git clone https://github.com/you/api && cd api && mise run ci` (not activated)                                                                                                        | `breath`, `clone`    |

## Palette

| token              | value                 | meaning                                            |
| ------------------ | --------------------- | -------------------------------------------------- |
| stage              | `#171417`             | background                                         |
| night              | `#110e11`             | deepest background                                 |
| panel              | `#211d21`             | cards and panels                                   |
| paper              | `#f4eee3`             | captions, badges, the pass tick                    |
| paper dim          | `#c2b6a4`             | secondary caption text                             |
| tools pink         | `#ed9fbc`             | `[tools]`, dev tools, backends, packslip, Versions |
| env gold           | `#e1bd87`             | `[env]`, Environments                              |
| tasks sage         | `#adce9c`             | `[tasks]`, Tasks, `[daemons]`                      |
| machine terracotta | `#eaa58e`             | Dotfiles, Everywhere, A new machine                |
| terminal           | `#251d26` / `#f3eaf0` | pane background / text                             |
| lemon              | `#f0d27a`             | ANSI yellow inside panes (never gold)              |

- A pillar colour only ever means its pillar. The pitch teaches the palette by lighting `[tools]`, `[env]` and `[tasks]`.
- ANSI colours stay true to the capture, inside panes only. Red means an error, in a terminal only.
- Sage never means "passed". A pass is a paper tick plus the bell.

## Type

- **Captions:** Space Grotesk 88/56/40 px.
- **Tickets and act titles:** Cormorant Garamond italic.
- **Terminals:** JetBrains Mono, with vector fallbacks for the spinner, `✓` and the block glyphs.
- **Keycaps** (Tab, ⌃L) are drawn outside the pane, never in the terminal font.
- Roc Grotesk is not used.

## Badges and footnotes

- **Badges:** "time-lapse", "illustration" (on `you/api` and `you/setup`), "Order from one real run. Not to scale.", "fresh machine · mise installed", and the slip's "Excerpt of hk's packslip · real keys, values abridged". Each is a paper outline at 30 px (the slip's and the ledger's 20-22 px), never a pillar colour. A terminal's badges ride its chrome bar, right to left: "time-lapse", "illustration", "fresh machine · mise installed". There is no experimental badge.
- **Footnotes:** at most one per frame, at 40 px, seven in total: two in `backends`, one in `packslip`, one in `redact`, two in `track`, and the end card's platform line.
- **Kitchen devices:** tickets on the rail and the chef, nothing else. The chef has no eye motion.

## Decisions and references

Comments across the showreel cite the planning documents this storyboard was written from, which are not in the repository. What each settled is recorded here, so a citation always resolves:

- **plan v3 §2** (length and acts): 27 sections in 9 acts, 258 beats at 75 BPM, 206.4 s; the short cut; the four version sets the reel must render with. Retimed on 2026-09-28 to 60 BPM and 436 beats (7:16.0) by the pacing rules above, the acts and chapters unchanged.
- **plan v3 §3** (beat sheet): the sections and captions above, act by act.
- **plan v3 §5** (music): a score composed in code, 75 BPM in F minor, quoting the chorus hook with no voice; only `cd ../dashboard` modulates (to D♭), every other `cd` ticks; the end card, and only the end card, plays jdx's sung line from the recording; M2 is never synthesized. Its sync points are ART.md §13.
- **plan v3 §7** (capture list): the takes C1–C19 and how each is recorded (`showreel-capture/README.md`), and bootstrap's plain-machine variant.
- **plan v3 §8** (honesty ledger): what the tests enforce (`forbidden.test`, `timing.test`, `captions.test`, `storyboard.test`, `handoff-frames.test`).
- **plan v3's timing checker** (`board5.py`): the script that wrote `sections.json`; its rules (the anchors, the digits rule, the forbidden words) live in `storyboard.test` and `captions.test`. The retime's planner (`build/retime/plan.ts`, which builds every section's schedule with `kit/pace.ts` `Pace` on the capture set, and `write.py`) rewrote the beats, the caption anchors and the event plans on 2026-09-28; `pace.test` holds them to the rules.
- **The song report:** the measurements of `mise-en-place.mp3` the score's splices use; the numbers are in `score/song.ts`.
- **jdx's decisions (2026-09-27 and 28):**
  1. Node's numbers come from the capture run (the `lts` alias's major and the other major), never from a scene or caption.
  2. The docs build re-records and re-renders when those change, keeps the last good render on any failure, and opens no refresh PRs.
  3. The npm example is `npm:prettier`.
  4. Experimental status is never shown, as trust is not.
  5. The end card plays jdx's sung line; the closing act is as long as it needs.
  6. `og:video` is the 60 fps reel, and install progress is shown only while it moves, under the time-lapse badge, never with a ✓ or a total.
  7. (jdx's rule 7) No timing text: an elapsed time or a rate is on screen only while an install replays under the time-lapse badge, and never unchanged for more than 0.1 s.
- **Rests and captions, decided in the review round (2026-09-28):** a bar line never rests on an empty, untitled window; the section before a ticket never clears its own stage (backends holds gh's pane, daemons its terminal, redact its footnote into their tickets); a card changing between two sections carries across the bar line and changes inside the incoming one (skip's card into args); captions hold on as the Captions rule above says.
