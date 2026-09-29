// What Act IV's task scenes share (depends, skip, args, daemons): the
// terminal window kept up across a bar line but free to leave and come back
// in between, the lit screen that follows it, and a task run's lines as the
// lanes take them out of the pane: each at its row in the terminal, handed
// over from the shot at one beat, then flown into its lane in turn.

import { type LitRect, TERM } from "../../bible";
import { castShadow, roundedRect } from "../../fx";
import { lerp } from "../../math";
import type { Capture, ScreenOptions } from "../../captures";
import { screenAt } from "../../captures";
import { staggerOf } from "../../kit/diagrams/common";
import { type LaneLine, taskLines } from "../../kit/diagrams/lanes";
import type { BoundaryId } from "../../handoff";
import { type G, keep, rectOf, restScreen } from "../../kit/grey";
import { drawPaneWindow } from "../../kit/rest";
import { arc, bump, curveAt, type Pt, span } from "../../kit/motion";
import {
  CHIP_ART,
  COLOR,
  DUR,
  MOTION,
  RADIUS,
  SHADOW,
  STROKE,
  TYPE,
} from "../../kit/style";
import {
  advance,
  drawTermLine,
  type Pane,
  run,
  termLayout,
  termLit,
} from "../../kit/term";

/** The pane's window, kept, at `alpha`, `dy` px off its rest (a window rising in or falling out). */
export function windowAt(g: G, p: Pane, alpha: number, dy = 0): void {
  if (alpha <= 0) return;
  keep(g, () => {
    g.ctx.save();
    g.ctx.globalAlpha *= alpha;
    g.ctx.translate(0, dy);
    drawPaneWindow(g.ctx, p);
    g.ctx.restore();
  });
}

/**
 * The pane's window coming back with bar line `id`'s screen already in it
 * (the next take's opening prompt, with its title): window and screen
 * rise and fade in together, so the window never shows empty or untitled
 * (ART.md §13 "Rests"). At rest (alpha 1, dy 0) it is exactly the rest's
 * pane layer.
 */
export function returnWindow(
  g: G,
  p: Pane,
  id: BoundaryId,
  alpha: number,
  dy = 0,
): void {
  if (alpha <= 0) return;
  windowAt(g, p, alpha, dy);
  g.ctx.save();
  g.ctx.translate(0, dy);
  restScreen(g, id, alpha);
  g.ctx.restore();
}

/** The pane's window as the lit screen the vignette spares, at `alpha` (none while it is gone). */
export const paneLit = (p: Pane, alpha: number): LitRect | null =>
  alpha > 0 ? termLit(rectOf(p), Math.min(1, alpha)) : null;

/**
 * A run's task lines as the lanes take them out of pane `p`: the screen at
 * take time `until` (cropped by `screen`), each task line where the pane
 * sets it. From beat `since` the lanes draw them there; from `flies` they
 * fly into their lanes together (lane by lane, a sixteenth apart, in the
 * order the lanes first printed, each over `flight` beats)
 * and wait there, queued, until each prints on `print(k, text, all)` (k
 * counts the task lines in the order they printed).
 */
export function flownLines(
  c: Capture,
  p: Pane,
  o: {
    after: number;
    until: number;
    screen?: ScreenOptions;
    since: number;
    flies: number;
    flight: number;
    /** When task line `k` (take order) prints, from its text and all the lines' texts. */
    print: (k: number, text: string, all: readonly string[]) => number;
  },
): LaneLine[] {
  const n = screenAt(c, o.until, o.screen ?? {}).lines.length;
  const L = termLayout(p, n);
  const fade = p.fade > 0 ? ([p.x + p.w - p.fade, p.x + p.w] as const) : null;
  const rows = taskLines(c, o);
  // A lane's lines fly together, so they keep their order in the air; the
  // lanes go a sixteenth apart, in the order they first printed.
  const names: string[] = [];
  const laneOf = (l: { runs: readonly { text: string }[] }) => {
    const name =
      /^\[([^\]\s]+)\]/.exec(l.runs.map((r) => r.text).join(""))?.[1] ?? "";
    if (!names.includes(name)) names.push(name);
    return names.indexOf(name);
  };
  const lanesOf = rows.map(laneOf);
  const texts = rows.map((l) => l.runs.map((r) => r.text).join(""));
  return rows.map((l, k) => {
    const flies = o.flies + staggerOf(lanesOf[k], names.length);
    return {
      runs: l.runs,
      at: o.print(k, texts[k], texts),
      from: {
        x: L.col(0),
        y: L.baseline(l.index),
        size: p.size,
        fade,
        since: o.since,
        flies,
        lands: flies + o.flight,
      },
    };
  });
}

/** Which of a screen's lines are a task's (`[name] …`): the rows the lanes take over. */
export const isTaskLine = (text: string): boolean =>
  /^\[[^\]\s]+\] /.test(text);

/** The first beat each task's lane appears (its first line lands), by task name. */
export function laneEntrances(lines: readonly LaneLine[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const l of lines) {
    const m = /^\[([^\]\s]+)\]/.exec(l.runs.map((r) => r.text).join(""));
    if (m && !out.has(m[1])) out.set(m[1], l.at);
  }
  return out;
}

/**
 * A captured value lifted out of a terminal row onto a card (ART.md §9
 * "Lifting a value"): the text, its capture colour, where it stands in the
 * pane (column 0's left edge, baseline, size), where its chip lands (the
 * chip's left edge and its text's baseline), the beat it starts to form,
 * and the beat it starts to leave.
 */
export interface ValueLift {
  text: string;
  color?: string;
  from: { x: number; y: number; size: number };
  to: { x: number; y: number };
  at: number;
  out: number;
}

/** How long a value lift takes to land, beats: form, rise, flight. */
export const VALUE_LANDS = 2 * DUR.flick + DUR.lift;

/** The highest a lifted value's arc bulges over its line, px, on a long throw. */
const MAX_BULGE = 48;

/** A value chip's width and height at mono `size` (kit/parts.ts chip's box). */
export const chipBox = (text: string, size: number) => ({
  w: Array.from(text).length * advance(size) + size,
  h: Math.round(size * CHIP_ART.heightEm),
});

/**
 * A value lifting (ART.md §9, form, then rise): the chip forms round a
 * copy of the captured run in place over 1/8 beat, its fill opaque from
 * the first frame so the row never reads doubled; it rises 6 px over the
 * next 1/8, its shadow lifting to `float`; it flies an arc (lift 0.2, at
 * most MAX_BULGE px; travel) over 3/4 beat to its place, growing from the
 * pane's size to TYPE.chip.lift (40, the size a value reads at on a
 * phone), and lands with a snap's small pop over 1/4, its shadow settling
 * to `small`. From `out` it fades over DUR.exit (glide). Held (landed)
 * until then, so a scene draws nothing of its own for it.
 */
export function liftValue(
  ctx: CanvasRenderingContext2D,
  l: ValueLift,
  b: number,
): void {
  if (b < l.at || b >= l.out + DUR.exit) return;
  const form = span(b, l.at, DUR.flick, "arrive");
  const rise = span(b, l.at + DUR.flick, DUR.flick, "arrive");
  const u = span(b, l.at + 2 * DUR.flick, DUR.lift, "travel");
  const landed = l.at + VALUE_LANDS;
  const leave = 1 - span(b, l.out, DUR.exit, "glide");
  const size = lerp(l.from.size, TYPE.chip.lift, u);
  const start: Pt = {
    x: l.from.x - l.from.size / 2,
    y: l.from.y - CHIP_ART.liftRise * rise,
  };
  const dx = Math.abs(l.to.x - start.x);
  const lift = Math.min(MOTION.arcLift, (2 * MAX_BULGE) / Math.max(1, dx));
  const p = u >= 1 ? l.to : curveAt(arc(start, l.to, lift), u);
  const pop = 1 + 0.06 * bump(b, landed, DUR.tick);
  const air = rise * (1 - span(b, landed, DUR.tick, "arrive"));
  const { w, h } = chipBox(l.text, size);
  const r = { x: p.x, y: p.y - h * 0.72, w, h };
  ctx.save();
  ctx.globalAlpha *= leave;
  ctx.translate(r.x + w / 2, r.y + h / 2);
  ctx.scale(pop, pop);
  ctx.translate(-(r.x + w / 2), -(r.y + h / 2));
  const path = () => roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.chip);
  castShadow(ctx, SHADOW.small, path, 1 - air, r);
  castShadow(ctx, SHADOW.float, path, air, r);
  path();
  ctx.fillStyle = TERM.window;
  ctx.fill();
  if (form > 0) {
    ctx.save();
    ctx.globalAlpha *= form;
    roundedRect(ctx, r.x + 1, r.y + 1, r.w - 2, r.h - 2, RADIUS.chip - 1);
    ctx.lineWidth = STROKE.badge;
    ctx.strokeStyle = COLOR.mark;
    ctx.stroke();
    ctx.restore();
  }
  drawTermLine(
    ctx,
    [run(l.text, l.color ?? TERM.text)],
    p.x + size / 2,
    p.y,
    size,
  );
  ctx.restore();
}
