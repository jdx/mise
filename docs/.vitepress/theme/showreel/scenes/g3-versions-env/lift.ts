// Things the Environments scenes move between a terminal, a card and the
// stage (ART.md §8–9): a captured value lifting out of its row as a value
// chip, and a file card flying out of a card row and folding back into it.
// Pure functions of the beat.

import {
  chip,
  fileCard,
  type FileCardOptions,
  roundCorners,
} from "../../kit/parts";
import { along } from "../../kit/diagrams/folders";
import { arc, curveAt, type Pt, type Rect, span } from "../../kit/motion";
import { DUR, EASE, MOTION } from "../../kit/style";
import { clamp, lerp, progress } from "../../math";
import { TERM } from "../../bible";
import { drawTermLine, run } from "../../kit/term";

/** Where a value is set: its first column's left edge, its baseline, its size. */
export interface Spot {
  x: number;
  y: number;
  size: number;
}

/** How long a value's lift takes, beats (ART.md §9): form, rise, flight and settle. */
export const LIFT_BEATS = DUR.flick + DUR.flick + DUR.lift + DUR.tick;

/** The corners of a routed flight, px. */
const ROUTE_R = 60;

/**
 * A captured value lifting from `from` (its terminal row) to `to` (where
 * its chip's text sits once landed) from beat `at` (ART.md §9: form, then
 * rise): the chip forms round a copy of the row in place over 1/8 beat,
 * its fill covering the pane's own glyphs from the first frame and the
 * copy on top of them, so the row never reads doubled; it rises 6 px over
 * the next 1/8, flies over 3/4 beat (travel) growing to its size, and
 * lands with snap over 1/4. The flight is an arc (`bulge`, motion.arc), or
 * with `clear` a routed one: right along its row's height to x `clear`
 * (past the text of every row shown), round a corner, up or down to its
 * slot's height, and into the slot from the right, keeping its row's size
 * until it is past the rows' text. Draws nothing before `at` or once
 * landed (the destination draws it then); returns whether it has landed.
 */
export function liftChip(
  ctx: CanvasRenderingContext2D,
  text: string,
  from: Spot,
  to: Spot,
  b: number,
  at: number,
  color: string = TERM.text,
  /** The arc's bulge (motion.arc): up and back by default; negative swings it under, rising into its place. */
  bulge: number = MOTION.arcLift,
  clear?: number,
): boolean {
  if (b < at) return false;
  const form = span(b, at, DUR.flick, "arrive");
  const rise = span(b, at + DUR.flick, DUR.flick, "settle");
  const flyAt = at + 2 * DUR.flick;
  const fly = span(b, flyAt, DUR.lift, "travel");
  // Landed by the clock, not by the ease: snap overshoots past 1 on its way.
  if (b >= flyAt + DUR.lift + DUR.tick) return true;
  const settle = span(b, flyAt + DUR.lift, DUR.tick, "snap");
  const start: Pt = { x: from.x, y: from.y - 6 * rise };
  let p: Pt = start;
  let size = lerp(from.size, to.size, fly);
  if (clear === undefined) {
    if (fly > 0) p = curveAt(arc(start, { x: to.x, y: to.y }, bulge), fly);
  } else {
    const route = roundCorners(
      [
        start,
        { x: clear, y: start.y },
        { x: clear, y: to.y },
        { x: to.x, y: to.y },
      ],
      ROUTE_R,
    );
    if (fly > 0) p = along(route, fly);
    // Its row's size along the row, growing once it is past the text.
    const total = route.reduce(
      (n, q, i) =>
        i ? n + Math.hypot(q.x - route[i - 1].x, q.y - route[i - 1].y) : 0,
      0,
    );
    const u0 = total > 0 ? (0.8 * Math.abs(clear - start.x)) / total : 0;
    size = lerp(
      from.size,
      to.size,
      EASE.move(clamp((fly - u0) / Math.max(1e-6, 1 - u0))),
    );
  }
  const dy = fly >= 1 ? -8 * (1 - settle) : 0;
  ctx.save();
  // The chip forming round the copy, and the copy on top of it.
  if (form > 0) chip(ctx, text, p.x - size / 2, p.y + dy, size, form, color);
  if (form < 1) drawTermLine(ctx, [run(text, color)], p.x, p.y + dy, size);
  ctx.restore();
  return false;
}

/**
 * Where a file card flown out of a row stands on beat `b` (ART.md §8): out
 * of the row from `outAt` (travel, 3/4 beat), fading in over the first
 * 30 % of the way; back into it from `backAt` (drop, 3/4 beat: it lands on
 * the row with the heavy thing's 6 % give), fading over the last 40 % of
 * the time, so it is small by the time it covers anything. `k` 0 is the
 * row, 1 its place. `o.out` and `o.back` set the two flights' beats (a
 * scene paced to one focal motion at a time, kit/pace.ts, gives each its
 * slot).
 */
export function outAndBack(
  b: number,
  outAt: number,
  backAt: number,
  o: { out?: number; back?: number } = {},
): { k: number; alpha: number } {
  if (b < backAt) {
    const k = span(b, outAt, o.out ?? DUR.std, "travel");
    return { k, alpha: clamp(k / 0.3) };
  }
  const p = progress(backAt, backAt + (o.back ?? DUR.std), b);
  return {
    k: clamp(1 - EASE.drop(p)),
    alpha: 1 - clamp((p - 0.6) / 0.4),
  };
}

/**
 * A file card between a card row and its place (ART.md §8), `k` of the
 * way (0 the row: the card a fifth its size, centred on it; 1 its rect), at
 * `alpha`: it travels an arc, tilting 1.5° in flight and settling to 0.
 * At its place (k 1, alpha 1) it is exactly fileCard's. With `under` (the
 * card or panel its row is in) it is clipped to outside that rect, so it
 * comes out from under the rect's edge and goes back under it, never
 * covering the text it came out of.
 */
export function fileBetween(
  ctx: CanvasRenderingContext2D,
  row: Rect,
  r: Rect,
  at: { k: number; alpha: number },
  o: FileCardOptions,
  under?: Rect,
): void {
  const { k, alpha } = at;
  if (alpha <= 0) return;
  if (k >= 1 && alpha >= 1) {
    fileCard(ctx, r, o);
    return;
  }
  if (under) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(-1e4, -1e4, 3e4, 3e4);
    ctx.rect(under.x, under.y, under.w, under.h);
    ctx.clip("evenodd");
    fileBetween(ctx, row, r, at, o);
    ctx.restore();
    return;
  }
  const a: Pt = { x: row.x + row.w / 2, y: row.y + row.h / 2 };
  const z: Pt = { x: r.x + r.w / 2, y: r.y + r.h / 2 };
  const c = curveAt(arc(a, z, MOTION.arcLift / 2), k);
  const s = lerp(0.2, 1, k);
  const tilt = ((MOTION.flightTilt * Math.PI) / 180) * Math.sin(Math.PI * k);
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.translate(c.x, c.y);
  ctx.rotate(tilt);
  ctx.scale(s, s);
  ctx.translate(-z.x, -z.y);
  fileCard(ctx, r, o);
  ctx.restore();
}
