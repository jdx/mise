// registry (plan v3 §3, Act I; ART.md §12.1; STORYBOARD.md "Pacing"):
// three rivers of the pinned mise's registry names (C4) wash into the left
// column, back stream first, and flow left, the card dimming beside them.
// Once in they slow to a drift and dim to texture: `node` rides the middle
// stream and `terraform` the front one, each once, looking like every
// other name until they light in turn in tools pink, bold and a little
// larger, with their glow. The streams ease to a stop as the caption
// comes, and hold still while it says them, an underline drawing on under
// each as it is named, and leaves once read. The streams
// ebb, front first, leaving the left column empty for backends, and the
// card comes back to full. There is no count on screen: the caption
// carries "1,000+". The card stays in place through the section and both
// its bar lines.

import type { ReelFacts } from "../bible";
import { type CaptureId, type ReelData, reelData } from "../captures";
import { rivers } from "../kit/diagrams/rivers";
import { greyScene, missing, type Play, rest } from "../kit/grey";
import { span } from "../kit/motion";
import { type FocusPlan, focusOf, Pace } from "../kit/pace";
import { DUR, LAYOUT, RIVERS_ART } from "../kit/style";
import type { SceneCues } from "../score/cues";
import { perFacts, wordLands } from "./g2-tools/common";

/** The streams wash in over their first 3/4 beat, back first, and flow at full speed for 2 beats. */
const FLOW = 2;
const WASH = 3 / 4;
/** Then they slow, over a beat, to this fraction of their speed: a drift. */
const DRIFT = 0.3;
const SLOW = 1;
/** The lit names light a half beat apart. */
const LIT_EACH = DUR.half;
/** The drift eases to a stop over this long, ending on the caption's anchor (rule 5: what it names holds still). */
const STOP = 1;

/**
 * The distance a speed easing (smoothstep) from `v0` to `v1` over `d`
 * beats from `t0` has covered on beat `b`, in beats of drift: the
 * integral of v0 - (v0 - v1) smoothstep, smoothstep's being u³ - u⁴/2.
 */
function eased(b: number, t0: number, d: number, v0: number, v1: number) {
  const u = Math.min(1, Math.max(0, (b - t0) / d));
  return d * (u * v0 - (v0 - v1) * (u ** 3 - u ** 4 / 2));
}

/**
 * The streams' clock, in beats of drift, on beat `b`: running at full speed
 * to FLOW, easing down to DRIFT over SLOW, drifting, then easing to a stop
 * on beat `stop` (over STOP) and holding still from there. A pure function
 * of the beat.
 */
function drift(b: number, stop: number): number {
  const from = Math.max(FLOW + SLOW, stop - STOP);
  const dur = Math.max(1e-6, stop - from);
  return (
    Math.min(b, FLOW) +
    (b > FLOW ? eased(b, FLOW, SLOW, 1, DRIFT) : 0) +
    (b > FLOW + SLOW ? DRIFT * (Math.min(b, from) - FLOW - SLOW) : 0) +
    (b > from ? eased(b, from, dur, DRIFT, 0) : 0)
  );
}

/** When everything happens (kit/pace.ts), and the focus. */
const schedule = perFacts((d: ReelData | null) => {
  const p = new Pace("registry", { d, from: 0 });
  const flow = p.move("rivers", FLOW);
  const lit = p.move("lit", LIT_EACH + DUR.light);
  const cap = p.caption(0);
  // The caption leaves once read (sections.json `until`); the streams
  // ebb half a beat after it has gone.
  p.wait(cap.out + DUR.tick + DUR.half);
  const out = p.move("riversOut", WASH);
  const events = p.events();
  const light = { node: lit.at, terraform: lit.at + LIT_EACH };
  // The underline draws on under each as the caption names it.
  const named = {
    node: wordLands("registry", d, /^node$/, events) ?? cap.end,
    terraform: wordLands("registry", d, /^terraform\.?$/, events) ?? cap.end,
  };
  // The lit names stand in the column's middle (x 650) once the streams
  // have stopped, from the caption's anchor to the ebb; the slow drift
  // before it keeps them inside their window (x 540–760).
  const stop = cap.at;
  const cross = drift(stop, stop);
  // The rivers have the focus from the first frame; once in, their names
  // dim to texture under the lit two; all lit again on the bar line.
  const focus: FocusPlan = [
    [flow.at, "rivers"],
    [flow.end, "lit"],
    [out.at, "*"],
  ];
  return { p, light, named, cross, stop, ebb: out.at, focus };
});

export const scene = greyScene(
  "registry",
  (g) => {
    const { ctx, b } = g;
    const S = schedule(g.d);
    // use|registry and registry|backends keep the same card: dimmed while the rivers run.
    rest(g, "use|registry", focusOf(S.focus, "card", b));
    const names = g.d?.registry.names ?? [];
    if (!names.length) {
      missing(ctx, { ...LAYOUT.left, y: 300, h: 200 }, "C4");
      return;
    }
    const n = RIVERS_ART.rivers.length;
    const each = DUR.stagger * 2;
    const streams = RIVERS_ART.rivers.map(
      (_, i) =>
        span(b, i * each, WASH, "glide") *
        (1 -
          span(b, S.ebb + (n - 1 - i) * each, WASH - (n - 1) * each, "glide")),
    );
    rivers(ctx, {
      names,
      b,
      drift: drift(b, S.stop),
      cross: S.cross,
      named: S.named,
      light: S.light,
      streams,
      dim: focusOf(S.focus, "rivers", b),
    });
  },
  { events: (d) => schedule(d).p.events() },
);

/** The score's cues (score/cues.ts): each lit name lights in turn. */
export const cues: SceneCues<"registry"> = (facts: ReelFacts | null) => {
  const S = schedule(reelData(facts));
  return { lit: [S.light.node, S.light.terraform] };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: {} };
};
