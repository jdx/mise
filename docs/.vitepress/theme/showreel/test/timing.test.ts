// No timing text, frame by frame (plan v3 §8 timing.test, jdx's rule 7 and
// decision 6). The whole reel is rendered at 60 fps on a recording canvas
// in node, and every terminal line each frame draws is scanned for an
// elapsed time (`\d+(\.\d+)?\s?(ms|s)`) or a rate (`… MB/s`); timestamps and
// dates carry neither, so Postgres's log lines pass. A frame may draw one
// only while an install replays faster than real time, under the
// time-lapse badge, and no row may hold one unchanged for more than 0.1 s:
// install rows appear only while they move. No progress row is ever drawn
// with a ✓ or ✔ (a finished install, or `mise lock`'s checked rows). And a
// frame that draws a placeholder repository (`you/api`, `you/setup`), even
// part typed, draws the "illustration" badge with it.
//
// Needs the capture set, as forbidden.test.ts does.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReelFacts } from "../bible";
import { SECTIONS, sec } from "../bible";
import { namesPlaceholder } from "../kit/rest";
import { SPINNER } from "../kit/term";
import { loadFacts } from "../load";
import { inkFrames } from "./record";
import { REPO } from "./repo";

/** An elapsed time or a transfer rate, as mise's progress rows print them. */
export const TIMING = /\b\d+(?:\.\d+)?\s?(?:ms|s)\b|\d\s?[kKMGT]?i?B\/s\b/;
/** A progress row: mise's header, a tool's row, or `mise lock`'s. */
export const PROGRESS = /^(?:mise by @jdx|\s\S+@\S+|lock\s)/;
/** The longest a timing token may stay on screen unchanged, s. */
const HOLD = 0.1;
const FPS = 60;

test("the timing patterns catch what they name, and pass what they do not", () => {
  const bad = [
    " jq@1.8.2 installing                                         249ms  ░░░░░░░░░░ ◠",
    "mise by @jdx  █████████████████████░░░  2/3 · 1.5s",
    "lock            hk@2.2.0 linux-arm64               5 B 3s [==>               ] ◡",
    "mise by @jdx  ██░░░░░░░░░░░░░░░░░░░░░░  0/1 · 10.0 MB/s · 1.0s",
  ];
  for (const t of bad) assert.match(t, TIMING, t);
  const good = [
    "• [shop/postgres] 2026-09-27 22:01:27.059 UTC [2573] LOG:  starting PostgreSQL 1",
    "remote: Counting objects: 100% (23/23), done.",
    "     ↳ 1/1 pkgs · 2.7 MiB",
    "Wrote 2 file(s) from https://github.com/you/setup.git.",
    "→ Processing 4 tool(s): hk@2.2.0, jq@1.8.2, node@24.21.0, npm:prettier@3.9.9",
    "2   2026-09-27 22:01  edit      edited ~/.zshrc   1",
    "~/work/api $ cd ../s",
  ];
  for (const t of good) assert.doesNotMatch(t, TIMING, t);
  assert.match(" hk@2.2.0 ✓", PROGRESS);
  assert.match("lock            jq@1.8.2 windows-x64      ✔", PROGRESS);
  assert.doesNotMatch("✔ [shop/postgres] started on port 5432", PROGRESS);
  // A placeholder, whole or being typed; the fixture user's home is not one.
  for (const t of [
    "~ $ git clone https://github.com/you/a",
    "~ $ mise bootstrap --adopt you/",
    "~ $ mise bootstrap --adopt you/setup",
    "Wrote 2 file(s) from https://github.com/you/setup.git.",
  ])
    assert.ok(namesPlaceholder(t), t);
  for (const t of [
    "linked /home/you/work/api/.claude/skills/hk-debug -> /home/you/.local/share/mise",
    "/home/you/.local/share/mise/installs/hk/2.2.0/skills/hk-configure",
  ])
    assert.ok(!namesPlaceholder(t), t);
});

const { facts, report } = loadFacts(REPO);

test("at 60 fps, timing text only moves under the time-lapse badge, no progress row is checked, and placeholders are badged", (t) => {
  if (!facts) {
    if (process.env.SHOWREEL_REQUIRE_CAPTURES) assert.fail(report.join("; "));
    t.skip(`no capture set: ${report.join("; ")}`);
    return;
  }
  const sectionAt = (at: number) =>
    SECTIONS.find(({ id }) => at < sec(id).end)?.id ?? "end";
  const bad = new Set<string>();
  const flag = (at: number, why: string) => {
    if (bad.size < 40) bad.add(`${at.toFixed(3)} s (${sectionAt(at)}): ${why}`);
  };
  // Each timing-bearing row, its spinner dropped, and the frames in a row it has been up.
  let runs = new Map<string, number>();
  let timed = 0;
  const frames = inkFrames(facts as unknown as ReelFacts, FPS, (at, inked) => {
    const rows = inked.filter((x) => x.kind === "term").map((x) => x.text);
    const texts = inked.filter((x) => x.kind === "text").map((x) => x.text);
    const lapse = texts.includes("time-lapse");
    const next = new Map<string, number>();
    for (const row of rows) {
      if (PROGRESS.test(row) && /[✓✔]/.test(row))
        flag(at, `a checked progress row: ${JSON.stringify(row)}`);
      if (!TIMING.test(row)) continue;
      timed++;
      if (!lapse)
        flag(
          at,
          `timing text without the time-lapse badge: ${JSON.stringify(row)}`,
        );
      const key = Array.from(row)
        .filter((ch) => !SPINNER.includes(ch) && !"◜◠◝◞◡◟".includes(ch))
        .join("")
        .trimEnd();
      if (next.has(key)) continue;
      const n = (runs.get(key) ?? 0) + 1;
      next.set(key, n);
      if (n / FPS > HOLD + 1e-9 && n === Math.floor(HOLD * FPS) + 1)
        flag(at, `timing text held over ${HOLD} s: ${JSON.stringify(row)}`);
    }
    runs = next;
    // Whole or being typed (`…github.com/you/a`): badged from its first frame.
    if (rows.some((r) => namesPlaceholder(r)))
      if (!texts.some((x) => x.startsWith("illustration")))
        flag(at, "a placeholder repository without the illustration badge");
  });
  t.diagnostic(`${frames} frames at ${FPS} fps; ${timed} timed rows drawn`);
  assert.deepEqual([...bad], [], [...bad].join("\n"));
});
