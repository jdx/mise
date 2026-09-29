// track's checkpoint rail in motion (ART.md §12.9): the rail kit/parts.ts
// draws at rest (the bar lines' rail, kit/rest.ts), with the moves between
// its rests. Each checkpoint's node snaps in on the beat the take shows it
// (scale 0 to 1, 1/4 beat, snap), its id and trigger with it; the pointer
// drops onto the first as it snaps (14 px, snap 1/4, fading in over its
// first 1/8) and travels to each new one; on the rollback it
// rewinds to the checkpoint restored (leave 3/8), the dashed arc draws from
// there over the track (arrive 1/2), the rollback's node snaps in at the
// arc's end, and the pointer rides the arc to it (travel 1/2).
//
// Once every move is over the rail is kit/parts.ts `rail` itself, the same
// call the bar line makes, so the section lands on track|machines to the
// pixel. Until then it is that same call with the rollback's node and the
// pointer left out, and those two drawn here on the rail's own geometry.
// A pure function of the beat.

import { PALETTE, PILLAR } from "../../bible";
import { ring } from "../../fx";
import { clamp, lerp, progress, TAU } from "../../math";
import { drawText, font, MONO } from "../../type";
import { type Curve, curveAt, hop, span } from "../../kit/motion";
import type { Rect } from "../../kit/motion";
import { pointer, rail } from "../../kit/parts";
import type { Checkpoint } from "../../kit/rest";
import {
  COLOR,
  DASH,
  DUR,
  EASE,
  RAIL_ART,
  STROKE,
  TYPE,
} from "../../kit/style";

/** When each move happens, section beats. */
export interface RailTimes {
  /** Each checkpoint's node snaps in on this beat (Infinity: not in this take). */
  made: readonly number[];
  /**
   * The rollback, for the checkpoint made from another (`from`): the
   * pointer rewinds from `rewind`, the arc draws from `arc`; its node snaps
   * in on its `made` beat, and the pointer rides the arc from then.
   */
  rewind?: number;
  arc?: number;
}

/** How long each move takes, beats (ART.md §12.9). */
export const RAIL_DUR = {
  /** A node snapping in. */
  node: DUR.tick,
  /** The pointer dropping onto the first node as it snaps in, and fading in over the drop's start. */
  pointerIn: DUR.tick,
  pointerLag: 0,
  pointerFade: DUR.flick,
  /** The pointer travelling to a new node along the track. */
  step: DUR.half,
  /** The rewind to the checkpoint restored. */
  rewind: DUR.short,
  /** The arc drawing on. */
  arc: DUR.half,
  /** The pointer riding the arc. */
  ride: DUR.half,
  /** A ring off each new node as it lands, and how far it spreads, px. */
  ring: DUR.half,
  ringR: 44,
} as const;

/** The pointer drops onto the first node from this far above it, px. */
const POINTER_DROP = 14;

/** The rail's geometry, as kit/parts.ts `rail` lays it out. */
function geometry(r: Rect, n: number) {
  const A = RAIL_ART;
  const y = r.y + A.y;
  const x0 = r.x + A.inset;
  const x1 = r.x + r.w - A.inset;
  const slots = Math.max(3, n);
  const step = (x1 - x0 - 80) / (slots - 1);
  return {
    y,
    xAt: (i: number) => x0 + 40 + i * step,
    /** The pointer's apex over a node, at rest. */
    tipY: y - A.node.r - A.pointer.gap + 4,
    /** The rollback's arc from node `a` to node `z`. */
    arc(a: number, z: number): Curve {
      const p = { x: x0 + 40 + a * step, y: y - A.node.r - 10 };
      const q = { x: x0 + 40 + z * step, y: y - A.node.r - 10 };
      return {
        a: p,
        b: q,
        c: { x: (p.x + q.x) / 2, y: y - A.arcRise - A.node.r - 40 },
      };
    },
  };
}

/**
 * A rollback's beats from the moment its prompt is answered (`yes`): the
 * pointer rewinds from then, the arc starts drawing a 32nd before the
 * pointer is home, and the rollback's node snaps in as the arc arrives.
 */
export function rollbackBeats(yes: number): {
  rewind: number;
  arc: number;
  made: number;
} {
  const arc = yes + RAIL_DUR.rewind - DUR.flick;
  return { rewind: yes, arc, made: arc + RAIL_DUR.arc };
}

/** The last beat anything on the rail moves: from then on it is the bar line's rail. */
export function railSettled(dots: readonly Checkpoint[], o: RailTimes): number {
  if (!dots.length) return -Infinity;
  let end = (o.made[0] ?? Infinity) + RAIL_DUR.pointerLag + RAIL_DUR.pointerIn;
  dots.forEach((d, i) => {
    const m = o.made[i] ?? Infinity;
    end = Math.max(end, m + RAIL_DUR.node, m + RAIL_DUR.ring);
    if (d.from !== undefined) end = Math.max(end, m + RAIL_DUR.ride);
    else if (i > 0) end = Math.max(end, m + RAIL_DUR.step);
  });
  return end;
}

/** The rollback's arc drawn on to `u`: dashed COLOR.arc, as kit/parts.ts draws it at rest. */
function drawArc(ctx: CanvasRenderingContext2D, k: Curve, u: number): void {
  if (u <= 0) return;
  ctx.save();
  ctx.setLineDash([...DASH.arc]);
  ctx.strokeStyle = COLOR.arc;
  ctx.lineWidth = STROKE.arc;
  ctx.beginPath();
  for (let s = 0; s <= 40; s++) {
    const p = curveAt(k, (s / 40) * u);
    if (s) ctx.lineTo(p.x, p.y);
    else ctx.moveTo(p.x, p.y);
  }
  ctx.stroke();
  ctx.restore();
}

/** A node snapping in at `on` (0 to 1): as kit/parts.ts draws it, id and trigger under it. */
function drawNode(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  d: Checkpoint,
  on: number,
): void {
  if (on <= 0) return;
  const A = RAIL_ART;
  const sc = EASE.snap(clamp(on));
  ctx.save();
  ctx.fillStyle = COLOR.surfaceTop;
  ctx.beginPath();
  ctx.arc(x, y, (A.node.r + A.node.ring) * sc, 0, TAU);
  ctx.fill();
  ctx.fillStyle = PILLAR.machine;
  ctx.beginPath();
  ctx.arc(x, y, A.node.r * sc, 0, TAU);
  ctx.fill();
  ctx.globalAlpha *= clamp(on);
  drawText(ctx, d.id, x, y + A.idY, {
    font: font(TYPE.rail.id, 600, MONO),
    fill: PALETTE.text1,
    align: "center",
  });
  drawText(ctx, d.label, x, y + A.triggerY, {
    font: font(TYPE.rail.trigger, 500, MONO),
    fill: PALETTE.text2,
    align: "center",
  });
  ctx.restore();
}

/**
 * The checkpoint rail at `r` on beat `b`, with `dots` (kit/rest.ts
 * checkpoints: ids and triggers from the take) made on `o.made`.
 */
export function checkpointRail(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  dots: readonly Checkpoint[],
  b: number,
  o: RailTimes,
): void {
  const n = dots.length;
  const made = dots.map((_, i) => o.made[i] ?? Infinity);
  // At rest: the bar line's own rail.
  if (n === 0 || b >= railSettled(dots, o)) {
    const on = dots.map((d, i) => ({
      ...d,
      on: clamp(progress(made[i], made[i] + RAIL_DUR.node, b)),
    }));
    const last = made.reduce((k, m, i) => (b >= m ? i : k), -1);
    rail(ctx, r, on, last, 1);
    return;
  }
  const G = geometry(r, n);
  const onOf = (i: number) => progress(made[i], made[i] + RAIL_DUR.node, b);
  // The rollback's checkpoint, whose arc and node are drawn here.
  const rb = dots.findIndex((d) => d.from !== undefined);
  rail(
    ctx,
    r,
    dots.map((d, i) =>
      i === rb ? { ...d, on: 0, from: undefined } : { ...d, on: onOf(i) },
    ),
    -1,
    1,
  );
  if (rb >= 0) {
    const from = dots[rb].from!;
    const k = G.arc(from, rb);
    const arcAt = o.arc ?? made[rb] - RAIL_DUR.arc;
    drawArc(ctx, k, span(b, arcAt, RAIL_DUR.arc, "arrive"));
    drawNode(ctx, G.xAt(rb), G.y, dots[rb], onOf(rb));
  }
  // A ring off each node as it lands, in its own colour.
  made.forEach((m, i) => {
    const p = progress(m, m + RAIL_DUR.ring, b);
    if (p > 0 && p < 1)
      ring(ctx, G.xAt(i), G.y, RAIL_DUR.ringR, p, PILLAR.machine, STROKE.ring);
  });
  // The pointer: over the latest checkpoint, travelling between them.
  const first = made[0];
  if (b < first + RAIL_DUR.pointerLag) return;
  const dropAt = first + RAIL_DUR.pointerLag;
  const drop = span(b, dropAt, RAIL_DUR.pointerIn, "snap");
  let x = G.xAt(0);
  let y = G.tipY - POINTER_DROP * (1 - drop);
  let at = 0;
  for (let i = 1; i < n; i++) {
    if (b < made[i] && !(i === rb && o.rewind !== undefined && b >= o.rewind))
      break;
    if (i === rb) {
      // Back to the checkpoint restored, then along the arc to the new one.
      const from = dots[rb].from!;
      const back = span(b, o.rewind ?? made[rb], RAIL_DUR.rewind, "leave");
      x = lerp(G.xAt(at), G.xAt(from), back);
      y = G.tipY;
      if (b >= made[rb]) {
        const u = span(b, made[rb], RAIL_DUR.ride, "travel");
        const p = curveAt(G.arc(from, rb), u);
        x = p.x;
        y = p.y;
      }
    } else {
      const k = span(b, made[i], RAIL_DUR.step, "travel");
      x = lerp(G.xAt(at), G.xAt(i), k);
      y = G.tipY - 12 * hop(k);
    }
    at = i;
  }
  pointer(ctx, x, y, progress(dropAt, dropAt + RAIL_DUR.pointerFade, b));
}
