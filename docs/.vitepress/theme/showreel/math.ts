// from jdx/hk@37937824 docs/.vitepress/theme/showreel/math.ts
// Timing, easing, and deterministic randomness for the reel. Every frame is a
// pure function of time, so nothing here keeps state between calls.

export type Ease = (t: number) => number;

export const clamp = (v: number, lo = 0, hi = 1): number =>
  v < lo ? lo : v > hi ? hi : v;
export const lerp = (a: number, b: number, t: number): number =>
  a + (b - a) * t;
/** Progress of `v` from `a` to `b`, clamped to [0, 1]. */
export const progress = (a: number, b: number, v: number): number =>
  b === a ? (v >= b ? 1 : 0) : clamp((v - a) / (b - a));
export const smoothstep = (a: number, b: number, v: number): number => {
  const t = progress(a, b, v);
  return t * t * (3 - 2 * t);
};
export const TAU = Math.PI * 2;
export const DEG = Math.PI / 180;

// Penner curves plus a few designer curves.
export const linear: Ease = (t) => t;
export const inCubic: Ease = (t) => t * t * t;
export const outCubic: Ease = (t) => 1 - (1 - t) ** 3;
export const outQuart: Ease = (t) => 1 - (1 - t) ** 4;
export const inOutSine: Ease = (t) => -(Math.cos(Math.PI * t) - 1) / 2;
export const outBack =
  (s = 1.70158): Ease =>
  (t) =>
    1 + (s + 1) * (t - 1) ** 3 + s * (t - 1) ** 2;

/** CSS-style cubic-bezier timing function. */
export function cubicBezier(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): Ease {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const sampleX = (s: number) => ((ax * s + bx) * s + cx) * s;
  const sampleY = (s: number) => ((ay * s + by) * s + cy) * s;
  const slopeX = (s: number) => (3 * ax * s + 2 * bx) * s + cx;
  return (x) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let s = x;
    for (let i = 0; i < 6; i++) {
      const err = sampleX(s) - x;
      const d = slopeX(s);
      if (Math.abs(err) < 1e-6) return sampleY(s);
      if (Math.abs(d) < 1e-6) break;
      s -= err / d;
    }
    let lo = 0;
    let hi = 1;
    s = x;
    for (let i = 0; i < 24; i++) {
      const v = sampleX(s);
      if (Math.abs(v - x) < 1e-6) break;
      if (v < x) lo = s;
      else hi = s;
      s = (lo + hi) / 2;
    }
    return sampleY(s);
  };
}

/** Fast out, long soft landing: the house curve for things arriving. */
export const swiftOut = cubicBezier(0.16, 1, 0.3, 1);
/** Slow start, decisive finish: the house curve for things leaving. */
export const swiftIn = cubicBezier(0.7, 0, 0.84, 0);
/** Symmetric, weighty move between two resting states. */
export const swiftInOut = cubicBezier(0.83, 0, 0.17, 1);

/**
 * Damped oscillation around zero after an impulse at `start`: a wobble that
 * begins at full amplitude and settles. Useful for jiggles and follow-through.
 */
export function wobble(t: number, start: number, freq = 4, decay = 6): number {
  const d = t - start;
  if (d < 0) return 0;
  return Math.exp(-decay * d) * Math.sin(TAU * freq * d);
}

/** Attack/decay envelope peaking at `at`, zero outside. */
export function pulse(
  t: number,
  at: number,
  attack = 0.02,
  decay = 0.25,
): number {
  if (t < at - attack || t > at + decay * 6) return 0;
  if (t < at) return attack <= 0 ? 1 : progress(at - attack, at, t);
  return Math.exp(-(t - at) / decay);
}

export type Key = readonly [time: number, value: number, ease?: Ease];

/**
 * Keyframe track: `keys([[0, 0], [0.5, 1, outBack()], [1, 0]])`. A key's
 * easing applies to the segment arriving at it. Holds before the first key
 * and after the last.
 */
export function keys(frames: readonly Key[]): (t: number) => number {
  return (t) => {
    if (t <= frames[0][0]) return frames[0][1];
    for (let i = 1; i < frames.length; i++) {
      const [t1, v1, ease = linear] = frames[i];
      if (t <= t1) {
        const [t0, v0] = frames[i - 1];
        return lerp(v0, v1, ease(progress(t0, t1, t)));
      }
    }
    return frames[frames.length - 1][1];
  };
}

/** Stagger helper: the local progress of item `i` of `n` in a cascade. */
export function stagger(
  t: number,
  start: number,
  each: number,
  duration: number,
  i: number,
  ease: Ease = linear,
): number {
  return ease(progress(start + i * each, start + i * each + duration, t));
}

// Deterministic randomness.

/** Integer hash to [0, 1). The same inputs always give the same value. */
export function hash(n: number, seed = 0): number {
  let h =
    (Math.imul(n | 0, 0x27d4eb2d) ^ Math.imul(seed | 0, 0x165667b1)) >>> 0;
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/** Seeded PRNG for building fixed layouts once. */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
