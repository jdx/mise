// The seven tickets' shared pieces (STORYBOARD.md "Tickets hand the stage
// on", ART.md §10).
//
// A ticket takes the stage it inherits down under its printing paper and
// brings the next section's in under its lifting one. kit/grey.ts
// ticketStage does both with the motion vocabulary's exit and entrance
// (leave and arrive). Rendered, the exit read as a blink: leave's curve
// holds an object nearly still for most of its 3/8 beat and rushes it out
// in the last 1/8, so at 30 fps the pitch's card and terminal were whole
// on one frame and gone three frames later, the slideshow cut the
// viewer review asked to lose. So the tickets take the stage down on
// glide instead, inside the same window (each layer from beat 0, a
// sixteenth apart, gone by TICKET.cue.clearEnd, falling MOTION.fallOut
// px): it visibly leaves under the paper as the paper feeds. The next
// stage still comes in on the kit's entrance (stagePresence "enter"),
// which lands well. Both ends are exactly at rest, so the bar lines hold.
//
// And the score's cues for a ticket section (score/cues.ts LISTEN's
// print, tear and lift), from the ticket's own constants, so the printer
// ticks, the tear's F and the lift's air land where the paper does.

import type { Joins, LitRect, SectionId } from "../../bible";
import type { BoundaryId } from "../../handoff";
import {
  boundaryIn,
  boundaryOut,
  type G,
  greyScene,
  keep,
  stagePresence,
} from "../../kit/grey";
import type { Presence } from "../../kit/motion";
import { drawLayer, restLit, RESTS } from "../../kit/rest";
import { DUR, EASE, MOTION, TICKET } from "../../kit/style";
import { drawTicketFor, PRINT_TICKS } from "../../kit/ticket";
import { progress } from "../../math";

/** A ticket's cues, section beats: the feed's first printer step, the tear, the lift-off's dip. */
export const TICKET_CUES = {
  print: PRINT_TICKS[0],
  tear: TICKET.cue.tear,
  lift: TICKET.cue.liftAt,
} as const;

/**
 * How each of bar line `id`'s layers stands at a ticket's beat `b` as the
 * ticket takes it down: from TICKET.cue.clear, a sixteenth apart (capped
 * so the cascade fits), each falling and fading on glide, all gone by
 * TICKET.cue.clearEnd. Exactly at rest before the first starts.
 */
export function clearPresence(id: BoundaryId, b: number): Presence[] {
  const C = TICKET.cue;
  const n = RESTS[id].length;
  const each = n > 1 ? Math.min(DUR.stagger, DUR.staggerMax / (n - 1)) : 0;
  const dur = C.clearEnd - C.clear - (n - 1) * each;
  return RESTS[id].map((_, i) => {
    const at = C.clear + i * each;
    const k = EASE.glide(progress(at, at + dur, b));
    return { alpha: 1 - k, dy: MOTION.fallOut * k };
  });
}

/** Bar line `id`'s layers, kept, each as `ps` has it. */
export function drawStageAs(
  g: G,
  id: BoundaryId,
  ps: readonly Presence[],
): void {
  keep(g, () =>
    RESTS[id].forEach((l, i) => {
      const p = ps[i];
      if (!p || p.alpha <= 0) return;
      g.ctx.save();
      g.ctx.globalAlpha *= p.alpha;
      g.ctx.translate(0, p.dy);
      drawLayer(g.ctx, l, g.d, g.t);
      g.ctx.restore();
    }),
  );
}

/** Where a bar line keeps its lit screen: the index of its pane (or chef) layer, or -1. */
const litLayer = (id: BoundaryId): number =>
  RESTS[id].findIndex((l) => l.kind === "pane" || l.kind === "chef");

/**
 * A ticket's lit screen at beat `b` (kit/grey.ts sectionLit's ticket rule,
 * on the tickets' own clear): the incoming bar line's window as it
 * leaves, then the next one's as it arrives. The bar lines are the reel's
 * (kit/grey.ts boundaryIn, boundaryOut): a film's joins where it has them.
 */
export function ticketLit(
  id: SectionId,
  b: number,
  joins?: Joins,
): LitRect | null {
  const hin = boundaryIn(id, joins);
  const hout = boundaryOut(id, joins);
  const at = (r: LitRect | null, k: number): LitRect | null =>
    r && k > 0 ? { ...r, alpha: r.alpha * Math.min(1, k) } : null;
  const z = hout ? restLit(hout) : null;
  if (hout && z) {
    const i = litLayer(hout);
    const k = i < 0 ? 0 : stagePresence(hout, "enter", b)[i].alpha;
    if (k > 0) return at(z, k);
  }
  const a = hin ? restLit(hin) : null;
  if (!hin || !a) return null;
  const i = litLayer(hin);
  return at(a, i < 0 ? 0 : clearPresence(hin, b)[i].alpha);
}

/**
 * A ticket section's scene: the stage it inherits taken down under the
 * printing paper (clearPresence), the next section's brought in under the
 * lifting ticket (the kit's entrance), and the act's ticket on its rail
 * (kit/ticket.ts drawTicketFor), lit as ticketLit says. The stage it
 * inherits is the bar line it starts from in the reel being drawn (G.prev):
 * the source's, or the section a film plays before it (film.ts).
 */
export function ticketScene(id: SectionId) {
  return greyScene(
    id,
    (g) => {
      const { prev, next } = g;
      if (prev) drawStageAs(g, prev, clearPresence(prev, g.b));
      if (next) drawStageAs(g, next, stagePresence(next, "enter", g.b));
      drawTicketFor(g.ctx, id, g.b, { meta: g.join?.meta });
    },
    { lit: (b, _d, joins) => ticketLit(id, b, joins) },
  );
}
