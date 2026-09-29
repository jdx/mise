// The kit's staging options (kit/grey.ts, kit/rest.ts, kit/term.ts,
// kit/parts.ts, kit/ticket.ts, captures.ts), as numbers: panes and cards
// sized to their content, a terminal that never scrolls back up, soft
// wraps joined to their lines, the illustration badge held from the first
// typed `you/` to the clear (and never on the fixture user's home), the
// cwd's cross-fade on the section's beats, an install's held tail, the
// camera's push-in, an exit that ends without a pop, and a ticket opaque
// while any of it is on the frame.
//
// The take-based checks need the capture set, as forbidden.test.ts does.

import assert from "node:assert/strict";
import { test } from "node:test";
import { BEAT } from "../bible";
import {
  type Capture,
  type CaptureId,
  capture,
  fileOf,
  type ReelData,
  reelData,
  rowText,
  screenAt,
  stepOf,
} from "../captures";
import { cut, cwdHop, enteredAt, play, pushAt } from "../kit/grey";
import { exit } from "../kit/motion";
import { cardFit } from "../kit/parts";
import {
  fittedPane,
  namesPlaceholder,
  screenBadges,
  shownRows,
  takeScroll,
} from "../kit/rest";
import { DUR, LAYOUT, PANES } from "../kit/style";
import { FIT_MIN, fitPane, lineText, paneHeight } from "../kit/term";
import { ticketPlace, ticketPose } from "../kit/ticket";
import { loadFacts } from "../load";
import { REPO } from "./repo";

const { facts, report } = loadFacts(REPO);
const data: ReelData | null = reelData(facts as never);

/** Take `id`, or a skip (a failure where captures are required). */
function take(t: { skip: (m: string) => void }, id: CaptureId): Capture | null {
  const c = capture(data, id);
  if (c) return c;
  if (process.env.SHOWREEL_REQUIRE_CAPTURES) assert.fail(report.join("; "));
  t.skip(`no capture set: ${report.join("; ")}`);
  return null;
}

test("a ticket is opaque while any of its paper is on the frame, and gone once it has left", () => {
  for (const narrow of [false, true]) {
    const { paper } = ticketPlace(narrow);
    for (let b = 0.001; b < 2; b += 0.001) {
      const p = ticketPose(b, { narrow });
      if (!p) continue;
      // Its lowest point: the foot, and the corner the lift's turn drops.
      const foot =
        paper.y +
        paper.h +
        p.dy +
        (paper.w / 2) * Math.abs(Math.sin((p.rot * Math.PI) / 180));
      if (foot > 0)
        assert.equal(p.alpha, 1, `translucent on the frame at ${b.toFixed(3)}`);
    }
  }
});

test("an exit fades without a pop: no frame takes more than a tenth of its opacity", () => {
  // At 60 fps an exit over DUR.exit (3/8 beat) is 18 frames.
  const step = 1 / (60 * BEAT);
  let prev = 1;
  for (let b = 1; b <= 1 + DUR.exit + step; b += step) {
    const a = exit(b, 1).alpha;
    assert.ok(prev - a <= 0.1, `${(prev - a).toFixed(3)} gone at ${b}`);
    prev = a;
  }
  assert.equal(exit(1 + DUR.exit, 1).alpha, 0);
});

test("a pane fits its rows: the preset's corner and columns, FIT_MIN rows at least, never taller", () => {
  const p = PANES.side;
  const three = fitPane(p, 3);
  assert.equal(three.rows, FIT_MIN);
  assert.equal(three.h, paneHeight(p, FIT_MIN));
  assert.ok(three.h < p.h);
  assert.deepEqual(
    [three.x, three.y, three.w, three.x0],
    [p.x, p.y, p.w, p.x0],
  );
  assert.equal(fitPane(p, 99).rows, p.rows);
  assert.ok(fitPane(p, 99).h <= p.h);
  assert.equal(fitPane(p, 9).rows, 9);
});

test("a card fits its rows: as tall as they are, shorter as a table folds, never past its rect", () => {
  const text =
    fileOf(data, "C2", "work/api/mise.toml") ??
    '[tools]\nnode = "24"\n\n[env]\nAPP_ENV = "api"\n\n[tasks.ci]\nrun = "x"\n';
  const r = LAYOUT.card;
  const open = cardFit(r, {
    title: "t",
    text,
    from: "C2",
    open: ["[tools]", "[env]"],
  });
  const folded = cardFit(r, { title: "t", text, from: "C2", open: [] });
  assert.ok(open.h <= r.h && folded.h <= r.h);
  assert.ok(folded.h < open.h, `folded ${folded.h}, open ${open.h}`);
  assert.deepEqual([open.x, open.y, open.w], [r.x, r.y, r.w]);
});

test("the push-in is exactly 1 outside its span, and holds its scale inside it", () => {
  const p = { from: 2, to: 3, scale: 1.3, focus: { x: 960, y: 400 }, out: 6 };
  assert.equal(pushAt(p, 0), 1);
  assert.equal(pushAt(p, 2), 1);
  assert.ok(Math.abs(pushAt(p, 4) - 1.3) < 1e-9);
  assert.equal(pushAt(p, 6 + DUR.move), 1);
  assert.equal(pushAt({ ...p, out: undefined }, 99), 1.3);
});

test("a take's screen never scrolls back up in a pane: track's rollback prompt, bootstrap's confirms", (t) => {
  for (const id of ["C15", "C18"] as const) {
    const c = take(t, id);
    if (!c) return;
    const clears = c.keys.filter(([, k]) => k === "\f").map(([at]) => at);
    let last = -1;
    let lastT = 0;
    for (const f of c.frames) {
      const first = takeScroll(c, f.t, PANES.side.rows);
      if (first < last)
        assert.ok(
          clears.some((k) => k > lastT && k <= f.t) ||
            screenAt(c, f.t).lines.length - 1 < last,
          `${id} scrolled back from ${last} to ${first} at ${f.t}`,
        );
      last = first;
      lastT = f.t;
    }
  }
});

test("a fitted pane grows with its take and never shrinks on its own", (t) => {
  const c = take(t, "C13");
  if (!c) return;
  let h = 0;
  for (const f of c.frames) {
    const p = fittedPane(c, f.t, PANES.side);
    assert.ok(p.h >= h - 1e-9, `shrank at ${f.t}`);
    assert.ok(p.h <= PANES.side.h + 1e-9);
    assert.ok(p.rows >= FIT_MIN && p.rows <= PANES.side.rows);
    h = p.h;
  }
});

test("joinWraps crops a soft wrap's rest with its line, and nothing else", (t) => {
  const c = take(t, "C15");
  if (!c) return;
  const end = c.frames[c.frames.length - 1].t;
  const all = screenAt(c, end).lines.map(lineText);
  const joined = screenAt(c, end, { joinWraps: true }).lines.map(lineText);
  const rest = all.filter(
    (_, i) => i > 0 && Array.from(all[i - 1]).length >= c.width,
  );
  assert.ok(rest.includes("g.toml)"), "track's wrapped row is where it was");
  assert.equal(joined.length, all.length - rest.length);
  assert.ok(!joined.includes("g.toml)"));
  // A RegExp joins only after the rows it matches.
  const none = screenAt(c, end, { joinWraps: /^no such row$/ });
  assert.equal(none.lines.length, all.length);
});

test("the illustration badge holds from the first typed `you/` to the clear, and never for /home/you", (t) => {
  for (const id of ["C18", "C19"] as const) {
    const c = take(t, id);
    if (!c) return;
    const typed = c.frames.find((f) =>
      f.rows.some((r) => namesPlaceholder(rowText(c, r))),
    );
    assert.ok(typed, `${id} types a placeholder`);
    const texts = screenAt(c, typed.t).lines.map(lineText);
    const b = screenBadges(c, typed.t, shownRows(texts, PANES.side));
    assert.ok(
      b.some((x) => x.text === "illustration"),
      `${id} unbadged at its first you/`,
    );
  }
  const c8 = take(t, "C8");
  if (!c8) return;
  for (const f of c8.frames) {
    const texts: string[] = screenAt(c8, f.t).lines.map(lineText);
    assert.deepEqual(
      screenBadges(c8, f.t, shownRows(texts, PANES.side)),
      [],
      `packslip badged at ${f.t}`,
    );
  }
});

test("a cwd's cross-fade runs on the section's beats: a cut past the `cd` still shows all of it", (t) => {
  const c = take(t, "C7");
  if (!c) return;
  const cd = stepOf(c, "cd-dashboard");
  // Cut from before the `cd` to well after it: in take time the hop was
  // over before the cut landed; on the beats it starts there.
  const plays = [cut(0, cd.type), cut(1, cd.end), play(2, cd.end, cd.end)];
  const at1 = cwdHop(c, cd.end, { plays, b: 1 });
  assert.ok(at1 && at1.k < 0.01, "the hop starts on the cut's beat");
  const mid = cwdHop(c, cd.end, { plays, b: 1 + DUR.tick / 2 });
  assert.ok(mid && Math.abs(mid.k - 0.5) < 1e-9);
  assert.equal(cwdHop(c, cd.end, { plays, b: 1 + DUR.tick }), null);
  // A shot that opens after the `cd` never hops.
  assert.equal(cwdHop(c, cd.end, { plays: [cut(0, cd.end)], b: 0 }), null);
});

test("an install's held frame is the command just entered, with no output under it", (t) => {
  const c = take(t, "C3");
  if (!c) return;
  const s = stepOf(c, "use");
  const at = enteredAt(c, s.enter + 0.02);
  const lines = screenAt(c, at)
    .lines.map(lineText)
    .filter((l) => l.trim());
  assert.match(lines.at(-1) ?? "", /\$ mise use jq$/);
  assert.ok(at <= s.enter + 0.02);
});
