// daemons (plan v3 §3, Act IV; ART.md §12.14), one thing at a time
// (STORYBOARD.md "Pacing", kit/pace.ts): in the window args left, on the
// take's own prompt, `cd ../shop` types at TYPE_RATE (C14). Then the
// shop's card slides in: [daemons] and [tasks.db] open, the rest folded,
// no [settings] table and no badge; the task's `daemons = "postgres"` is
// marked and a thread runs up the card's left margin to the daemon it
// names, `[daemons] postgres = "18"`, and caption 1 says the task declares
// the daemon it needs. Then `mise run db` types and plays at real time:
// pitchfork's own readiness spinner (`waiting for command … pg_isready …`)
// over Postgres's log lines, dimmed as texture, then `✔ [shop/postgres]
// started on port 5432`, `[db] $ psql -Atc "show server_version"` and the
// server's real version, held for its read while [daemons] lights sage and
// its pilot light comes on; the version lifts out of its row as a 40 px
// value chip and lands beside the major the card asks for (`postgres =
// "18"` ... `18.6`, readable on a phone), its thread drawing with it, and
// caption 2 says what happened. The chip and thread go, and the card, lit,
// and the terminal on the server's version (kit/rest.ts HOLDS.C14) hold
// into the Dotfiles ticket, which takes them down. Whatever is not the
// focus dims (PACE.focusDim), and all of it is back up for the rest.

import { sec } from "../bible";
import {
  type Capture,
  capture,
  fileOf,
  firstWith,
  type ReelData,
  reelData,
  stepOf,
} from "../captures";
import { bump, span } from "../kit/motion";
import {
  beatOf,
  CARD,
  card,
  type CardRows,
  cut,
  findRow,
  greyScene,
  keep,
  LEFT,
  type Play,
  shot,
  step,
  threadAlong,
} from "../kit/grey";
import { type FocusPlan, focusOf, Pace, restOut, unfocus } from "../kit/pace";
import { drawPaneWindow, HOLDS, LOG_DIM } from "../kit/rest";
import { CARD_ART, DUR, MOTION, THREAD, TYPE } from "../kit/style";
import type { SceneCues } from "../score/cues";
import type { SceneEvents } from "../storyboard";
import { chipBox, liftValue, paneLit, VALUE_LANDS } from "./g4-tasks/common";

/**
 * The card's entrance, beats after it starts: it slides in; the task's
 * `daemons = "postgres"` is marked; the in-card thread draws up to
 * `postgres = "18"`, which takes a seat mark as its head arrives.
 */
const CARD_IN = {
  mark: DUR.enter + DUR.tick,
  thread: DUR.enter + DUR.tick + DUR.tick,
  dur: DUR.enter + DUR.tick + DUR.tick + DUR.thread,
} as const;
/** The version's chip lifts and lands beside `postgres = …`, its thread drawing with it, and the row takes its mark. */
const TIE_DUR = VALUE_LANDS + DUR.tick;
/** The chip sits this far right of the row's value. */
const CHIP_GAP = 24;

/** The take, when the set has it (a set of versions alone has no captures). */
const take = (d: ReelData | null): Capture | null =>
  d?.captures ? capture(d, "C14") : null;

interface Beats {
  card: number;
  db: number;
  run: number;
  lit: number;
  tie: number;
  tieOut: number;
}

/**
 * The section's schedule (kit/pace.ts), in the order STORYBOARD.md's event
 * plan gives, one focal motion at a time, each command typed at TYPE_RATE
 * and its output held for its read.
 */
function plan(d: ReelData | null, c: Capture) {
  const p = new Pace("daemons", { d, from: 0 });
  const cd = p.term("cd", (at) => [step(c, "cd", at)], { lines: 0 });
  const crd = p.move("card", CARD_IN.dur);
  p.caption(0);
  const db = p.term(
    "db",
    (at) => [step(c, "db", at, { until: HOLDS.C14!(c) })],
    { lines: 3 },
  );
  const lit = p.move("lit", DUR.light);
  const tie = p.move("tie", TIE_DUR);
  // The chip and its thread hold while caption 2 is read, for as long as
  // the section's rest allows (the ticket takes nothing down with it).
  const c2 = p.caption(1);
  const latest = sec("daemons").beats - restOut() - DUR.threadFade;
  const out = p.move("tieOut", DUR.threadFade, {
    after: Math.min(c2.out, latest),
  });
  const plays: Play[] = [
    cut(0, stepOf(c, "cd").type),
    ...cd.plays,
    ...db.plays,
  ];
  const focus: FocusPlan = [
    [crd.at, "card"],
    [db.at, "term"],
    [lit.at, "*"],
  ];
  const beats: Beats = {
    card: crd.at,
    db: db.at,
    run: db.enter,
    lit: lit.at,
    tie: tie.at,
    tieOut: out.at,
  };
  return { pace: p, plays, focus, events: p.events(), beats };
}

type Plan = ReturnType<typeof plan>;
const PLANS = new WeakMap<Capture, Plan>();
/** The schedule for a take (a pure function of it and the facts' captions); null without it. */
export function schedule(d: ReelData | null): Plan | null {
  const c = take(d);
  if (!c) return null;
  let s = PLANS.get(c);
  if (!s) PLANS.set(c, (s = plan(d, c)));
  return s;
}

/** Without the take: the storyboard's event plan (sections.json). */
const FALLBACK: Beats = {
  card: 2.28,
  db: 6,
  run: 7.44,
  lit: 8.92,
  tie: 9.92,
  tieOut: 16,
};

const startedAt = (c: Capture): number | null =>
  firstWith(c, /started on port/, stepOf(c, "db").enter);

/** When the take's cwd changes to the shop: the prompt `cd ../shop` leaves. */
const cdAt = (c: Capture): number =>
  firstWith(c, /^~\/work\/shop \$/, stepOf(c, "cd").enter) ??
  stepOf(c, "cd").end;

export const cues: SceneCues<"daemons"> = (facts) => {
  const d = reelData(facts);
  const s = schedule(d);
  if (!s) return {};
  const c = take(d)!;
  const started = startedAt(c);
  return {
    cd: beatOf(s.plays, cdAt(c)),
    run: s.beats.run,
    started: started === null ? null : beatOf(s.plays, started),
    // The click as the version's chip lands beside `postgres = …`.
    tie: s.beats.tie + VALUE_LANDS,
  };
};

export const scene = greyScene(
  "daemons",
  (g) => {
    const { ctx, b } = g;
    const sc = schedule(g.d);
    const B = sc?.beats ?? FALLBACK;
    const focus: FocusPlan = sc?.focus ?? [];
    // The card slides in from the right once the shell is in the shop.
    const slide = span(b, B.card, DUR.enter, "arrive");
    // The task's need, marked, and tied to the daemon it names; both go as
    // `mise run db` starts to type.
    const needOut = 1 - span(b, B.db, DUR.threadFade, "glide");
    const mark = span(b, B.card + CARD_IN.mark, DUR.tick, "arrive") * needOut;
    const need = span(b, B.card + CARD_IN.thread, DUR.thread, "arrive");
    // `postgres = …` takes a seat mark as the in-card thread's head reaches it.
    const named =
      span(
        b,
        B.card + CARD_IN.thread + DUR.thread - DUR.flick,
        DUR.tick,
        "arrive",
      ) * needOut;
    // The version's chip lands VALUE_LANDS after it lifts; its thread draws with it.
    const tieLands = B.tie + VALUE_LANDS;
    const tie = span(b, B.tie + DUR.half, VALUE_LANDS - DUR.half, "arrive");
    const tieA = 1 - span(b, B.tieOut, DUR.threadFade, "glide");
    // `postgres = …` takes a seat mark as the chip lands beside it.
    const landed = span(b, tieLands, DUR.tick, "arrive") * tieA;
    const lit = B.lit;
    const rows: CardRows = keep(g, () => {
      ctx.save();
      ctx.translate(MOTION.slideIn * (1 - slide), 0);
      const r = card(ctx, CARD, {
        title: "~/work/shop/mise.toml",
        text: fileOf(g.d, "C14", "work/shop/mise.toml"),
        from: "C14",
        open: ["[daemons]", "[tasks.db]"],
        seat: [
          { re: /^daemons = /, a: mark },
          { re: /^postgres = /, a: Math.max(landed, named) },
        ],
        lit: { "[daemons]": span(b, lit, DUR.light, "arrive") },
        ignite: { "[daemons]": bump(b, lit, DUR.ignite) },
        alpha: slide * focusOf(focus, "card", b),
      });
      ctx.restore();
      return r;
    });
    // The task's need, tied to the daemon it names: down the card's left
    // margin (ART.md §9, inside a card), anchor dots on both rows.
    const src = rows.find((r) => /^daemons = /.test(r.text));
    const dst = rows.find((r) => /^postgres = /.test(r.text));
    if (src && dst && need > 0 && needOut > 0) {
      const mid = (r: (typeof rows)[number]) =>
        (r.top ?? r.y) + (r.h ?? CARD_ART.annotation.h) / 2;
      const x = CARD.x + CARD_ART.anchorX;
      threadAlong(
        ctx,
        [
          { x, y: mid(src) },
          { x, y: mid(dst) },
        ],
        need,
        needOut,
      );
    }
    // The terminal: the window from args, on the prompt it held, the take
    // in it; it stays up through the bar line (the daemons|dotfiles rest),
    // and the ticket takes it down with the card.
    const termDim = unfocus(focus, "term", b);
    keep(g, () => drawPaneWindow(ctx, LEFT, termDim));
    ctx.save();
    // From the prompt the bar line holds to the one it ends on, kept:
    // whole on both bar lines. Postgres's log lines and every wrapped row
    // are texture (kit/rest.ts LOG_DIM, as the rest dims them).
    const s = shot(g, "C14", LEFT, -1, Infinity, () => sc?.plays ?? [], {
      bare: true,
      keep: true,
      lineAlpha: LOG_DIM,
      focus: () => termDim,
    });
    // The server's version, tied to the major the card asks for: its
    // thread out of its row's end, along the row (nothing else prints on
    // it), up the gutter and into the card's left edge, drawn on as its
    // chip lands beside the row's value.
    const v = s && findRow(s.screen, /^\d+(\.\d+)+$/);
    const to = rows.find((r) => /^postgres = /.test(r.text));
    if (s && v && to) {
      const y1 = (to.top ?? to.y) + (to.h ?? CARD_ART.annotation.h) / 2;
      if (tie > 0 && tieA > 0) {
        const cell = s.layout.cell(v.line, Array.from(v.text).length);
        const y0 = cell.y + cell.h / 2;
        threadAlong(
          ctx,
          [
            { x: cell.x + THREAD.exit, y: y0 },
            { x: THREAD.gutterX, y: y0 },
            { x: THREAD.gutterX, y: y1 },
            { x: CARD.x + CARD_ART.anchorX, y: y1 },
          ],
          tie,
          tieA,
        );
      }
      // The chip centred on the row, just right of its value.
      const { h } = chipBox(v.text, TYPE.chip.lift);
      const run0 = s.screen.lines[v.line][0];
      liftValue(
        ctx,
        {
          text: v.text,
          color: run0?.color,
          from: {
            x: s.layout.cell(v.line, 0).x,
            y: s.layout.baseline(v.line),
            size: s.pane.size,
          },
          to: { x: to.x + (to.w ?? 0) + CHIP_GAP, y: y1 - h / 2 + 0.72 * h },
          at: B.tie,
          out: B.tieOut,
        },
        b,
      );
    }
    ctx.restore();
  },
  {
    lit: () => paneLit(LEFT, 1),
    events: (d): SceneEvents | null => schedule(d)?.events ?? null,
  },
);
