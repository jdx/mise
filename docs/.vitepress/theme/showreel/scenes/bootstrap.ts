// bootstrap (STORYBOARD.md Act VII; ART.md §12.5; STORYBOARD.md "Pacing"): a
// fresh machine restores its dotfiles and mise config. One focal motion at a
// time, half a beat apart (kit/pace.ts Pace), whatever is not the focus
// dimmed, and never the terminal while its output is read. `~ $ mise
// bootstrap --adopt you/setup` (C18) types at TYPE_RATE; the illustration
// badge comes up with the first `you/` typed and stays for the whole shot
// (kit/rest.ts screenBadges), and the fresh machine's badge has faded from
// the chrome just before it. Its output prints at real time: the repository
// line and two paragraphs of prose, dimmed to texture (BOOTSTRAP_DIM), the
// plan table and its prompt, and the pane holds it for its read. The pane
// keeps its size and its 26 px type throughout, so every line it prints can
// be read; `~/.zshrc` lifts off its own `create` row as a card and flies out
// along it and up to the top of the empty column, and then the steps' tiles
// come in under it, pending. The prompt is answered on camera; once `Wrote 2
// file(s)` has printed, a thread runs from it along the gutter to Dotfiles
// and Config, which tick as it lands, and caption 1 follows. The services
// prompt is answered; once `applied mise-history` and `mise bootstrap:
// tools` have printed, their threads run to Watcher (which ticks; neither
// without systemd on machine 2) and to Tools, whose ring starts turning,
// never a tick: the shot holds on the frame before the tools step's first
// progress row (a transfer rate shows within 0.2 s). Caption 2 follows and
// leaves once read; then the card and the tiles leave, top first, and the
// terminal holds alone for the breath (kit/rest.ts bootstrap|breath). The
// screen scrolls as a terminal does: when a confirm's widget collapses to
// its `? Yes` line, it leaves blank rows at the bottom rather than panning
// back down (kit/rest.ts takeScroll).

import type { LitRect } from "../bible";
import {
  type Capture,
  capture,
  fileOf,
  type ReelData,
  reelData,
  rowText,
  screenAt,
} from "../captures";
import { staggerOf } from "../kit/diagrams/common";
import { bootstrapLabels, type Tile, tiles } from "../kit/diagrams/tiles";
import {
  beatOf,
  clipTime,
  cut,
  fileCard,
  greyScene,
  keep,
  LEFT,
  type Play,
  rectOf,
  rest,
  seatMark,
  step,
  threadAlong,
} from "../kit/grey";
import { exit, span } from "../kit/motion";
import { GAP, Pace, unfocus } from "../kit/pace";
import {
  BOOTSTRAP_DIM,
  BOOTSTRAP_HOLD,
  drawPaneWindow,
  FRESH,
  namesPlaceholder,
  OPENS,
} from "../kit/rest";
import { DUR, EASE, LAYOUT, THREAD } from "../kit/style";
import { lineText, termLayout, termLit } from "../kit/term";
import type { SceneCues } from "../score/cues";
import {
  focusPlan,
  inFocus,
  moveOr,
  perFacts,
  takeIn,
  termAct,
} from "./g5-dotfiles-lock/pacing";
import { drawTake, flyingFile, scrollOfTake } from "./g6-machine/shot";

/** The file card's rows: small enough for three to fit LAYOUT.bootFile (ART.md §12.5). */
const FILE_SIZE = 20;
/**
 * The card's flight: along its own row out of the pane, then up the empty
 * column to its place at the top, so it crosses none of the pane's text,
 * its chrome or its badges.
 */
const flight = (a: { x: number; y: number }, z: { x: number; y: number }) => ({
  a,
  b: z,
  c: { x: z.x, y: a.y },
});
/** The plan table's row for the dotfile the card is. */
const ZSHRC_ROW = /^~\/\.zshrc\s+create/;

/** The rows that finish or start a step, each drawing a thread to its tile, which ticks (or starts) as the thread lands. */
const STEP_ROWS: Readonly<Record<string, RegExp>> = {
  Dotfiles: /^Wrote \d+ file/,
  Config: /^Wrote \d+ file/,
  Watcher: /applied mise-history/,
  Tools: /^mise bootstrap: tools/,
};

/** A thread drawing on, then its tile ticking (DUR.tick) and flashing (DUR.half). */
const TICK = DUR.thread + DUR.half;
/** The Tools tile's ring coming on (DUR.tick) as its thread lands. */
const RING = DUR.thread + DUR.tick;

/**
 * When everything happens (kit/pace.ts): the section's focal motions one
 * at a time, from the take; without it, the storyboard's plan.
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C18");
  const labels = bootstrapLabels(d?.variant ?? "systemd");
  const watcher = labels.includes("Watcher");
  const p = new Pace("bootstrap", { d });
  const adopt = termAct(p, "adopt", c, (c, at) => [step(c, "confirm-1", at)], {
    lines: 5,
  });
  // The create row takes its seat mark a flick before the card lifts off
  // it; the card flies to the top of the column while the column is still
  // empty, and then the tiles come in under it.
  const card = moveOr(p, c, "zshrc", DUR.flick + DUR.move);
  const tileIn = moveOr(
    p,
    c,
    "tiles",
    DUR.enter + staggerOf(labels.length - 1, labels.length),
  );
  const yes1 = termAct(p, "yes1", c, (c, at) => [step(c, "confirm-2", at)], {
    lines: 3,
  });
  const tick1 = moveOr(p, c, "tick1", DUR.stagger + TICK);
  p.caption(0);
  const yes2 = termAct(
    p,
    "yes2",
    c,
    (c, at) => [step(c, "done", at, { until: BOOTSTRAP_HOLD(c) })],
    { lines: 4 },
  );
  // Watcher's thread and tick, and Tools' ring: the two rows printed
  // together, and their threads run together to the two tiles under each
  // other, one gesture.
  const tick2 = moveOr(p, c, "tick2", watcher ? Math.max(TICK, RING) : RING);
  const c2 = p.caption(1);
  // Caption 2 leaves once read; then the card and the tiles go.
  p.wait(c2.out + GAP);
  const out = moveOr(
    p,
    c,
    "tilesOut",
    DUR.stagger + staggerOf(labels.length - 1, labels.length) + DUR.exit,
  );
  const acts = [adopt, yes1, yes2];
  const plays: Play[] = c
    ? [cut(0, OPENS.C18!(c)), ...acts.flatMap((a) => a.plays)]
    : [];
  // The first frame showing a placeholder, whole or being typed: where the
  // illustration badge pops in (kit/rest.ts screenBadges).
  const named = c
    ? c.frames.find((f) => f.rows.some((r) => namesPlaceholder(rowText(c, r))))
    : undefined;
  return {
    p,
    plays,
    /** `you/` is typed: the illustration badge comes up where the fresh machine's has just faded. */
    named: named && plays.length ? beatOf(plays, named.t) : adopt.enter,
    tiles: tileIn.at,
    /** `~/.zshrc` lifts off its create row, and lands. */
    lift: card.at + DUR.flick,
    /** Each tile's thread starts: Dotfiles (Config a sixteenth later), Watcher, Tools. */
    wrote: tick1.at,
    applied: watcher ? tick2.at : null,
    tools: tick2.at,
    ring: tick2.at + DUR.thread,
    /** The column clears, top first: the card, then the tiles. */
    clear: out.at,
    focus: focusPlan(
      [
        [adopt.at, "term"],
        [card.at, "tiles"],
        [yes1.at, "term"],
        [tick1.at, "tiles"],
        [yes2.at, "term"],
        [tick2.at, "tiles"],
        [out.at, "*"],
      ],
      acts.map((a) => [a.at, a.held] as const),
    ),
  };
});

type Schedule = ReturnType<typeof schedule>;

/** Where `~/.zshrc`'s create row stands in the pane as the card lifts off it: the card's source. */
function sourceRow(
  c: Capture,
  x: Schedule,
): { x: number; y: number; w: number; h: number } {
  const ct = clipTime(x.plays, x.lift).ct;
  const sc = screenAt(c, ct);
  const texts = sc.lines.map(lineText);
  const i = Math.max(
    0,
    texts.findIndex((l) => ZSHRC_ROW.test(l)),
  );
  const L = termLayout(LEFT, texts.length, scrollOfTake(c, ct, LEFT));
  const cell = L.cell(i, 0);
  return {
    x: cell.x - 8,
    y: cell.y,
    w: Array.from(texts[i].trimEnd()).length * L.advance + 16,
    h: cell.h,
  };
}

const centre = (r: { x: number; y: number; w: number; h: number }) => ({
  x: r.x + r.w / 2,
  y: r.y + r.h / 2,
});

/** The steps' tiles (ART.md §12.5): in together, pending; each ticks (or starts) as its thread lands. */
function tilesOf(x: Schedule, variant: string): Tile[] {
  return bootstrapLabels(variant).map((label): Tile => {
    switch (label) {
      case "Dotfiles":
        return { label, appear: x.tiles, done: x.wrote + DUR.thread };
      case "Config":
        return {
          label,
          appear: x.tiles,
          done: x.wrote + DUR.thread + DUR.stagger,
        };
      case "Watcher":
        return {
          label,
          appear: x.tiles,
          done: x.applied === null ? undefined : x.applied + DUR.thread,
        };
      default:
        return { label, appear: x.tiles, start: x.ring };
    }
  });
}

export const cues: SceneCues<"bootstrap"> = (facts) => {
  const d = reelData(facts);
  if (!capture(d, "C18")) return {};
  const x = schedule(d);
  const systemd = d?.variant !== "plain";
  // Each tile's tick (and the Tools tile's ring) as its thread lands.
  return {
    card: x.lift,
    wrote: x.wrote + DUR.thread,
    watcher: systemd && x.applied !== null ? x.applied + DUR.thread : null,
    tools: x.ring,
  };
};

export const scene = greyScene(
  "bootstrap",
  (g) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C18");
    if (!c) {
      rest(g, "bootstrap|breath");
      return;
    }
    const x = schedule(g.d);
    const pane = LEFT;
    const ps = x.plays;
    const ct = clipTime(ps, b).ct;
    const src = sourceRow(c, x);
    const dim = unfocus(x.focus, "term", b);
    const shown = keep(g, () => {
      drawPaneWindow(ctx, pane, dim);
      // The create row takes a seat mark as its card lifts off it (the row
      // keeps its own text), and lets it go as the card lands.
      const mark =
        span(b, x.lift - DUR.flick, DUR.flick, "arrive") *
        (1 - span(b, x.lift + DUR.half, DUR.half, "glide"));
      seatMark(ctx, src, mark);
      return drawTake(g, c, ct, pane, {
        scroll: scrollOfTake(c, ct, pane),
        lineAlpha: BOOTSTRAP_DIM,
        plays: ps,
        dim,
        // The fresh machine's badge rides the chrome from `new` and fades
        // out over the quarter beat before `you/` is typed, so the
        // illustration badge pops in on its own (never beside it, pushing
        // it along the bar).
        badges: [
          {
            text: FRESH,
            alpha: 1 - span(b, x.named - DUR.badge, DUR.badge, "glide"),
          },
        ],
      });
    });
    // `~/.zshrc`, off its row to the top of the column; it leaves first.
    const lines = fileOf(g.d, "C18", ".zshrc")?.trim().split("\n") ?? null;
    const fly = EASE.travel(span(b, x.lift, DUR.move, "linear"));
    const gone = exit(b, x.clear);
    inFocus(ctx, x.focus, "tiles", b, () => {
      if (fly > 0 && fly < 1 && lines)
        flyingFile(ctx, src, LAYOUT.bootFile, fly, {
          title: "~/.zshrc",
          lines,
          size: FILE_SIZE,
          path: flight(centre(src), centre(LAYOUT.bootFile)),
        });
      else if (fly >= 1 && gone.alpha > 0) {
        ctx.save();
        ctx.translate(0, gone.dy);
        fileCard(ctx, LAYOUT.bootFile, {
          title: "~/.zshrc",
          lines,
          from: "C18",
          size: FILE_SIZE,
          alpha: gone.alpha,
        });
        ctx.restore();
      }
    });
    const steps = tilesOf(x, g.d?.variant ?? "systemd");
    const rects = inFocus(ctx, x.focus, "tiles", b, () =>
      tiles(ctx, {
        tiles: steps,
        b,
        t: g.t,
        leave: x.clear + DUR.stagger,
      }),
    );
    // Each step's row sends a thread to its tile, which ticks as it lands.
    steps.forEach((tile, i) => {
      const re = STEP_ROWS[tile.label];
      const from =
        tile.label === "Tools"
          ? x.tools
          : tile.label === "Watcher"
            ? x.applied
            : x.wrote + (tile.label === "Config" ? DUR.stagger : 0);
      if (!re || from === null) return;
      const k = span(b, from, DUR.thread, "arrive");
      const a =
        1 - span(b, from + DUR.thread + DUR.half, DUR.threadFade, "glide");
      if (k <= 0 || a <= 0) return;
      const texts = shown.lines.map(lineText);
      let row = -1;
      texts.forEach((l, j) => {
        if (re.test(l)) row = j;
      });
      if (row < 0) return;
      const cell = shown.layout.cell(row, 0);
      const end = shown.layout.col(Array.from(texts[row].trimEnd()).length);
      const x0 = Math.min(end, pane.x + pane.w - THREAD.exit) + THREAD.exit;
      const y0 = cell.y + cell.h / 2;
      const r = rects[i];
      const y1 = r.y + r.h / 2;
      const gx = THREAD.gutterX;
      threadAlong(
        ctx,
        Math.abs(y1 - y0) < 2
          ? [
              { x: x0, y: y0 },
              { x: r.x, y: y0 },
            ]
          : [
              { x: x0, y: y0 },
              { x: gx, y: y0 },
              { x: gx, y: y1 },
              { x: r.x, y: y1 },
            ],
        k,
        a,
      );
    });
  },
  {
    // The pane is the lit screen.
    lit: (): LitRect => termLit(rectOf(LEFT)),
    events: (d) => schedule(d).p.events(),
  },
);

/** The schedule, for the pacing checks. */
export const bootstrapSchedule = schedule;
