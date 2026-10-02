// The reel's props: the sounds its sync points make (plan v3 §5, ART.md
// §13). A ticket's printer ticks, feeds and tears; a line seats with a
// click; keys are soft clicks and Tab a soft pop; a stamp thunks; a bad
// input gets a dissonant brass stab; Postgres lights like a pilot flame; a
// checkpoint is a tape deck's click and a rollback its rewind; big type
// slams; an arrival rings the kitchen's service bell; and the name writes
// on, on the celesta. Stage positions
// map to the stereo field with panX, so a sound sits where its picture is.

import { hash } from "../math";
import { brass, celesta } from "./instruments";
import { ad, hz, line, type Mix, perc, sweep } from "./mix";
import { crackle, thump, tick, whoosh } from "./sounds";

/** A per-event variation in [-1, 1]: the same for the same event from any start point. */
const vary = (t: number, seed: number): number =>
  hash(Math.round(t * 9973), seed) * 2 - 1;

/** Where the reel's layout sits in the stereo field (panX of LAYOUT's columns). */
export const PAN = {
  /** The terminal column, x 160 to 1100. */
  term: -0.24,
  /** The card column, x 1140 to 1760. */
  card: 0.36,
  /** A ticket centred on the stage. */
  center: 0,
  /** The keycaps, by the terminal's right edge. */
  key: 0.06,
} as const;

/**
 * How far the music dips under an Act III click, the quiet act's: deeper
 * than a seat's own duck, so its soft clicks speak (vars.ts, redact.ts).
 */
export const ACT3_DUCK = 0.35;

/**
 * The name, "mise-en-place", as the open and the end card write it on, a
 * syllable each: C D♭ C (the chorus's "It's mise-en-place", motif.ts
 * CHORUS e 96 to 100), two octaves up.
 */
const NAME = [84, 85, 84];

/** Syllable `i` of the name writing on at `t`, on the celesta. */
export function writeName(m: Mix, t: number, i: number, pan: number): void {
  celesta(m, t, hz(NAME[i % 3]), 0.05, pan, 1.2, 0.4);
}

/**
 * One step of a thermal printer's feed: the stepper's short buzz and the
 * paper's tick, a little different each step.
 */
export function printer(m: Mix, t: number, vel: number, pan = 0): void {
  const v = m.voice(perc(t, vel, 0.0005, 0.026), { pan, send: 0.05 });
  if (!v) return;
  const f = 1150 * (1 + 0.04 * vary(t, 31));
  v.osc("square", f, 0.3, v.filter("bandpass", 1900, 2.5));
  v.noise("white", 1, v.filter("bandpass", 4300, 3), 1, t + 0.03);
}

/** A ticket feeding out of the rail in `steps` printer steps from `t0` to `t1`. */
export function feed(
  m: Mix,
  t0: number,
  t1: number,
  steps: number,
  vel: number,
  pan = 0,
): void {
  for (let k = 0; k < steps; k++)
    printer(
      m,
      t0 + ((t1 - t0) * k) / steps,
      vel * (0.85 + 0.15 * (k % 2)),
      pan,
    );
}

/** The ticket torn off along its teeth: a short rip and a flick of paper. */
export function tear(m: Mix, t: number, vel: number, pan = 0): void {
  crackle(m, t - 0.03, 0.09, 0.7 * vel, pan);
  whoosh(
    m,
    ad(t - 0.04, t, 0.4 * vel, t + 0.08),
    sweep(t - 0.04, 5200, t + 0.08, 2600),
    2.4,
    { pan, send: 0.1, hold: false },
    "white",
  );
}

/** Something light rising off the stage, or arriving on it: air. `up` rises, else it settles. */
export function air(
  m: Mix,
  t0: number,
  t1: number,
  vel: number,
  pan0 = 0,
  pan1 = pan0,
  up = true,
): void {
  const band = up ? sweep(t0, 700, t1, 2600) : sweep(t0, 2400, t1, 800);
  whoosh(m, ad(t0, t0 + 0.6 * (t1 - t0), vel, t1), band, 1.3, {
    pan: line(t0, pan0, t1, pan1),
    send: 0.22,
    hold: false,
  });
}

/** A line seating in its slot, a chip landing: the click. */
export function seat(m: Mix, t: number, vel = 1, pan: number = PAN.card): void {
  m.duck(t, 0.08, 0.1);
  tick(m, t, 2100, 0.34 * vel, pan, 0.1);
  tick(m, t + 0.004, 4200, 0.12 * vel, pan, 0.06);
}

/**
 * A key pressed on its keycap (⌃L): a soft, low click, with the key's
 * plastic on top so it speaks through the music on a laptop.
 */
export function keyClick(
  m: Mix,
  t: number,
  vel = 1,
  pan: number = PAN.key,
): void {
  tick(m, t, 900, 0.36 * vel, pan, 0.06);
  tick(m, t + 0.001, 2600, 0.16 * vel, pan, 0.05);
  thump(m, t + 0.002, 0.06 * vel, 240, 120, 0.07, { pan });
}

/**
 * The stamp's thunk: a rubber stamp's weight on paper, low and damped, the
 * wooden block's knock and the paper's slap, on the landing (the stamp's
 * approach's end). The knock and the slap carry it: the weight alone sat
 * below 180 Hz, which a laptop or a phone does not play, and under Act
 * III's organ it was 17 dB down.
 */
export function thunk(m: Mix, t: number, vel = 1, pan = 0, bite = 1): void {
  m.duck(t, 0.45, 0.16);
  thump(m, t, 0.55 * vel, 150, 58, 0.2, { pan });
  // `bite` raises only the knock and the slap: more weight only drives the
  // master's compressor and barely raises what a small speaker plays.
  const top = vel * bite;
  const k = m.voice(perc(t, 0.3 * top, 0.0006, 0.04), { pan, send: 0.1 });
  if (k) k.osc("triangle", sweep(t, 980, t + 0.03, 640));
  const v = m.voice(perc(t, 1.6 * top, 0.0006, 0.05), { pan, send: 0.12 });
  if (v) v.noise("pink", 1, v.filter("bandpass", 1100, 1), 1, t + 0.08);
  const p = m.voice(perc(t + 0.003, 0.45 * top, 0.0005, 0.025), { pan });
  if (p) p.noise("white", 1, p.filter("bandpass", 3600, 1.4), 1, t + 0.04);
}

/**
 * Bad input: a brass cluster jammed against the key, E F B E over a low
 * hit, while the music ducks hard under it.
 */
export function errorStab(m: Mix, t: number, vel = 1, pan = 0): void {
  m.duck(t, 0.65, 0.3);
  brass(m, t, t + 0.2, [52, 53, 59, 64], 0.11 * vel, {
    attack: 0.006,
    sustain: 0.5,
    release: 0.14,
    bright: 3600,
    pan,
    send: 0.3,
  });
  thump(m, t, 0.55 * vel, 95, 40, 0.34, { pan });
}

/**
 * A gas pilot lighting: the igniter's spark, then the flame catching with a
 * soft low push of air, "whump". Postgres has started. The push is pitched
 * and filtered high enough to be heard on small speakers: at 70 Hz under a
 * 1.3 kHz noise it was 80 % sub-bass, and only the spark carried.
 */
export function whump(m: Mix, t: number, vel = 1, pan = 0): void {
  m.duck(t, 0.6, 0.3);
  tick(m, t - 0.06, 3800, 0.18 * vel, pan, 0.1);
  const v = m.voice(ad(t, t + 0.025, 0.5 * vel, t + 0.55), { pan, send: 0.3 });
  if (!v) return;
  v.osc("sine", [
    [t, 110],
    [t + 0.35, 55, "exp"],
  ]);
  v.noise("pink", 2, v.filter("lowpass", sweep(t, 2400, t + 0.45, 300), 2));
}

/**
 * A tape deck's key: the transport's clunk and its latch a moment after.
 * Each checkpoint on the rail.
 */
export function tapeClick(m: Mix, t: number, vel = 1, pan = 0): void {
  thump(m, t, 0.3 * vel, 170, 70, 0.08, { pan });
  const a = m.voice(perc(t, 0.45 * vel, 0.0003, 0.012), { pan, send: 0.08 });
  if (a) a.noise("white", 1, a.filter("bandpass", 2800, 4), 1, t + 0.02);
  const b = m.voice(perc(t + 0.018, 0.25 * vel, 0.0003, 0.01), {
    pan,
    send: 0.08,
  });
  if (b) b.noise("white", 1, b.filter("bandpass", 4600, 5), 1, t + 0.04);
}

/**
 * Tape spooling back from `t0` to `t1`: a garble of chirps (a whine rising
 * as the reels speed up, gated faster and faster), over hiss; it lands with
 * the deck's click.
 */
export function rewind(m: Mix, t0: number, t1: number, vel = 1, pan = 0): void {
  const env = [
    [t0, 0],
    [t0 + 0.06, vel],
    [t1 - 0.03, 0.9 * vel],
    [t1, 0.0001, "exp"],
    [t1 + 0.004, 0],
  ] as const;
  const v = m.voice(env, { pan, send: 0.14, hold: true });
  if (!v) return;
  const am = v.vca(0.5, v.filter("lowpass", 5200, 3));
  v.lfo("square", sweep(t0, 11, t1, 34), 0.5, am.gain);
  const whine = v.osc("triangle", sweep(t0, 520, t1, 2900), 0.5, am);
  v.lfo("sine", 9, sweep(t0, 40, t1, 180), whine.frequency);
  v.osc("sawtooth", sweep(t0, 260, t1, 1450), 0.18, am);
  v.noise("white", 0.12, v.filter("highpass", 5000, 0));
  // It lands with the deck's click, firmer than a checkpoint's.
  m.duck(t1, 0.3, 0.15);
  tapeClick(m, t1, 1.6, pan);
}

/**
 * The kitchen's service bell ("order up"): a small dome struck by its
 * plunger, its partials a dome's, the first beating slowly against its
 * twin. On F6, the tonic, by default. The reel's arrivals ring it.
 */
export function serviceBell(
  m: Mix,
  t: number,
  vel: number,
  pan = 0,
  f = hz(89),
): void {
  m.duck(t, 0.22, 0.3);
  const v = m.voice(perc(t, vel, 0.0008, 2.6), { pan, send: 0.36 });
  if (!v) return;
  // [ratio, level, decay as a fraction of the ring]
  const partials = [
    [1, 0.7, 1],
    [1.0028, 0.4, 0.9],
    [2.32, 0.4, 0.45],
    [4.25, 0.22, 0.22],
    [6.63, 0.1, 0.12],
    [9.38, 0.05, 0.07],
  ] as const;
  for (const [ratio, a, k] of partials) {
    const p = f * ratio;
    if (p > 15500) continue;
    v.osc("sine", p, k === 1 ? a : perc(t, a, 0.0006, 2.6 * k));
  }
  v.noise(
    "white",
    perc(t, 0.25, 0.0003, 0.006),
    v.filter("bandpass", 5600, 1.2),
    1,
    t + 0.02,
  );
}

/**
 * Big type slamming in: a short rush of air as it winds in, and on the
 * landing a low hit under a brass stab of `chord`.
 */
export function slam(
  m: Mix,
  t: number,
  chord: readonly number[],
  vel = 1,
  pan = PAN.term,
): void {
  whoosh(
    m,
    ad(t - 0.16, t - 0.01, 0.05 * vel, t + 0.02),
    sweep(t - 0.16, 1400, t, 5200),
    1.4,
    { pan, send: 0.12, hold: false },
    "white",
  );
  m.duck(t, 0.45, 0.2);
  thump(m, t, 0.55 * vel, 120, 44, 0.28, { pan });
  brass(m, t, t + 0.14, chord, 0.075 * vel, {
    attack: 0.008,
    sustain: 0.5,
    release: 0.1,
    bright: 3400,
    pan,
    send: 0.26,
  });
}
