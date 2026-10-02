// from jdx/hk@37937824 docs/.vitepress/theme/showreel/score/sounds.ts
// The score's sound palette: small struck and blown voices shared by every
// section, and the effects that mark an accent: air, crackles, blips, the
// shimmer and the riser. Every voice is synthesized; nothing is sampled.
// Each event varies a little, seeded by its own time (`vary`), so repeats
// sound human while every start point still hears the same sounds.
//
// Adapted for mise: only hk's generic voices, without its shanty and props,
// and the pitched runs (shimmer, the riser's fifths) default to F, the
// reel's home key. The pitched voices are in instruments.ts, the sounds of
// the sync points in props.ts, and the music is the bed (bed.ts).

import { BEAT } from "../timeline";
import { hash } from "../math";
import {
  ad,
  type Curve,
  hz,
  type Mix,
  type NoiseKind,
  perc,
  type Pt,
  sweep,
  type VoiceOpts,
  X,
} from "./mix";

// Sound palette. Lowpass and highpass Q values are resonance in dB, as the
// Web Audio spec defines them; bandpass Q is the usual bandwidth ratio.

/** A small struck tone with a quick inharmonic shimmer on top. */
export function ping(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  len: number,
  o: VoiceOpts = {},
): void {
  const v = m.voice(perc(t, vel, 0.001, len), o);
  if (!v) return;
  v.osc("sine", f);
  v.osc("sine", f * 2.76, perc(t, 0.25, 0.0005, len * 0.3));
}

/** A tiny mechanical click with a pitched body. */
export function tick(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan = 0,
  send = 0.06,
): void {
  const v = m.voice(perc(t, 0.7 * vel, 0.0004, 0.035), { pan, send });
  if (!v) return;
  v.noise("white", 1, v.filter("bandpass", f * 2.2, 2.2), 1, t + 0.05);
  v.osc("sine", f, perc(t, 0.86, 0.0004, 0.018));
}

/** A weighty low hit: a sine that drops in pitch, with a soft noise slap. */
export function thump(
  m: Mix,
  t: number,
  vel: number,
  f0: number,
  f1: number,
  len: number,
  o: VoiceOpts = {},
): void {
  const v = m.voice(perc(t, 0.6 * vel, 0.0015, len), o);
  if (!v) return;
  v.osc("sine", [
    [t, f0],
    [t + 0.045, f1 * 1.25, "exp"],
    [t + len, f1, "exp"],
  ]);
  v.noise(
    "white",
    perc(t, 0.3, 0.0005, 0.035),
    v.filter("lowpass", 2400, -3),
    1,
    t + 0.05,
  );
}

/** A cork-like pop: a sine that chirps up to its pitch. */
export function pop(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan = 0,
  send = 0.18,
): void {
  const v = m.voice(perc(t, vel, 0.0015, 0.17), { pan, send });
  if (!v) return;
  v.osc("sine", [
    [t, f * 0.42],
    [t + 0.02, f, "exp"],
    [t + 0.12, f * 1.015, "exp"],
  ]);
  v.osc(
    "triangle",
    sweep(t, f * 0.84, t + 0.02, f * 2),
    perc(t, 0.14, 0.0008, 0.05),
  );
}

/** A glassy bell: struck-bar partials over a bright transient. */
export function ding(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan: number,
  len = 1.3,
  send = 0.38,
): void {
  const v = m.voice(perc(t, vel, 0.0008, len), { pan, send });
  if (!v) return;
  const partials = [
    [1, 1, 1],
    [2, 0.3, 0.55],
    [2.76, 0.2, 0.35],
    [5.4, 0.1, 0.18],
    [8.93, 0.05, 0.1],
  ];
  for (const [ratio, a, k] of partials) {
    if (f * ratio > 16000) continue;
    v.osc("sine", f * ratio, k === 1 ? a : perc(t, a, 0.0005, len * k));
  }
  v.noise(
    "white",
    perc(t, 0.12, 0.0005, 0.06),
    v.filter("highpass", 7000, 0),
    1,
    t + 0.08,
  );
}

/** Air: noise through a moving band. Sustained, so a mid-sweep start still hears it. */
export function whoosh(
  m: Mix,
  env: readonly Pt[],
  band: Curve,
  q: number,
  o: VoiceOpts = {},
  kind: NoiseKind = "pink",
): void {
  const v = m.voice(env, { hold: true, ...o });
  if (v) v.noise(kind, 1, v.filter("bandpass", band, q));
}

/** A note of a phrase (motif.ts onReel): start and end in reel seconds, MIDI note, velocity. */
export type Note = readonly [
  start: number,
  end: number,
  midi: number,
  vel: number,
];

// Blips, crackles, glints and the riser.

/** A blip: a sine chirping down a fourth into its pitch. */
export function blip(m: Mix, t: number, f: number, vel: number): void {
  const v = m.voice(perc(t, 1.6 * vel, 0.001, 0.035), { send: 0.1 });
  if (v) v.osc("sine", sweep(t, f * 1.3, t + 0.02, f));
}

/** A per-event variation in [-1, 1]: the same for the same event from any start point. */
function vary(t: number, seed: number): number {
  return hash(Math.round(t * 9973), seed) * 2 - 1;
}

/** Something tearing over `len` seconds: crackle and hiss through a closing band, gated by a slowing sawtooth. */
export function crackle(
  m: Mix,
  t: number,
  len = 0.12,
  vel = 0.2,
  pan = 0,
): void {
  const e = t + len;
  const env: Pt[] = [
    [t, 0],
    [t + 0.01, vel],
    [Math.max(t + 0.02, e - 0.03), 0.7 * vel],
    [e, 0.0001, "exp"],
    [e + 0.004, 0],
  ];
  const v = m.voice(env, { pan, send: 0.12 });
  if (!v) return;
  const am = v.vca(0.6, v.filter("bandpass", sweep(t, 3000, e, 1500), 0.9));
  v.lfo("sawtooth", sweep(t, 120, e, 40), 0.4, am.gain);
  v.noise("crackle", 2.5, am, sweep(t, 2.4, e, 0.8));
  v.noise("white", 0.5, am);
}

/**
 * F minor's pentatonic with no third, F G B♭ C E♭, from F7 up an octave:
 * the shimmer's default run. It has no D♭ a semitone over the bed's
 * ringing C, and no A♭ to rub against the bed's F where its third is A.
 */
const F_RUN: readonly number[] = [101, 103, 106, 108, 111, 113];

/** A glint from `t0` to `t1`: tiny bells running up `notes` (MIDI), over a breath of air. */
export function shimmer(
  m: Mix,
  t0: number,
  t1: number,
  vel = 0.05,
  pan = 0.1,
  notes: readonly number[] = F_RUN,
): void {
  notes.forEach((n, i) => {
    const t =
      t0 + ((t1 - t0) * i) / notes.length + 0.005 * (1 + vary(t0, 60 + i));
    ping(m, t, hz(n), vel * (1 - 0.06 * i), 0.18, {
      pan: pan + 0.3 * vary(t0, 70 + i),
      send: 0.4,
    });
  });
  const a = m.voice(ad(t0, (t0 + t1) / 2, 0.8 * vel, t1 + 0.2), {
    send: 0.4,
    pan,
    hold: true,
  });
  if (a)
    a.noise("white", 1, a.filter("highpass", sweep(t0, 6000, t1, 9000), 0));
}

export interface RiserOpts {
  vel?: number;
  /** Times at which the roll's gate doubles its rate: eighths, then sixteenths, then 32nds... */
  doubles?: readonly number[];
  /** Rising fifths under the noise; true by default. */
  fifths?: boolean;
  /** The gated noise roll; true by default. */
  roll?: boolean;
  /**
   * The fifths' lower note at `t0`, MIDI: they rise an octave from it, the
   * fifth above it on top. C3 by default, the dominant of F minor.
   */
  from?: number;
}

/**
 * A build that cuts dead at `t1`: noise rising through a band, fifths
 * rising an octave (G over C by default, into the F of the downbeat after), and a noise
 * roll whose gate opens every eighth and doubles its rate at each of
 * `doubles`. Each rate runs whole cycles from `t0`, so a start in the middle
 * picks the gate up on its grid. With `fifths` and `roll` off it is a plain
 * filtered-noise riser.
 */
export function riser(m: Mix, t0: number, t1: number, o: RiserOpts = {}): void {
  const vel = o.vel ?? 1;
  const cut = (a: number, peak: number): Pt[] => [
    [t0, 0],
    [t0 + Math.min(0.25, (t1 - t0) / 3), a * vel],
    [t1 - 0.006, peak * vel, "exp"],
    [t1, 0.0001, "exp"],
    [t1 + 0.003, 0],
  ];
  const n = m.voice(cut(0.012, 0.2), { send: 0.05, hold: true });
  if (n)
    n.noise("white", 1, n.filter("bandpass", sweep(t0, 400, t1, 7000), 1.1));
  if (o.fifths !== false) {
    const f = m.voice(cut(0.02, 0.16), { send: 0.06, hold: true });
    if (f) {
      const lp = f.filter("lowpass", sweep(t0, 400, t1, 5000), 3);
      const root = o.from ?? 48;
      for (const det of [-10, 10])
        f.osc("sawtooth", sweep(t0, hz(root), t1, hz(root + 12)), 0.5, lp, det);
      f.osc("sawtooth", sweep(t0, hz(root + 7), t1, hz(root + 19)), 0.35, lp);
    }
  }
  if (o.roll !== false) {
    const doubles = o.doubles ?? [];
    const r = m.voice(cut(0.01, 0.2), { send: 0.04, hold: true, pan: 0.05 });
    if (r) {
      const am = r.vca(
        0.5,
        r.filter("bandpass", sweep(t0, 900, t1, 3600), 0.9),
      );
      const period = (t: number) =>
        (2 * X) / 2 ** doubles.filter((d) => d <= t + 1e-9).length;
      const cycle = (t: number) =>
        t0 + Math.ceil((t - t0) / period(t) - 1e-9) * period(t);
      const rate: Pt[] = [
        [t0, 2 / BEAT],
        ...doubles.map((d, i): Pt => [d, 2 ** (i + 2) / BEAT, "set"]),
      ];
      r.lfo("square", rate, 0.5, am.gain, cycle);
      r.noise("white", 1, am);
    }
  }
}

// The air a moving thing pushes aside as it lands.

/**
 * Air a moving thing pushes aside as it lands: a soft low breath and a few
 * grains of dust settling after it.
 */
export function puff(
  m: Mix,
  t: number,
  vel: number,
  pan = 0,
  dust = true,
): void {
  const a = m.voice(perc(t, vel, 0.004, 0.14), { pan, send: 0.18 });
  if (a)
    a.noise("pink", 1, a.filter("lowpass", sweep(t, 1400, t + 0.14, 300), 0));
  if (!dust) return;
  const d = m.voice(ad(t + 0.01, t + 0.04, 0.5 * vel, t + 0.3), {
    pan,
    send: 0.2,
  });
  if (d) d.noise("crackle", 1.2, d.filter("bandpass", 3200, 0.8), 0.6);
}
