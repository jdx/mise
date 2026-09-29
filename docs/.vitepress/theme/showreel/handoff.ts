// from jdx/hk@37937824 docs/.vitepress/theme/showreel/handoff.ts
// The exact frame on every bar line. The outgoing scene settles onto its
// boundary's frame by its last beat and the incoming scene starts from it at
// rest (a "hold"), or both evaluate the same function of global time through
// it (a "motion": the whip into `new`). The frame at a section's `end`
// belongs to the next section.
//
// Every frame is drawn only with the shared kit's own calls, the same calls
// the scenes make, so a scene that ends by making them lands on its handoff
// to the pixel; test/handoff-frames.test.ts holds every scene to that in
// Chromium. A frame reads the facts only for the layers it keeps up (a
// card shows the take's file, a window the take's screen), so it is the same
// on both sides under any one set of facts. The whip reads global time (env.t).
//
// Adapted for mise: the registry keeps hk's contract, but a frame may read
// the facts: what stays on the stage across a bar line (kit/rest.ts: the
// station card, the terminal's window, the chef) is drawn from the capture
// set, so the frame is the same on both sides under any one set of facts,
// not under every set. Only a ticket and the whip clear the stage. Every
// frame stands on the stage itself (fx.ts drawStage: the background and the
// pass lamp at global time, which glides only in the morph), and a hold's
// lit screen is the window (or the chef) its rest keeps up (restLit).

import {
  type LitRect,
  PALETTE,
  type SceneEnv,
  SECTIONS,
  type SectionId,
  sec,
} from "./bible";
import { reelData } from "./captures";
import { drawStage } from "./fx";
import { drawRest, keepsStage, restLit } from "./kit/rest";
import { drawWhip } from "./whip";

type Adjacent<T extends readonly { id: string }[]> = T extends readonly [
  infer A extends { id: string },
  infer B extends { id: string },
  ...infer R extends { id: string }[],
]
  ? [`${A["id"]}|${B["id"]}`, ...Adjacent<[B, ...R]>]
  : [];
/** A bar line between two sections: `"<from>|<to>"`. */
export type BoundaryId = Adjacent<typeof SECTIONS>[number];

/** Every boundary in order, from open|pitch to morph|end. */
export const BOUNDARIES = SECTIONS.slice(1).map(
  (s, i) => `${SECTIONS[i].id}|${s.id}` as BoundaryId,
);

export interface Handoff {
  id: BoundaryId;
  from: SectionId;
  to: SectionId;
  /** The bar line, global seconds: `sec(to).start`. */
  t: number;
  /**
   * hold: both scenes rest on this frame (the outgoing one settles onto it
   * by its last frame, the incoming one starts from it at rest). motion: a
   * shared move crosses the bar line (drawWhip); both scenes follow the
   * named function through it.
   */
  meet: "hold" | "motion";
  /** What is on the frame, in words. */
  note: string;
  /**
   * The lit screen on the frame (Scene.lit), which the vignette spares: both
   * scenes return it on their side of the bar line, so the vignette does not
   * jump there. Null where the frame has no screen.
   */
  lit: LitRect | null;
  /** Paint the whole frame. A motion handoff reads `env.t`. */
  draw(ctx: CanvasRenderingContext2D, env: SceneEnv): void;
}

/** Fill the whole frame. */
export const bg = (
  ctx: CanvasRenderingContext2D,
  env: SceneEnv,
  color: string = PALETTE.bg,
): void => {
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, env.W, env.H);
};

/**
 * A hold: the stage, and the layers both scenes keep up across the bar
 * line (kit/rest.ts RESTS), at rest.
 */
const plan = (id: BoundaryId): Omit<Handoff, "from" | "to" | "t"> => ({
  id,
  meet: "hold",
  note: keepsStage(id)
    ? "The stage (background and pass lamp) with the layers both scenes keep up across the bar line, at rest (kit/rest.ts RESTS)."
    : "The stage alone (background and pass lamp).",
  lit: restLit(id),
  draw(ctx, env) {
    drawStage(ctx, env.W, env.H, env.t);
    drawRest(ctx, id, reelData(env.facts), env.t);
  },
});

const LIST: Omit<Handoff, "from" | "to" | "t">[] = [
  plan("open|pitch"),
  plan("pitch|tools"),
  plan("tools|use"),
  plan("use|registry"),
  plan("registry|backends"),
  plan("backends|versions"),
  plan("versions|switch"),
  plan("switch|packslip"),
  plan("packslip|env"),
  plan("env|vars"),
  plan("vars|redact"),
  plan("redact|tasks"),
  plan("tasks|depends"),
  plan("depends|skip"),
  plan("skip|args"),
  plan("args|daemons"),
  plan("daemons|dotfiles"),
  plan("dotfiles|track"),
  plan("track|machines"),
  plan("machines|lock"),
  {
    id: "lock|new",
    meet: "motion",
    note: "The whip (whip.ts drawWhip of global t) over the stage: both scenes draw it; on the bar line it is the streaks at their peak.",
    lit: null,
    draw(ctx, env) {
      drawStage(ctx, env.W, env.H, env.t);
      drawWhip(ctx, env.t);
    },
  },
  plan("new|bootstrap"),
  plan("bootstrap|breath"),
  plan("breath|clone"),
  plan("clone|morph"),
  plan("morph|end"),
];

/** Every handoff by its boundary. */
export const HANDOFFS = Object.fromEntries(
  LIST.map((h) => {
    const [from, to] = h.id.split("|") as [SectionId, SectionId];
    return [h.id, { ...h, from, to, t: sec(to).start }];
  }),
) as Record<BoundaryId, Handoff>;

/** Paint boundary `id`'s bar-line frame. */
export function drawHandoff(
  ctx: CanvasRenderingContext2D,
  id: BoundaryId,
  env: SceneEnv,
): void {
  HANDOFFS[id].draw(ctx, env);
}

/** The handoff a section starts from, or null for the first. */
export function handoffIn(id: SectionId): Handoff | null {
  return Object.values(HANDOFFS).find((h) => h.to === id) ?? null;
}

/** The handoff a section ends on, or null for the last. */
export function handoffOut(id: SectionId): Handoff | null {
  return Object.values(HANDOFFS).find((h) => h.from === id) ?? null;
}
