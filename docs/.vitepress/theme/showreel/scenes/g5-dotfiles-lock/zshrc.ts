// track's `~/.zshrc` file card, and the line the take's `echo … >>
// ~/.zshrc` appends to it. The card at rest is the fixture (kit/rest.ts
// zshrcLines), the bar lines' card. When the echo's Enter lands, a copy of
// the quoted text lifts out of the typed command (ART.md §9: it rises 6 px,
// its chip forms round it, it flies an arc to the card's next row, its
// chip giving way over the flight's last quarter so only the text seats,
// and lands with snap), becomes that row (with the row's seat mark, which
// holds until the history shows the watcher saved it, and fades), and,
// when the take's rollback lands, takes a seat mark again and folds back
// out of the card (1/2 beat: the row closes on move, its text fading
// evenly). The pane keeps its own text: the copy is drawn, the capture is
// not edited. A pure function of the beat.

import { TERM } from "../../bible";
import { castShadow, roundedRect } from "../../fx";
import { clamp, lerp, progress } from "../../math";
import { chipRect, mono, rectPath } from "../../kit/diagrams/common";
import { arc, curveAt, type Pt, type Rect, span } from "../../kit/motion";
import {
  type CardRows,
  fileCard,
  gridRuns,
  seatMark,
  syntaxFor,
} from "../../kit/parts";
import {
  CHIP_ART,
  COLOR,
  DUR,
  FILE_ART,
  MOTION,
  RADIUS,
  SHADOW,
  STROKE,
  TYPE,
} from "../../kit/style";
import type { Run } from "../../kit/term";

/** Where the copy lifts from: its runs as the pane set them, their first column's left edge and baseline. */
export interface LiftSource {
  runs: readonly Run[];
  x: number;
  y: number;
  size: number;
}

/** When the appended line moves, section beats. */
export interface AliasTimes {
  /** The echo's Enter: the copy lifts. */
  lift: number;
  /** The rollback lands: the line takes a seat mark (default none). */
  mark?: number;
  /** Then it folds out. */
  fold: number;
  /** The history shows the watcher saved it: its seat mark lets go (default a beat after it lands). */
  saved?: number;
}

/** The lift's phases, beats from its start (ART.md §9 "Lifting a value"). */
export const LIFT = {
  rise: DUR.flick,
  form: DUR.flick,
  fly: DUR.move,
  land: DUR.tick,
} as const;

/** The beat the copy lands in its row: its seat and its click. */
export const landsAt = (lift: number): number =>
  lift + LIFT.rise + LIFT.form + LIFT.fly;

const TITLE = "~/.zshrc";

/** The copy arrives this far above its row and drops the rest of the way (snap). */
const DROP = 8;

export interface ZshrcOptions {
  rect: Rect;
  /** The fixture's lines, or null for the missing card. */
  lines: readonly string[] | null;
  /** The line the echo appends, as it was typed, or null when the take lacks it. */
  alias: string | null;
  b: number;
  times: AliasTimes;
}

/** The appended row's place on the card: under the fixture's last line. */
function rowOf(
  r: Rect,
  placed: CardRows,
): { x: number; y: number; top: number } | null {
  const last = placed.at(-1);
  if (!last) return null;
  const lh = TYPE.file.lineH;
  const y = last.y + lh;
  return {
    x: r.x + FILE_ART.inset,
    y,
    top: y - 0.8 * TYPE.file.size - (lh - TYPE.file.size) / 2,
  };
}

/**
 * The card, with the appended line while it is in the file: the landed
 * text, its seat mark, and its fold out. Returns where the appended row
 * sets its text (for the copy's landing), or null.
 */
export function zshrcCard(
  ctx: CanvasRenderingContext2D,
  o: ZshrcOptions,
): Pt | null {
  const r = o.rect;
  const placed = fileCard(ctx, r, {
    title: TITLE,
    lines: o.lines,
    from: "fixtures",
  });
  const row = rowOf(r, placed);
  if (!row || o.alias === null) return row;
  const b = o.b;
  const landed = landsAt(o.times.lift);
  if (b < landed || b >= o.times.fold + DUR.fold) return row;
  const size = TYPE.file.size;
  const lh = TYPE.file.lineH;
  const fold = span(b, o.times.fold, DUR.fold, "move");
  // The seat mark: up as the copy lands, held a beat, fading over the next.
  const letGo = Math.max(
    landed + DUR.tick,
    o.times.saved ?? landed + DUR.tick + DUR.seatHold,
  );
  const seat = Math.max(
    span(b, landed, DUR.tick, "arrive") *
      (1 - span(b, letGo, DUR.seatFade, "glide")),
    o.times.mark === undefined ? 0 : span(b, o.times.mark, DUR.tick, "arrive"),
  );
  ctx.save();
  ctx.beginPath();
  ctx.rect(r.x, r.y + FILE_ART.tabH, r.w, r.h - FILE_ART.tabH - 2);
  ctx.clip();
  // The fold: the row closes up into the line above it (move) as its text
  // lifts off, fading evenly over the fold (linear), so it reads for its
  // whole half beat.
  if (fold > 0) {
    ctx.beginPath();
    ctx.rect(r.x, row.top, r.w, lh * (1 - fold));
    ctx.clip();
    ctx.globalAlpha *= 1 - progress(o.times.fold, o.times.fold + DUR.fold, b);
  }
  seatMark(ctx, { x: r.x + 12, y: row.top + 2, w: r.w - 24, h: lh - 4 }, seat);
  // Landing: the text drops onto its mark (the chip's frame gave way in flight).
  const land = span(b, landed, LIFT.land, "snap");
  const dy = -DROP * (1 - land) - 10 * fold;
  gridRuns(ctx, syntaxFor(TITLE)(o.alias), row.x, row.y + dy, size, {
    fade: [r.x + r.w - 72, r.x + r.w - 12],
  });
  ctx.restore();
  return row;
}

/** A value chip's frame (ART.md §9) round `text` set at (`x`, `y`): the terminal's fill, a paper outline. */
function chipFrame(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  size: number,
  alpha: number,
  /** 0 to 1: how far it is in flight (the float shadow), else the small one. */
  floatK: number,
): void {
  if (alpha <= 0) return;
  const cr = chipRect(Array.from(text).length, x - size / 2, y, size);
  ctx.save();
  ctx.globalAlpha *= alpha;
  const path = rectPath(ctx, cr, RADIUS.chip);
  castShadow(ctx, SHADOW.float, path, floatK);
  castShadow(ctx, SHADOW.small, path, 1 - floatK);
  path();
  ctx.fillStyle = TERM.window;
  ctx.fill();
  roundedRect(ctx, cr.x + 1, cr.y + 1, cr.w - 2, cr.h - 2, RADIUS.chip - 1);
  ctx.lineWidth = STROKE.badge;
  ctx.strokeStyle = COLOR.mark;
  ctx.stroke();
  ctx.restore();
}

/**
 * The copy in flight, from the typed command to the card's row: it rises,
 * its chip forms round it, it flies an arc (lift 0.2, travel) and drops
 * onto the row, where zshrcCard takes it over. Draw it over everything.
 */
export function aliasLift(
  ctx: CanvasRenderingContext2D,
  from: LiftSource | null,
  to: Pt | null,
  text: string | null,
  b: number,
  lift: number,
): void {
  if (!from || !to || text === null) return;
  const landed = landsAt(lift);
  if (b < lift || b >= landed) return;
  const rise = span(b, lift, LIFT.rise, "settle");
  const form = span(b, lift + LIFT.rise, LIFT.form, "arrive");
  const fly = span(b, lift + LIFT.rise + LIFT.form, LIFT.fly, "travel");
  const start: Pt = { x: from.x, y: from.y - CHIP_ART.liftRise * rise };
  // The copy's text lands a little above its row and drops the rest of the way.
  const end: Pt = { x: to.x, y: to.y - DROP };
  // Half the reel's usual bulge: the copy comes in level with its row
  // instead of dropping across the card's lines above it.
  const p = fly > 0 ? curveAt(arc(start, end, MOTION.arcLift / 2), fly) : start;
  const size = lerp(from.size, TYPE.file.size, fly);
  ctx.save();
  // A gentle tilt in flight, settling to level (ART.md §8's 1.5°).
  const tilt = Math.sin(Math.PI * fly) * ((MOTION.flightTilt * Math.PI) / 180);
  if (tilt) {
    ctx.translate(p.x, p.y);
    ctx.rotate(-tilt);
    ctx.translate(-p.x, -p.y);
  }
  // In flight it casts the float shadow; over the flight's last quarter the
  // chip gives way, outline and fill, so only the text seats among the
  // card's rows (a chip taller than their pitch would cross the row above).
  const floatK = fly <= 0 ? 0 : 1 - clamp((fly - 0.7) / 0.3);
  chipFrame(
    ctx,
    text,
    p.x,
    p.y,
    size,
    clamp(form) * (1 - progress(0.75, 1, fly)),
    floatK * clamp(fly * 5),
  );
  mono(ctx, fly < 0.5 ? from.runs : syntaxFor(TITLE)(text), p.x, p.y, size);
  ctx.restore();
}
