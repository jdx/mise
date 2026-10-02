// from jdx/hk@37937824 docs/.vitepress/theme/showreel/fx.ts
// Shared finishing effects. Offscreen canvases are created lazily on first
// use so importing this module is safe during server-side rendering.
//
// Adapted for mise: the stage's light (the pass lamp over the stage, and
// its glide to the end card's), cast shadows from a ShadowSpec (kit/style.ts
// SHADOW) drawn without the shape that casts them, the motes round the chef,
// and a four-point sparkle (ART.md §3, §11).

import { type LitRect, PALETTE } from "./bible";
import { rgba } from "./color";
import type { ShadowSpec } from "./kit/style";
import { CHEF_ART, LAYOUT, STAGE_FX } from "./kit/style";
import { clamp, hash, inOutSine, lerp, progress, TAU } from "./math";
import { BEAT, sec } from "./timeline";

/** PALETTE.paper: flashes and rings default to the captions' warm paper. */
const BRIGHT = "#f4eee3";

export function makeCanvas(w: number, h: number): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

const glowCache = new Map<string, HTMLCanvasElement>();

/** Additive soft light. Cheap: one cached sprite per color. */
export function glow(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  radius: number,
  color: string,
  alpha = 1,
): void {
  if (alpha <= 0 || radius <= 0) return;
  let sprite = glowCache.get(color);
  if (!sprite) {
    sprite = makeCanvas(128, 128);
    const g = sprite.getContext("2d")!;
    const grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
    grad.addColorStop(0, rgba(color, 1));
    grad.addColorStop(0.2, rgba(color, 0.55));
    grad.addColorStop(0.5, rgba(color, 0.16));
    grad.addColorStop(1, rgba(color, 0));
    g.fillStyle = grad;
    g.fillRect(0, 0, 128, 128);
    glowCache.set(color, sprite);
  }
  ctx.save();
  ctx.globalCompositeOperation = "lighter";
  ctx.globalAlpha *= clamp(alpha);
  ctx.drawImage(sprite, x - radius, y - radius, radius * 2, radius * 2);
  ctx.restore();
}

let grainTiles: HTMLCanvasElement[] | null = null;

/**
 * Film grain over the frame, re-seeded `fps` times a second, or still at 0.
 * Its other job is dither: the lamp, the vignette and the glows climb only
 * 5 to 12 levels of 8-bit colour across hundreds of pixels, and the grain
 * breaks those steps up. Re-seeded grain is new noise on every frame, which
 * x264 cannot predict and rounds away at the reel's bitrate, so the steps
 * came back as rings that swept out as a glow swelled (the end card's
 * bloom: identical-luma runs of 22 px after the encode, 1.6 px before).
 * Still grain costs the encoder almost nothing to carry from frame to
 * frame, so it survives (2.7 px).
 */
export function grain(
  ctx: CanvasRenderingContext2D,
  W: number,
  H: number,
  t: number,
  amount = 0.06,
  fps = 0,
): void {
  if (amount <= 0) return;
  if (!grainTiles) {
    grainTiles = [];
    for (let k = 0; k < 4; k++) {
      const c = makeCanvas(256, 256);
      const g = c.getContext("2d")!;
      const img = g.createImageData(256, 256);
      for (let i = 0; i < 256 * 256; i++) {
        const v = hash(i, k + 11) * 255;
        img.data[i * 4] = v;
        img.data[i * 4 + 1] = v;
        img.data[i * 4 + 2] = v;
        img.data[i * 4 + 3] = 255;
      }
      g.putImageData(img, 0, 0);
      grainTiles.push(c);
    }
  }
  const f = fps > 0 ? Math.floor(t * fps) : 0;
  const tile = grainTiles[f % 4];
  const pattern = ctx.createPattern(tile, "repeat");
  if (!pattern) return;
  ctx.save();
  ctx.globalCompositeOperation = "overlay";
  ctx.globalAlpha = amount;
  ctx.translate(-hash(f, 3) * 256, -hash(f, 5) * 256);
  ctx.fillStyle = pattern;
  ctx.fillRect(0, 0, W + 256, H + 256);
  ctx.restore();
}

let vignetteLayer: HTMLCanvasElement | null = null;

/**
 * How far round a lit screen the vignette is spared, in the screen's half
 * widths and heights: all of it inside the ellipse through its corners,
 * fading back to none twice as far out.
 */
const SPARE_CORE = Math.SQRT2;
const SPARE_OUT = 2 * Math.SQRT2;

/**
 * Where the vignette starts, as a fraction of the way from the frame's
 * centre to its corners along the ellipse through them. At mid-height that
 * is just outside the stage's side margins (x 160 and 1760), so labels set
 * there keep their palette colours.
 */
const VIGNETTE_INNER = 0.55;

/**
 * Darkens the frame toward its corners: an ellipse with the frame's aspect,
 * clear inside VIGNETTE_INNER and easing in (quadratically) to `strength`
 * at the corners, so the stage is all but untouched and a long label does
 * not fade across its width. A lit screen, if given, is left out of it: the
 * ellipse through its corners spared by `lit.alpha`, fading back into the
 * vignette with no edge. Without one the vignette is painted straight onto
 * the frame.
 */
export function vignette(
  ctx: CanvasRenderingContext2D,
  W: number,
  H: number,
  strength = 0.35,
  lit: LitRect | null = null,
): void {
  const paint = (c: CanvasRenderingContext2D) => {
    // Squeezed by H/W about the centre, the frame is an H × H square and
    // the ellipse through its corners a circle of radius H/√2.
    c.save();
    c.translate(W / 2, H / 2);
    c.scale(W / H, 1);
    const g = c.createRadialGradient(0, 0, 0, 0, 0, (H / 2) * Math.SQRT2);
    g.addColorStop(0, "rgba(0,0,0,0)");
    for (let i = 0; i <= 8; i++) {
      const u = i / 8;
      g.addColorStop(
        VIGNETTE_INNER + (1 - VIGNETTE_INNER) * u,
        `rgba(0,0,0,${strength * u * u})`,
      );
    }
    c.fillStyle = g;
    c.fillRect(-H / 2, -H / 2, H, H);
    c.restore();
  };
  const spare = lit ? clamp(lit.alpha) : 0;
  if (!lit || spare <= 0 || lit.w < 1 || lit.h < 1) {
    ctx.save();
    paint(ctx);
    ctx.restore();
    return;
  }
  // On a layer of its own, so the spared ellipse can be cut out of it.
  const { width, height } = ctx.canvas;
  vignetteLayer ??= makeCanvas(width, height);
  if (vignetteLayer.width !== width || vignetteLayer.height !== height) {
    vignetteLayer.width = width;
    vignetteLayer.height = height;
  }
  const v = vignetteLayer.getContext("2d")!;
  v.setTransform(1, 0, 0, 1, 0, 0);
  v.globalAlpha = 1;
  v.globalCompositeOperation = "source-over";
  v.clearRect(0, 0, width, height);
  v.setTransform(ctx.getTransform());
  paint(v);
  v.globalCompositeOperation = "destination-out";
  v.translate(lit.x + lit.w / 2, lit.y + lit.h / 2);
  v.scale(lit.w / 2, lit.h / 2);
  const s = v.createRadialGradient(0, 0, 0, 0, 0, SPARE_OUT);
  const core = SPARE_CORE / SPARE_OUT;
  s.addColorStop(0, `rgba(0,0,0,${spare})`);
  for (let i = 0; i <= 8; i++) {
    const u = i / 8;
    s.addColorStop(
      core + (1 - core) * u,
      `rgba(0,0,0,${spare * (1 - u * u * (3 - 2 * u))})`,
    );
  }
  v.fillStyle = s;
  v.fillRect(-SPARE_OUT, -SPARE_OUT, 2 * SPARE_OUT, 2 * SPARE_OUT);
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
  ctx.drawImage(vignetteLayer, 0, 0);
  ctx.restore();
}

/** Full-frame flash, e.g. one or two frames on a hard cut. */
export function flash(
  ctx: CanvasRenderingContext2D,
  W: number,
  H: number,
  alpha: number,
  color = BRIGHT,
): void {
  if (alpha <= 0) return;
  ctx.save();
  ctx.globalCompositeOperation = "lighter";
  ctx.fillStyle = rgba(color, clamp(alpha));
  ctx.fillRect(0, 0, W, H);
  ctx.restore();
}

/**
 * Expanding shockwave ring. `p` runs 0..1 over its life; it grows from
 * radius `from` (0: out of its centre; a stamp's starts at its own edge, so
 * it never crosses the words it rings) to `radius`.
 */
export function ring(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  radius: number,
  p: number,
  color = BRIGHT,
  width = 6,
  from = 0,
): void {
  if (p <= 0 || p >= 1) return;
  const e = 1 - (1 - p) ** 3;
  const r = from + (radius - from) * e;
  ctx.save();
  ctx.strokeStyle = rgba(color, (1 - p) ** 1.5);
  // Never thicker than the ring is wide, so it does not start as a dot.
  ctx.lineWidth = Math.min(width * (1 - p) + 0.5, r);
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * 2);
  ctx.stroke();
  ctx.restore();
}

/**
 * Horizontal motion smear: draws `draw` several times along the motion with
 * falling opacity. `offset` is where the content sits, `velocity` how far it
 * travels in one frame (px); the trail extends behind it.
 */
export function smear(
  ctx: CanvasRenderingContext2D,
  offset: number,
  velocity: number,
  draw: () => void,
  samples = 6,
): void {
  const n = Math.abs(velocity) < 2 ? 1 : samples;
  for (let i = n - 1; i >= 0; i--) {
    ctx.save();
    ctx.translate(offset - (velocity * i) / n, 0);
    ctx.globalAlpha *= i === 0 ? 1 : 0.35 * (1 - i / n);
    draw();
    ctx.restore();
  }
}

/** Rounded rectangle path (independent of CanvasRenderingContext2D.roundRect). */
export function roundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  const rr = Math.max(0, Math.min(r, w / 2, h / 2));
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

/**
 * Deterministic screen shake offset for an impact at `at`. Zero on the
 * impact instant itself, so a shake keyed to a cut keeps the handoff exact;
 * it lasts about five times `decay`.
 */
export function shake(
  t: number,
  at: number,
  amp = 10,
  decay = 0.18,
): [number, number] {
  const d = t - at;
  if (d <= 0 || d > decay * 5) return [0, 0];
  const k = amp * Math.exp(-d / decay);
  const f = Math.floor(d * 60);
  return [(hash(f, 17) * 2 - 1) * k, (hash(f, 29) * 2 - 1) * k];
}

// The stage's light (ART.md §3).

/** A light on the stage: an ellipse of `color` at `alpha` in the middle, falling to nothing at its rim. */
export interface LampSpec {
  x: number;
  y: number;
  rx: number;
  ry: number;
  color: string;
  alpha: number;
}

/** A lamp `k` of the way from `a` to `b`. */
export const lerpLamp = (a: LampSpec, b: LampSpec, k: number): LampSpec => ({
  x: lerp(a.x, b.x, k),
  y: lerp(a.y, b.y, k),
  rx: lerp(a.rx, b.rx, k),
  ry: lerp(a.ry, b.ry, k),
  color: k < 0.5 ? a.color : b.color,
  alpha: lerp(a.alpha, b.alpha, k),
});

/**
 * The pass lamp at GLOBAL time `t`: still over the stage until the morph's
 * chef glides to its end-card place (CHEF_ART.glide, beats 5 to 6.5), then
 * gliding with it, on the same curve, to the end card's lamp over the
 * chef, which it keeps to the last frame. A pure function of global time,
 * so both sides of a bar line agree on it.
 */
export function lampAt(t: number): LampSpec {
  const m = sec("morph").start;
  const [g0, g1] = CHEF_ART.glide;
  const k = inOutSine(progress(m + g0 * BEAT, m + g1 * BEAT, t));
  return k <= 0
    ? STAGE_FX.lamp
    : k >= 1
      ? STAGE_FX.lampEnd
      : lerpLamp(STAGE_FX.lamp, STAGE_FX.lampEnd, k);
}

/** Paint a lamp: a smoothstep falloff from its centre to its rim, under whatever is drawn next. */
export function drawLamp(
  ctx: CanvasRenderingContext2D,
  lamp: LampSpec,
  k = 1,
): void {
  const a = lamp.alpha * k;
  if (a <= 0 || lamp.rx <= 0 || lamp.ry <= 0) return;
  ctx.save();
  ctx.translate(lamp.x, lamp.y);
  ctx.scale(1, lamp.ry / lamp.rx);
  const g = ctx.createRadialGradient(0, 0, 0, 0, 0, lamp.rx);
  for (let i = 0; i <= 12; i++) {
    const u = i / 12;
    g.addColorStop(u, rgba(lamp.color, a * (1 - u * u * (3 - 2 * u))));
  }
  ctx.fillStyle = g;
  ctx.fillRect(-lamp.rx, -lamp.rx, 2 * lamp.rx, 2 * lamp.rx);
  ctx.restore();
}

/**
 * The stage at GLOBAL time `t`: the background, and the pass lamp over it
 * (lampAt). Scenes and the bar lines' frames paint it first, under
 * everything.
 */
export function drawStage(
  ctx: CanvasRenderingContext2D,
  W: number,
  H: number,
  t: number,
): void {
  ctx.save();
  ctx.fillStyle = PALETTE.bg;
  ctx.fillRect(0, 0, W, H);
  drawLamp(ctx, lampAt(t));
  ctx.restore();
}

/** How far a shadow's caster is moved off the canvas, device px: only its shadow lands on the frame. */
const SHADOW_OFF = 20000;

/**
 * How far a shadow may spread past its object's sides, px: half the gutter
 * between the terminal's column and the card's, so a shadow never falls on
 * the neighbour across it.
 */
export const SHADOW_SPILL = LAYOUT.gutter.w / 2;

/** A rect of the stage (kit/motion.ts Rect). */
interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * A cast shadow from the lamp above: `path` (a function that builds a path
 * in the current transform, or a Path2D in it, filled nonzero) filled far
 * off the canvas with its shadow offset back, so only the shadow lands and
 * the object is then drawn once, whatever its own alpha. Offsets and blur
 * are device px scaled with the transform, so a preview at half size keeps
 * its proportions, and they fall straight down even under a rotation (the
 * light is above). The shadow's alpha is the spec's times `alpha` times the
 * context's globalAlpha. A clip set before the call clips the shadow.
 *
 * Given the object's `bounds`, the shadow falls only below its top edge and
 * within SHADOW_SPILL of its sides: it never climbs onto a badge above it or
 * spreads onto the neighbour across a gutter, so two objects side by side
 * look the same whichever is drawn first, and an object off the stage (a
 * layer waiting to whip in) casts nothing onto it. `bounds` are in the
 * current transform's units, as the path is.
 */
export function castShadow(
  ctx: CanvasRenderingContext2D,
  s: ShadowSpec,
  path: (() => void) | Path2D,
  alpha = 1,
  bounds?: Box,
): void {
  const a = clamp(s.alpha * alpha);
  if (a <= 0 || ctx.globalAlpha <= 0) return;
  const m = ctx.getTransform();
  const k = Math.hypot(m.a, m.b) || 1;
  const off = SHADOW_OFF / k;
  ctx.save();
  if (bounds) {
    ctx.beginPath();
    ctx.rect(
      bounds.x - SHADOW_SPILL,
      bounds.y,
      bounds.w + 2 * SHADOW_SPILL,
      bounds.h + s.dy + 3 * s.blur,
    );
    ctx.clip();
  }
  ctx.translate(-off, 0);
  ctx.shadowColor = rgba(s.color, a);
  ctx.shadowBlur = s.blur * k;
  ctx.shadowOffsetX = s.dx * k + m.a * off;
  ctx.shadowOffsetY = s.dy * k + m.b * off;
  ctx.fillStyle = "#000";
  if (typeof path === "function") {
    path();
    ctx.fill();
  } else ctx.fill(path);
  ctx.restore();
}

/** Several shadows under one shape (a raised object's `raised` and `contact`). */
export function castShadows(
  ctx: CanvasRenderingContext2D,
  specs: readonly ShadowSpec[],
  path: (() => void) | Path2D,
  alpha = 1,
  bounds?: Box,
): void {
  for (const s of specs) castShadow(ctx, s, path, alpha, bounds);
}

/** A rect of the frame: where motes may rise. */
interface Area {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Motes: specks of warm light rising round `around` (the chef's rect),
 * grown by STAGE_FX.motes.spread, each a pure function of GLOBAL time `t`
 * and its index. Only in open (after the chef resolves), morph and end.
 */
export function motes(
  ctx: CanvasRenderingContext2D,
  around: Area,
  t: number,
  alpha = 1,
): void {
  if (alpha <= 0) return;
  const m = STAGE_FX.motes;
  const span = around.h + 2 * m.spread;
  ctx.save();
  for (let i = 0; i < m.count; i++) {
    const x0 = around.x - m.spread + hash(i, 3) * (around.w + 2 * m.spread);
    const rise = (hash(i, 7) * span + t * m.rise) % span;
    const y = around.y + around.h + m.spread - rise;
    // Fading in at the bottom of their climb and out at the top.
    const edge = Math.min(1, rise / 60, (span - rise) / 60);
    const x = x0 + m.sway * Math.sin(TAU * m.swayHz * t + hash(i, 9) * TAU);
    const r = m.rMin + hash(i, 11) * (m.rMax - m.rMin);
    const a = m.alphaMin + hash(i, 13) * (m.alphaMax - m.alphaMin);
    ctx.fillStyle = rgba(m.color, clamp(a * edge * alpha));
    ctx.beginPath();
    ctx.arc(x, y, r, 0, TAU);
    ctx.fill();
  }
  ctx.restore();
}

/**
 * A four-point sparkle (hk's end-card star): two thin crossed diamonds of
 * radius `r` in `color`, with a soft glow, at `alpha`.
 */
export function sparkle(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  r: number,
  alpha: number,
  color = BRIGHT,
): void {
  if (alpha <= 0 || r <= 0) return;
  glow(ctx, x, y, r * 2.6, color, 0.55 * alpha);
  const w = r * 0.16;
  ctx.save();
  ctx.globalAlpha *= clamp(alpha);
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(x, y - r);
  ctx.quadraticCurveTo(x + w, y - w, x + r, y);
  ctx.quadraticCurveTo(x + w, y + w, x, y + r);
  ctx.quadraticCurveTo(x - w, y + w, x - r, y);
  ctx.quadraticCurveTo(x - w, y - w, x, y - r);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}
