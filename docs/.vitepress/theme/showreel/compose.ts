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
// (handoff.ts).

import {
  actOf,
  BEAT,
  type Chapter,
  CHAPTERS,
  DURATION,
  H,
  PALETTE,
  type ReelFacts,
  type Scene,
  sec,
  W,
} from "./bible";
import { rgba } from "./color";
import { grain, vignette } from "./fx";
import { STAGE_FX } from "./kit/style";
import { clamp } from "./math";
import { drawCaptions, font, MONO, timeCaptions } from "./type";

export interface Reel {
  duration: number;
  chapters: Chapter[];
  /** Draw the frame at `t` seconds into a canvas `pw` × `ph` device pixels. */
  render(
    ctx: CanvasRenderingContext2D,
    t: number,
    pw: number,
    ph: number,
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
    render(ctx, time, pw, ph) {
      const t = clamp(time, 0, DURATION - 1e-6);
      ctx.setTransform(pw / W, 0, 0, ph / H, 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = PALETTE.bg;
      ctx.fillRect(0, 0, W, H);
      const s = sceneAt(t);
      const lt = t - s.start;
      ctx.save();
      s.draw(ctx, lt, { W, H, t, facts });
      ctx.restore();
      if (!options.raw) {
        // Darker toward the corners, but not over a terminal's window, a lit screen.
        vignette(
          ctx,
          W,
          H,
          STAGE_FX.vignette.strength,
          s.lit?.(lt, { facts }) ?? null,
        );
        // Over the vignette, so a caption reads the same at the frame's edge;
        // under the grain, so it sits in the picture.
        drawCaptions(ctx, t, captions);
        grain(ctx, W, H, t, STAGE_FX.grain.amount);
      }
      if (options.burnIn) drawSlate(ctx, s, t);
      // Guard against a scene leaving the transform or blend mode dirty.
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";
    },
  };
}
