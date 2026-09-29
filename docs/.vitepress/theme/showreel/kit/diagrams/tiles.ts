// Bootstrap's tiles (bootstrap, ART.md §12.5): one raised tile per step of
// `mise bootstrap --adopt` (Dotfiles, Config, Watcher, Tools), stacked, each
// with a terracotta bar down its left edge and a mark at its right end:
// pending, a dashed ring; started, a solid paper ring; done, a paper tick
// drawn on, the tile's edge flashing to paper and back. A tile appears when
// the take prints the row that names it; tiles that appear together
// cascade. Tools only ever starts on camera, so it never ticks: given the
// global time, its ring carries a turning arc, the step still running as
// mise's own spinner turns, and the shot leaves it so. Without systemd on
// machine 2 there is no Watcher tile and the rest close up
// (`bootstrapLabels`). The tiles can leave together, top first, each
// falling and fading a sixteenth after the one above. A pure function of
// the steps' beats, the section's beat and (for the spinner) global time.

import { BEAT, PALETTE, PILLAR } from "../../bible";
import { mix } from "../../color";
import { roundedRect } from "../../fx";
import { clamp, lerp, TAU } from "../../math";
import { drawText, font } from "../../type";
import { exit, type Rect } from "../motion";
import {
  COLOR,
  DASH,
  DUR,
  LAYOUT,
  MOTION,
  RADIUS,
  SHADOW,
  STROKE,
  TILES_ART,
  TYPE,
} from "../style";
import {
  bumpOver,
  pop,
  raised,
  scaled,
  span,
  staggerOf,
  typeBaseline,
  tick,
} from "./common";

/** A step's tile: its label, and the beats it appears, starts and finishes (never, for a step the shot leaves running). */
export interface Tile {
  label: string;
  appear: number;
  start?: number;
  done?: number;
}

export type TileState = "hidden" | "pending" | "started" | "done";

/** Where a tile's step stands on beat `b`. */
export function tileState(t: Tile, b: number): TileState {
  if (b < t.appear) return "hidden";
  if (t.done !== undefined && b >= t.done) return "done";
  if (t.start !== undefined && b >= t.start) return "started";
  return "pending";
}

/** The steps bootstrap shows, in order: no Watcher on a machine without systemd. */
export function bootstrapLabels(variant: string): string[] {
  return variant === "plain"
    ? ["Dotfiles", "Config", "Tools"]
    : ["Dotfiles", "Config", "Watcher", "Tools"];
}

export interface TilesOptions {
  tiles: readonly Tile[];
  /** Local beats. */
  b: number;
  alpha?: number;
  /** The column the tiles stack in (LAYOUT.tiles). */
  rect?: Rect;
  /**
   * GLOBAL seconds: a started step's ring turns an arc round it, one turn
   * every SPIN beats, the step still running. Without it the ring is still.
   */
  t?: number;
  /** From this beat the tiles leave, top first: each falls and fades (DUR.exit), a sixteenth apart. */
  leave?: number;
}

/** A started ring's arc turns once in this many beats (global time): a slow, steady spinner. */
export const SPIN = 2;
/** The arc's length, radians, and how far the ring under it dims while it turns. */
const ARC = (5 * Math.PI) / 9;
const TRACK = 0.25;

/** The tiles on beat `b`. Returns each tile's rect. */
export function tiles(ctx: CanvasRenderingContext2D, o: TilesOptions): Rect[] {
  const T = TILES_ART;
  const R = o.rect ?? LAYOUT.tiles;
  const b = o.b;
  const alpha = o.alpha ?? 1;
  const rects = o.tiles.map((_, i) => ({
    x: R.x,
    y: R.y + i * (T.h + T.gap),
    w: R.w,
    h: T.h,
  }));
  if (alpha <= 0) return rects;
  // Tiles that appear on the same beat cascade, 1/16 beat apart.
  const appear = o.tiles.map((t, i) => {
    let n = 0;
    for (let j = 0; j < i; j++)
      if (Math.abs(o.tiles[j].appear - t.appear) < 1e-6) n++;
    return t.appear + n * DUR.stagger;
  });
  ctx.save();
  ctx.globalAlpha *= alpha;
  o.tiles.forEach((t, i) => {
    const k = span(b, appear[i], DUR.enter, "arrive");
    if (k <= 0) return;
    const out =
      o.leave === undefined
        ? { alpha: 1, dy: 0 }
        : exit(b, o.leave + staggerOf(i, o.tiles.length));
    if (out.alpha <= 0) return;
    const r = {
      ...rects[i],
      y: rects[i].y + MOTION.riseIn * (1 - k) + out.dy,
    };
    const started =
      t.start === undefined ? 0 : span(b, t.start, DUR.tick, "arrive");
    const doneK =
      t.done === undefined ? 0 : span(b, t.done, DUR.tick, "arrive");
    const on = Math.max(started, doneK);
    const flash = t.done === undefined ? 0 : bumpOver(b, t.done, DUR.half);
    ctx.save();
    ctx.globalAlpha *= k * out.alpha;
    raised(ctx, r, RADIUS.tile, {
      shadow: SHADOW.small,
      edge: mix(PALETTE.divider, PALETTE.paper, flash),
      edgeWidth: lerp(STROKE.hairline, STROKE.lit, flash),
    });
    // The machine pillar's bar down its left edge, inside its corners.
    ctx.save();
    roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.tile);
    ctx.clip();
    ctx.fillStyle = PILLAR.machine;
    ctx.fillRect(r.x, r.y, T.bar, r.h);
    ctx.restore();
    drawText(
      ctx,
      t.label,
      r.x + T.labelX,
      typeBaseline(r.y + r.h / 2, TYPE.tile.size),
      {
        font: font(TYPE.tile.size, TYPE.tile.weight),
        fill: mix(PALETTE.text3, PALETTE.text1, on),
      },
    );
    drawMark(ctx, r.x + r.w - T.markX, r.y + r.h / 2, started, doneK, o.t);
    ctx.restore();
  });
  ctx.restore();
  return rects;
}

/**
 * A tile's mark: the dashed ring pending, the solid ring started (with its
 * arc turning, given global time `t`), the tick done.
 */
function drawMark(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  started: number,
  done: number,
  t?: number,
): void {
  const r = TILES_ART.markR;
  ctx.save();
  ctx.lineCap = "round";
  // Pending: a dashed text3 ring, giving way to whichever comes next.
  const pending = 1 - Math.max(started, done);
  if (pending > 0) {
    ctx.save();
    ctx.globalAlpha *= pending;
    ctx.setLineDash([...DASH.waiting]);
    ctx.lineWidth = STROKE.tree;
    ctx.strokeStyle = PALETTE.text3;
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, TAU);
    ctx.stroke();
    ctx.restore();
  }
  // Started: a solid paper ring, popping in (0.9 → 1); a step that then
  // finishes trades it for the tick.
  const ring = started * (1 - done);
  if (ring > 0) {
    const p = pop(ring);
    ctx.save();
    ctx.globalAlpha *= p.alpha;
    scaled(ctx, cx, cy, p.scale, () => {
      ctx.lineWidth = STROKE.ring;
      ctx.strokeStyle = COLOR.mark;
      if (t === undefined) {
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, TAU);
        ctx.stroke();
        return;
      }
      // Running: the ring as a track, and a bright arc turning round it.
      ctx.save();
      ctx.globalAlpha *= TRACK;
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, TAU);
      ctx.stroke();
      ctx.restore();
      const a0 = TAU * ((t / (SPIN * BEAT)) % 1) - Math.PI / 2;
      ctx.beginPath();
      ctx.arc(cx, cy, r, a0, a0 + ARC);
      ctx.stroke();
    });
    ctx.restore();
  }
  tick(ctx, cx, cy, 2.1 * r, clamp(done));
  ctx.restore();
}
