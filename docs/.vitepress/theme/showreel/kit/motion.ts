// from jdx/hk@37937824 docs/.vitepress/theme/showreel/kit/motion.ts
// Motion every scene shares, on the caller's clock (seconds): the
// terminal cursor's blink, thrown arcs and the sparks that travel them,
// and settles that land exactly on their mark, so a scene rests on its
// handoff frame to the pixel. Every function is a pure function of its
// arguments.
//
// Adapted for mise: the reel's motion vocabulary (ART.md §13, the numbers in
// kit/style.ts EASE, DUR and MOTION) as helpers on a section's BEATS: named
// eases over a span, entrances and exits, anticipation, the slam of kinetic
// type, a damped swing, a hop, and a capped stagger. Each is exactly at rest
// (0 or 1, no residue) outside its span, so nothing crosses a bar line.

import { PALETTE } from "../bible";
import { mix, rgba } from "../color";
import { glow } from "../fx";
import { clamp, lerp, progress, TAU } from "../math";
import { BEAT } from "../timeline";
import { DUR, EASE, type EaseName, MOTION } from "./style";

export interface Pt {
  x: number;
  y: number;
}
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A rect tweened toward another. */
export const lerpRect = (a: Rect, b: Rect, k: number): Rect => ({
  x: lerp(a.x, b.x, k),
  y: lerp(a.y, b.y, k),
  w: lerp(a.w, b.w, k),
  h: lerp(a.h, b.h, k),
});

/**
 * A terminal's block cursor, blinking with the beat: on for a beat, off for
 * the next. Pass GLOBAL `t`, so a cursor that crosses a bar line keeps its
 * phase: on while floor(t / BEAT) is even.
 */
export const cursorOn = (t: number): boolean =>
  Math.floor(t / BEAT + 1e-9) % 2 === 0;

// Thrown arcs.

/** A quadratic Bézier: a thrown arc or a gentle bend. */
export interface Curve {
  a: Pt;
  c: Pt;
  b: Pt;
}

/** An arc from a to b bulging `lift` of its length to the left of travel (up for a rightward throw). */
export function arc(a: Pt, b: Pt, lift = 0.25): Curve {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const flip = dx < 0 ? -1 : 1;
  return {
    a,
    b,
    c: {
      x: (a.x + b.x) / 2 + dy * lift * flip,
      y: (a.y + b.y) / 2 - Math.abs(dx) * lift,
    },
  };
}

export function curveAt(k: Curve, u: number): Pt {
  const v = 1 - u;
  return {
    x: v * v * k.a.x + 2 * v * u * k.c.x + u * u * k.b.x,
    y: v * v * k.a.y + 2 * v * u * k.c.y + u * u * k.b.y,
  };
}

/** The direction of travel at u, not normalized. */
export function curveTangent(k: Curve, u: number): Pt {
  return {
    x: 2 * (1 - u) * (k.c.x - k.a.x) + 2 * u * (k.b.x - k.c.x),
    y: 2 * (1 - u) * (k.c.y - k.a.y) + 2 * u * (k.b.y - k.c.y),
  };
}

export interface SparkOptions {
  color?: string;
  /** Head radius px. */
  size?: number;
  /** Tail length along the curve, in u. */
  trail?: number;
  alpha?: number;
}

/** A spark travelling a curve, its head at u with a fading tail: a key handed on, a pulse. */
export function drawSpark(
  ctx: CanvasRenderingContext2D,
  k: Curve,
  u: number,
  o: SparkOptions = {},
): void {
  const a = o.alpha ?? 1;
  if (a <= 0 || u < 0 || u > 1) return;
  const color = o.color ?? PALETTE.paper;
  const size = o.size ?? 7;
  const trail = o.trail ?? 0.18;
  const head = curveAt(k, u);
  ctx.save();
  ctx.globalAlpha *= a;
  glow(ctx, head.x, head.y, size * 6, color, 0.7);
  const n = 10;
  ctx.lineCap = "round";
  for (let i = n; i >= 1; i--) {
    const p0 = curveAt(k, Math.max(0, u - (trail * i) / n));
    const p1 = curveAt(k, Math.max(0, u - (trail * (i - 1)) / n));
    ctx.strokeStyle = rgba(color, 0.9 * (1 - i / (n + 1)));
    ctx.lineWidth = size * 1.6 * (1 - i / (n + 1));
    ctx.beginPath();
    ctx.moveTo(p0.x, p0.y);
    ctx.lineTo(p1.x, p1.y);
    ctx.stroke();
  }
  ctx.fillStyle = mix(color, "#ffffff", 0.6);
  ctx.beginPath();
  ctx.arc(head.x, head.y, size, 0, TAU);
  ctx.fill();
  ctx.restore();
}

// Settles that are home, to the last bit, when they say they are.

/**
 * 0 before `at`, then up to 1 over `dur` seconds, overshooting by about
 * `over` of the way (a back-out curve) and landing exactly on 1: a pop or a
 * snap that is home when it says it is.
 */
export function land(t: number, at: number, dur: number, over = 0.12): number {
  const p = progress(at, at + dur, t);
  if (p <= 0 || p >= 1) return p;
  // outBack with the overshoot solved for: s 1.70158 gives 10%.
  const s = 1.70158 * (over / 0.1);
  return 1 + (s + 1) * (p - 1) ** 3 + s * (p - 1) ** 2;
}

/** A bump up from 0 and back to exactly 0: sin² over [at, at + dur]. */
export function bump(t: number, at: number, dur: number): number {
  const p = progress(at, at + dur, t);
  return p <= 0 || p >= 1 ? 0 : Math.sin(Math.PI * p) ** 2;
}

// The motion vocabulary, on a section's beats (ART.md §13).

/** Eased progress over beats `at` to `at + dur`: 0 before, 1 after, `ease` between. */
export function span(
  b: number,
  at: number,
  dur: number,
  ease: EaseName = "arrive",
): number {
  const p = progress(at, at + dur, b);
  return p <= 0 ? 0 : p >= 1 ? 1 : EASE[ease](p);
}

/** How an actor stands while entering or leaving: its opacity, and how far it is off its mark (px, +down). */
export interface Presence {
  alpha: number;
  dy: number;
}

/**
 * A window, card or mark entering at beat `at`: rising MOTION.riseIn px
 * and fading in over DUR.enter (arrive), then exactly at rest.
 */
export function enter(
  b: number,
  at: number,
  dur: number = DUR.enter,
): Presence {
  const k = span(b, at, dur, "arrive");
  return { alpha: k, dy: MOTION.riseIn * (1 - k) };
}

/**
 * The same actor leaving from beat `at`: falling MOTION.fallOut px (leave,
 * accelerating away) and fading out over DUR.exit (glide). Exactly at rest
 * before `at`. The fade is not on `leave`: steepest at its end, it took a
 * third of the actor's opacity in the last frame, so every exit ended in a
 * pop.
 */
export function exit(b: number, at: number, dur: number = DUR.exit): Presence {
  const k = span(b, at, dur, "leave");
  return { alpha: 1 - span(b, at, dur, "glide"), dy: MOTION.fallOut * k };
}

/** Up from `a` to `z` (beats): an entrance at `a`, an exit ending on `z`. */
export function presence(
  b: number,
  a: number,
  z: number,
  o: { enter?: number; exit?: number } = {},
): Presence {
  const x = o.exit ?? DUR.exit;
  if (b < a || b >= z) return { alpha: 0, dy: 0 };
  const i = enter(b, a, o.enter ?? DUR.enter);
  const e = exit(b, z - x, x);
  return { alpha: i.alpha * e.alpha, dy: i.dy + e.dy };
}

/**
 * Anticipation before a weighty move at beat `at`: 0 at rest, dipping to 1
 * (the full MOTION.anticipate px, or 2–3 %, opposite the move) over the
 * DUR.anticipate before `at` (wind), and back to 0 on `at` exactly, when
 * the move itself takes over.
 */
export function anticipate(
  b: number,
  at: number,
  dur: number = DUR.anticipate,
): number {
  const p = progress(at - dur, at, b);
  if (p <= 0 || p >= 1) return 0;
  // Down on the wind curve, back up in the last fifth.
  return p < 0.8 ? EASE.wind(p / 0.8) : 1 - EASE.settle((p - 0.8) / 0.2);
}

/** Kinetic type slamming in: its scale, opacity, and the impact's strength (0 to 1). */
export interface Slam {
  scale: number;
  alpha: number;
  /** The impact glow's strength: 1 on the landing, decaying over half a beat. */
  impact: number;
  /** 0 while approaching, 1 from the landing on. */
  landed: number;
}

/**
 * The slam of kinetic type landing on beat `land` (ART.md §12.10): the
 * approach from MOTION.slamFrom× and transparent to 1× over DUR.slam
 * (wind, accelerating in: the anticipation), the impact on the beat (a
 * squash to 96.5 % and a glow that decays over half a beat), and the settle
 * back to 1× over the next 1/8 beat (settle). Exactly 1× from then on.
 */
export function slam(b: number, land: number): Slam {
  const d = DUR.slam;
  if (b < land - d)
    return { scale: MOTION.slamFrom, alpha: 0, impact: 0, landed: 0 };
  if (b < land) {
    const k = EASE.wind(progress(land - d, land, b));
    return {
      scale: lerp(MOTION.slamFrom, 1, k),
      alpha: clamp(k * 1.6),
      impact: 0,
      landed: 0,
    };
  }
  const since = b - land;
  const settle = progress(0, DUR.flick, since);
  const squash = settle >= 1 ? 0 : 0.035 * (1 - EASE.settle(settle));
  const decay = progress(0, DUR.half, since);
  return {
    scale: 1 - squash,
    alpha: 1,
    impact: decay >= 1 ? 0 : (1 - decay) ** 2,
    landed: 1,
  };
}

/**
 * A damped swing on a hinge, starting at full swing `deg` on beat `at` and
 * decaying with time constant `tau` SECONDS; exactly 0 from `until`.
 */
export function swing(
  b: number,
  at: number,
  deg: number,
  tau: number,
  until: number,
  freq = 1.6,
): number {
  if (b < at || b >= until) return 0;
  const s = (b - at) * BEAT;
  const fade = 1 - progress(at + (until - at) * 0.7, until, b);
  return deg * Math.exp(-s / tau) * Math.cos(TAU * freq * s) * fade;
}

/** A hop along an arc: how high (0 to 1, times the hop's height) at progress `k`. */
export const hop = (k: number): number =>
  k <= 0 || k >= 1 ? 0 : 4 * k * (1 - k);

/**
 * Item `i` of `n` in a cascade from beat `at`, each taking `dur`: its eased
 * progress, the items DUR.stagger apart, capped so the whole cascade starts
 * within DUR.staggerMax.
 */
export function cascade(
  b: number,
  at: number,
  i: number,
  n: number,
  dur: number,
  ease: EaseName = "arrive",
): number {
  const each = n > 1 ? Math.min(DUR.stagger, DUR.staggerMax / (n - 1)) : 0;
  return span(b, at + i * each, dur, ease);
}
