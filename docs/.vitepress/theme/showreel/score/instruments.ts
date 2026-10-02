// The pitched voices the sound design strikes its stingers on: strings
// plucked, brass, mallets, a celesta, an organ, and a kick. They were the
// synthesized band's (plan v3 §5: pizzicato, brass and marimba for Acts 0
// to II, the organ for III, a voice per task lane for IV), which the bed
// (bed.ts) has replaced; each is now only ever an effect, on the effects
// bus, over the bed.
//
// Every voice is synthesized; nothing is sampled. Each event varies a
// little, seeded by its own time, so repeats sound played while every start
// point still hears the same sounds.

import { hash } from "../math";
import {
  type Curve,
  hold,
  hz,
  type Mix,
  perc,
  type Pt,
  sweep,
  type Voice,
} from "./mix";
import { thump } from "./sounds";

/** A per-event variation in [-1, 1]: the same for the same event from any start point. */
const vary = (t: number, seed: number): number =>
  hash(Math.round(t * 9973), seed) * 2 - 1;

/** A periodic wave from harmonic amplitudes (index 1 is the fundamental), cached per context. */
function waveOf(
  cache: WeakMap<BaseAudioContext, PeriodicWave>,
  ac: BaseAudioContext,
  amp: (k: number) => number,
  n = 40,
): PeriodicWave {
  let w = cache.get(ac);
  if (!w) {
    const real = new Float32Array(n);
    const imag = new Float32Array(n);
    for (let k = 1; k < n; k++) imag[k] = amp(k);
    w = ac.createPeriodicWave(real, imag);
    cache.set(ac, w);
  }
  return w;
}

/**
 * An LFO's wave for a voice that enters mid-sound: the harmonics `amp`,
 * started `phase` radians into their cycle, so an LFO started where
 * playback joins the voice is in step with one started at the voice's
 * start. Only a mid-reel start makes one.
 */
function phasedWave(
  ac: BaseAudioContext,
  amp: (k: number) => number,
  phase: number,
  n: number,
): PeriodicWave {
  const real = new Float32Array(n);
  const imag = new Float32Array(n);
  for (let k = 1; k < n; k++) {
    real[k] = amp(k) * Math.sin(k * phase);
    imag[k] = amp(k) * Math.cos(k * phase);
  }
  return ac.createPeriodicWave(real, imag);
}

/**
 * A steady LFO on `target` at `rate` Hz from the voice's start: if
 * playback joins the voice mid-sound, the LFO starts there already in
 * step, so a render from any point hears the same wobble from then on.
 */
function steadyLfo(
  v: Voice,
  cache: WeakMap<BaseAudioContext, PeriodicWave>,
  amp: (k: number) => number,
  n: number,
  rate: number,
  depth: Curve,
  target: AudioParam,
): void {
  const late = v.enter + v.shift - v.t0;
  const phase = 2 * Math.PI * ((rate * late) % 1);
  const wave =
    late > 1e-6
      ? phasedWave(v.m.ac, amp, phase, n)
      : waveOf(cache, v.m.ac, amp, n);
  v.lfo(wave, rate, depth, target);
}

const sines = new WeakMap<BaseAudioContext, PeriodicWave>();
const SINE = (k: number): number => (k === 1 ? 1 : 0);

// Strings.

export interface PluckOpts {
  /** Vibrato depth as a fraction of the pitch. */
  wobble?: number;
  /** How long the string rings, in seconds. */
  len?: number;
  /** The first brightness, as a multiple of the pitch (7 by default). */
  bright?: number;
}

/**
 * A pizzicato string (hk's fiddle pluck): plucked a touch sharp and
 * settling to pitch, its brightness closing fast, through the body's air
 * and wood resonances, with the finger's snap on top. Higher strings ring
 * shorter.
 */
export function pizz(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan: Curve = 0.2,
  send = 0.2,
  o: PluckOpts = {},
): void {
  const len = o.len ?? Math.min(0.5, 0.18 + 60 / f);
  const v = m.voice(perc(t, vel, 0.002, len), {
    pan,
    send,
  });
  if (!v) return;
  // The body's resonances, kept gentle: with the cello's and the organ's,
  // stronger ones piled 8 dB more 150 to 400 Hz into Acts 0 to II than
  // hk's reel has, and the band read boxy.
  const air = v.filter("peaking", 280, 1.6);
  air.gain.value = 1;
  const body = v.filter("peaking", 450, 1.4, air);
  body.gain.value = 2;
  const lp = v.filter(
    "lowpass",
    [
      [t, Math.min(14000, (o.bright ?? 7) * f)],
      [t + 0.15, 1.6 * f, "exp"],
    ],
    4,
    body,
  );
  const pitch: Pt[] = [
    [t, f * 1.008],
    [t + 0.03, f, "exp"],
  ];
  const saw = v.osc("sawtooth", pitch, 0.55, lp);
  const tri = v.osc("triangle", pitch, 0.45, lp);
  if (o.wobble) {
    const depth = sweep(t, f * o.wobble, t + len, f * o.wobble * 0.15);
    v.lfo("sine", 14, depth, saw.frequency);
    v.lfo("sine", 14, depth, tri.frequency);
  }
  v.noise(
    "white",
    perc(t, 0.35, 0.0005, 0.01),
    v.filter("bandpass", 2500, 1.2),
    1,
    t + 0.02,
  );
}

/** A chord plucked across the strings, low to high, 4 ms a string. */
export function pizzChord(
  m: Mix,
  t: number,
  notes: readonly number[],
  vel: number,
  pan = 0,
  o: PluckOpts & { send?: number } = {},
): void {
  const each = vel / Math.sqrt(notes.length);
  notes.forEach((n, i) =>
    pizz(
      m,
      t + 0.004 * i,
      hz(n),
      each * (1 + 0.06 * vary(t, 10 + i)),
      pan + 0.08 * (i - (notes.length - 1) / 2),
      o.send ?? 0.2,
      o,
    ),
  );
}

// Brass.

export interface BrassOpts {
  pan?: number;
  send?: number;
  /** Seconds the lips take to reach pressure. */
  attack?: number;
  /** Where the tone has settled by the note's end, as a fraction of its peak. */
  sustain?: number;
  release?: number;
  /** The bell's brightest, Hz. */
  bright?: number;
  /** A cup mute: nasal, with the lows taken away. */
  mute?: boolean;
}

/**
 * A brass section sounding `notes` from `t0` to `t1`: two sawtooths a note,
 * a few cents apart, scooping up to pitch, through a lowpass that opens as
 * the lips reach pressure and settles back (the brass "blat"), and the
 * horn's formant near 1.2 kHz. A breath of air at the start.
 */
export function brass(
  m: Mix,
  t0: number,
  t1: number,
  notes: readonly number[],
  vel: number,
  o: BrassOpts = {},
): void {
  const attack = o.attack ?? 0.02;
  const env = hold(
    t0,
    attack,
    vel,
    t1,
    (o.sustain ?? 0.6) * vel,
    o.release ?? 0.08,
  );
  const v = m.voice(env, {
    pan: o.pan ?? 0,
    send: o.send ?? 0.2,
    hold: true,
  });
  if (!v) return;
  const top = o.bright ?? 3400;
  let into: AudioNode = v.amp;
  if (o.mute) {
    const nose = v.filter("peaking", 1700, 2.2, v.amp);
    nose.gain.value = 7;
    into = v.filter("highpass", 420, 0, nose);
  }
  const formant = v.filter("peaking", 1150, 1.3, into);
  formant.gain.value = 4;
  const settle = Math.max(t0 + attack + 0.01, Math.min(t1, t0 + attack + 0.2));
  const lp = v.filter(
    "lowpass",
    [
      [t0, top * 0.25],
      [t0 + attack, top, "exp"],
      [settle, top * 0.5, "exp"],
    ],
    1.5,
    formant,
  );
  const each = 0.5 / Math.sqrt(notes.length);
  for (const n of notes) {
    const f = hz(n);
    const scoop: Pt[] = [
      [t0, f * 0.985],
      [t0 + 0.035, f, "exp"],
    ];
    v.osc("sawtooth", scoop, each, lp, -6);
    v.osc("sawtooth", scoop, each, lp, 5);
  }
  v.noise(
    "pink",
    perc(t0, 0.05, 0.003, 0.05),
    v.filter("bandpass", 1500, 1, lp),
    1,
    t0 + 0.08,
  );
}

// Mallets.

/**
 * A marimba bar: the fundamental with the bar's tuned fourth partial (two
 * octaves up, a touch flat) and a faint tenth, all dying faster than the
 * fundamental, and the mallet's soft knock. Low bars ring longer.
 */
export function marimba(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan = 0,
  o: { len?: number; send?: number } = {},
): void {
  const len = o.len ?? Math.min(1.1, 0.16 + 80 / f);
  const v = m.voice(perc(t, vel, 0.002, len), {
    pan,
    send: o.send ?? 0.22,
  });
  if (!v) return;
  v.osc("sine", f);
  v.osc("sine", f * 3.93, perc(t, 0.3, 0.001, len * 0.22));
  if (f * 9.2 < 15000)
    v.osc("sine", f * 9.2, perc(t, 0.08, 0.0008, len * 0.07));
  v.noise(
    "white",
    perc(t, 0.22, 0.0005, 0.012),
    v.filter("bandpass", Math.min(5000, f * 4), 1.4),
    1,
    t + 0.03,
  );
}

/**
 * A woodblock: a hollow wooden knock, pitched, with its second mode and the
 * stick's tick: the lint lane's voice (lanes.ts), like a knife on a board.
 */
export function woodblock(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan = 0,
  send = 0.12,
): void {
  const v = m.voice(perc(t, vel, 0.0005, 0.075), { pan, send });
  if (!v) return;
  v.osc("sine", sweep(t, f * 1.05, t + 0.012, f));
  v.osc("sine", f * 2.63, perc(t, 0.3, 0.0005, 0.025));
  v.noise(
    "white",
    perc(t, 0.5, 0.0003, 0.008),
    v.filter("bandpass", f * 1.7, 3),
    1,
    t + 0.02,
  );
}

/**
 * A celesta: a struck steel plate over a wooden resonator, its partials
 * the octave and a faint second octave, dying fast under a long, pure
 * fundamental, with the felt hammer's touch.
 */
export function celesta(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan = 0,
  len = 1.4,
  send = 0.4,
): void {
  const v = m.voice(perc(t, vel, 0.0015, len), { pan, send });
  if (!v) return;
  v.osc("sine", f);
  if (f * 2 < 15000) v.osc("sine", f * 2, perc(t, 0.2, 0.001, len * 0.3));
  if (f * 4.02 < 15000)
    v.osc("sine", f * 4.02, perc(t, 0.08, 0.0008, len * 0.1));
  v.noise(
    "white",
    perc(t, 0.05, 0.0005, 0.01),
    v.filter("lowpass", 5000, 0),
    1,
    t + 0.02,
  );
}

const organs = new WeakMap<BaseAudioContext, PeriodicWave>();

/**
 * A drawbar organ's wave: 8', 4', 2⅔' and 2' pulled out, with a trace of
 * the higher footages. The 16' is a sine an octave down, beside it.
 */
function organOf(ac: BaseAudioContext): PeriodicWave {
  const bars: Record<number, number> = {
    1: 1,
    2: 0.6,
    3: 0.5,
    4: 0.28,
    6: 0.1,
    8: 0.07,
  };
  return waveOf(organs, ac, (k) => bars[k] ?? 0, 12);
}

export interface OrganOpts {
  send?: number;
  attack?: number;
  release?: number;
  /** The rotor: "slow" (chorale, about 0.8 Hz) or "fast" (tremolo, about 6.5 Hz). */
  speed?: "slow" | "fast";
  /** The lowpass the cabinet leaves: lower for a pad under the tune. */
  bright?: number;
  /** The 16' drawbar under it, 0 to 1. */
  sub?: number;
  /** The key's click on the attack. */
  click?: boolean;
}

/**
 * An organ chord from `t0` to `t1`: every note on the drawbar wave and its
 * 16', through a rotating speaker, played as two voices panned apart whose
 * horns turn a little out of step, so the chord swims across the field. The
 * key click marks the attack. Chords sit near a single note's level.
 */
export function organ(
  m: Mix,
  t0: number,
  t1: number,
  notes: readonly number[],
  vel: number,
  o: OrganOpts = {},
): void {
  const each = vel / 2;
  const rate = o.speed === "fast" ? 6.4 : 0.8;
  const wave = organOf(m.ac);
  [-1, 1].forEach((side, k) => {
    const v = m.voice(
      hold(t0, o.attack ?? 0.012, each, t1, each, o.release ?? 0.09),
      {
        pan: side * 0.45,
        send: o.send ?? 0.28,
        hold: true,
      },
    );
    if (!v) return;
    const r = rate * (k ? 1.09 : 1);
    const am = v.vca(0.82);
    // The rotor turns from t0; a start mid-chord joins it in step.
    steadyLfo(v, sines, SINE, 2, r, 0.18, am.gain);
    const lp = v.filter("lowpass", o.bright ?? 3400, 0, am);
    const per = 1 / Math.sqrt(notes.length);
    for (const n of notes) {
      const f = hz(n);
      const o8 = v.osc(wave, f, 0.5 * per, lp, side * 3);
      steadyLfo(v, sines, SINE, 2, r, f * 0.0018, o8.frequency);
      if (o.sub ?? 0.4) v.osc("sine", f / 2, 0.5 * per * (o.sub ?? 0.4), lp);
    }
    if (o.click !== false && k === 0)
      v.noise(
        "white",
        perc(t0, 0.08, 0.0003, 0.006),
        v.filter("bandpass", 2600, 1.2),
        1,
        t0 + 0.02,
      );
  });
}

// Drums.

/**
 * A kick: a falling sine and the beater's click. The toque's drop
 * (morph.ts) is the reel's one.
 */
export function kick(m: Mix, t: number, vel = 1): void {
  const k = 1 + 0.03 * vary(t, 21);
  thump(m, t, 0.95 * vel, 135 * k, 45, 0.3);
  const c = m.voice(perc(t, 0.22 * vel, 0.0004, 0.012));
  if (c) c.noise("white", 1, c.filter("bandpass", 3200, 1.2), 1, t + 0.02);
}
