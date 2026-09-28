// Pacing for Acts V to VII (track, lock, bootstrap, breath, clone): the
// kit's Pace (kit/pace.ts) as these scenes build on it. A schedule is
// built once per facts object; without the take a terminal act keeps the
// span the storyboard's event plan gives it (sections.json `plan`, which
// is this same schedule on today's capture set), so the captions anchor
// where they would with it; and the focus follows the focal motion, but
// never dims a terminal whose output is still being read (rules 3 and 4).

import type { SectionId } from "../../bible";
import type { Capture } from "../../captures";
import type { Play } from "../../kit/grey";
import { type FocusPlan, focusOf, type Pace, type Span } from "../../kit/pace";
import { boardOf } from "../../storyboard";

export { focusPlan, perFacts, takeIn } from "../../kit/pace";

/** Event `name` of section `id`'s storyboard plan (sections.json `plan`), or null. */
export function planned(
  id: SectionId,
  name: string,
): { at: number; end: number } | null {
  const e = boardOf(id).plan?.find((x) => x.name === name);
  return e ? { at: e.at, end: e.end } : null;
}

/**
 * A focal motion `name` of `dur` beats (Pace.move); without the take, the
 * plan's span for it, so every event, and every caption anchored on one,
 * lands where the plan (this schedule on today's take) has it.
 */
export function moveOr(
  p: Pace,
  c: Capture | null,
  name: string,
  dur: number,
  o: { after?: number } = {},
): Span {
  const s = c ? null : planned(p.id, name);
  return s ? p.move(name, s.end - s.at, { after: s.at }) : p.move(name, dur, o);
}

/**
 * The beat to start a `dur`-beat motion from so that it stops on the half
 * beat (no sooner than the Pace allows): a caption anchored half a beat
 * after it then starts on the half beat too, where the caption before it
 * can leave straight into it (storyboard.ts captionsFor: one change).
 */
export const endOnHalf = (p: Pace, dur: number): number =>
  Math.ceil((p.next() + dur) * 2 - 1e-6) / 2 - dur;

/** A terminal act as a schedule records it: its span, its plays, its Enter, and when its output has been read. */
export type TermAct = Span & {
  plays: readonly Play[];
  held: number;
  enter: number;
};

/**
 * Terminal act `name` on take `c`: Pace.term with its output read for
 * `lines`. Without the take, the plan's span for it (the pane draws the
 * missing card), held as long as the take's would be.
 */
export function termAct(
  p: Pace,
  name: string,
  c: Capture | null,
  plays: (c: Capture, at: number) => readonly Play[],
  o: { lines?: number; after?: number } = {},
): TermAct {
  if (c) return p.term(name, (at) => plays(c, at), o);
  const s = planned(p.id, name);
  const dur = s ? s.end - s.at : 1;
  const span = p.move(name, dur, { after: s?.at ?? o.after });
  return { ...span, plays: [], held: span.end + 2, enter: span.at };
}

/** Draw with layer `layer` dimmed as the focus has it on beat `b` (FOCUS_DIM out of focus). */
export function inFocus<T>(
  ctx: CanvasRenderingContext2D,
  plan: FocusPlan,
  layer: string,
  b: number,
  draw: () => T,
): T {
  const k = focusOf(plan, layer, b);
  if (k >= 1) return draw();
  ctx.save();
  ctx.globalAlpha *= k;
  try {
    return draw();
  } finally {
    ctx.restore();
  }
}
