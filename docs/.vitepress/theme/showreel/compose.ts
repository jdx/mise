// from jdx/hk@37937824 docs/.vitepress/theme/showreel/compose.ts
// Composites scenes, their captions, and the reel's finishing passes.
// `render` is a pure function of time, so playback, scrubbing, and offline
// export agree. It imports no scene, so a preview can bundle just the one
// it shows (showreel-frames.mjs); reel.ts composes all of them.
//
// Adapted for mise: chapters are acts, a draft can burn in a slate
// (timecode, section and beat) over the finished frame, and the finishing
// passes take the stage's numbers (kit/style.ts STAGE_FX: the vignette at
// 0.42, the grain). Each scene paints the stage itself, lamp and all
// (fx.ts drawStage, from kit/grey.ts greyScene), as the bar lines' frames do
// (handoff.ts). A film (film.ts) that cuts between sections the source
// never put side by side hands the scenes its joins (bible.ts Joins) and
// may ask for a frame with the scene standing off its mark (Presence): it
// is then drawn on a layer of its own over the stage, so the stage, the
// vignette and the grain stay still while the scene enters or leaves.

import {
  actOf,
  BEAT,
  type Chapter,
  CHAPTERS,
  DURATION,
  H,
  type Joins,
  PALETTE,
  type ReelFacts,
  type Scene,
  type SceneEnv,
  sec,
  W,
} from "./bible";
import { rgba } from "./color";
import { drawStage, grain, makeCanvas, vignette } from "./fx";
import type { Presence } from "./kit/motion";
import { STAGE_FX } from "./kit/style";
import { clamp } from "./math";
import { drawCaptions, font, MONO, timeCaptions } from "./type";

export interface Reel {
  duration: number;
  chapters: Chapter[];
  /**
   * Draw the frame at `t` seconds into a canvas `pw` × `ph` device pixels.
   * With `frame`, the scene stands off its mark as the presence says (its
   * opacity and how far it is below its place, logical px), on a layer over
   * the still stage: how a film enters and leaves a scene it cuts to
   * (film.ts).
   */
  render(
    ctx: CanvasRenderingContext2D,
    t: number,
    pw: number,
    ph: number,
    frame?: Presence,
  ): void;
}

export interface ReelOptions {
  /** Skip captions, grain, and vignette (for comparing raw scene frames). */
  raw?: boolean;
  /**
   * Burn a slate into the top-left corner, over everything: the timecode,
   * the act and section, and the section's beat. For drafts and review
   * frames only; the published render never sets it.
   */
  burnIn?: boolean;
  /**
   * The bar lines a film gives the sections it plays out of the source
   * order (bible.ts Joins): a ticket then takes down the stage of the
   * section the film plays before it. None for the source reel.
   */
  joins?: Joins;
}

/** `m:ss.fff`, the slate's timecode. */
export function timecode(t: number): string {
  const ms = Math.round(t * 1000);
  return `${Math.floor(ms / 60_000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}.${String(ms % 1000).padStart(3, "0")}`;
}

/**
 * The slate a draft burns in at `t`, over the finished frame: a small HUD in
 * the top right corner, clear of the stage and of the player's controls at
 * the foot of the frame, with the section's id, the timecode and the beat.
 */
function drawSlate(ctx: CanvasRenderingContext2D, s: Scene, t: number): void {
  const sc = sec(s.id);
  const b = (t - sc.start) / BEAT;
  const text = `${s.id}  ${timecode(t)}  b${b.toFixed(2)}/${sc.beats}  ${actOf(s.id).numeral}`;
  ctx.save();
  ctx.font = font(20, 500, MONO);
  ctx.textBaseline = "alphabetic";
  const w = ctx.measureText(text).width;
  ctx.fillStyle = rgba(PALETTE.night, 0.78);
  ctx.fillRect(W - 24 - w - 24, 14, w + 24, 34);
  ctx.fillStyle = PALETTE.paper;
  ctx.fillText(text, W - 24 - w - 12, 38);
  ctx.restore();
}

/** The layer a scene is drawn on when it stands off its mark (Reel.render's `frame`), one per canvas size. */
let layer: { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } | null =
  null;
function sceneLayer(pw: number, ph: number): CanvasRenderingContext2D {
  if (!layer || layer.canvas.width !== pw || layer.canvas.height !== ph) {
    const canvas = makeCanvas(pw, ph);
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("no 2d context for the scene layer");
    layer = { canvas, ctx };
  }
  return layer.ctx;
}

/** Whether a presence leaves the scene exactly on its mark. */
const onMark = (p: Presence | undefined): boolean =>
  !p || (p.alpha >= 1 && p.dy === 0);

/**
 * A reel of `scenes`, in timeline order. Given every section's scene it is
 * the whole reel; given one, it draws that section on the reel's clock, so
 * times outside it fall to the nearest scene.
 */
export function composeReel(
  scenes: readonly Scene[],
  facts: ReelFacts | null,
  options: ReelOptions = {},
): Reel {
  if (!scenes.length) throw new Error("a reel needs a scene");
  const sceneAt = (t: number): Scene => {
    for (const s of scenes) if (t < s.end) return s;
    return scenes[scenes.length - 1];
  };
  // Captions can depend on the facts, so they are placed once per reel.
  const captions = scenes.flatMap((s) =>
    timeCaptions(sec(s.id), s.captions?.(facts) ?? []),
  );
  return {
    duration: DURATION,
    chapters: CHAPTERS,
    render(ctx, time, pw, ph, frame) {
      const t = clamp(time, 0, DURATION - 1e-6);
      ctx.setTransform(pw / W, 0, 0, ph / H, 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = PALETTE.bg;
      ctx.fillRect(0, 0, W, H);
      const s = sceneAt(t);
      const lt = t - s.start;
      const env: SceneEnv = { W, H, t, facts, joins: options.joins };
      // The scene off its mark (a film's entrance or exit): the stage
      // stays, and the scene's own frame, lamp and all, rises or falls over
      // it at the presence's opacity. The lamp is a soft gradient, so its
      // copy moving a few px under the fade is not seen.
      const off = onMark(frame) ? null : frame!;
      if (!off) {
        ctx.save();
        s.draw(ctx, lt, env);
        ctx.restore();
      } else {
        const lc = sceneLayer(pw, ph);
        lc.setTransform(pw / W, 0, 0, ph / H, 0, 0);
        lc.globalAlpha = 1;
        lc.globalCompositeOperation = "source-over";
        lc.save();
        s.draw(lc, lt, env);
        lc.restore();
        lc.setTransform(1, 0, 0, 1, 0, 0);
        drawStage(ctx, W, H, t);
        if (off.alpha > 0) {
          ctx.save();
          ctx.globalAlpha = clamp(off.alpha);
          ctx.setTransform(1, 0, 0, 1, 0, (off.dy * ph) / H);
          ctx.drawImage(lc.canvas, 0, 0);
          ctx.restore();
          ctx.setTransform(pw / W, 0, 0, ph / H, 0, 0);
        }
      }
      if (!options.raw) {
        // Darker toward the corners, but not over a terminal's window, a lit
        // screen (standing where the scene stands).
        const lit = s.lit?.(lt, env) ?? null;
        vignette(
          ctx,
          W,
          H,
          STAGE_FX.vignette.strength,
          off && lit
            ? { ...lit, y: lit.y + off.dy, alpha: lit.alpha * clamp(off.alpha) }
            : lit,
        );
        // Over the vignette, so a caption reads the same at the frame's edge;
        // under the grain, so it sits in the picture. With the scene.
        ctx.save();
        if (off) {
          ctx.globalAlpha = clamp(off.alpha);
          ctx.translate(0, off.dy);
        }
        drawCaptions(ctx, t, captions);
        ctx.restore();
        grain(ctx, W, H, t, STAGE_FX.grain.amount, STAGE_FX.grain.fps);
      }
      if (options.burnIn) drawSlate(ctx, s, t);
      // Guard against a scene leaving the transform or blend mode dirty.
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";
    },
  };
}
