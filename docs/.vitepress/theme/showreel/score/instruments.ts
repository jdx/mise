// The band. The song moves from an operetta band (brass, strings plucked
// and bowed, a double bass) to a Hammond organ, soul and reggae, and ends
// as rock (the song report §2.7); the reel follows it act by act (plan v3
// §5): pizzicato, brass and marimba for Acts 0 to II; the organ and
// brushes for III; a voice per task lane for IV (woodblock, pizzicato,
// marimba); off-beat chops, a thin bass and a one-drop for V and VI; a
// filtered pulse in VII's valley and the whole kit at the climax. The lead
// is a reed, a clarinet's odd harmonics, that sings the hook with no words.
// Each act's ticket plays its fragment of the hook on the act's own voice,
// so the Act V and VI tickets add a tape-worn electric piano and a
// melodica, and VII a celesta.
//
// Every voice is synthesized; nothing is sampled. Each event varies a
// little, seeded by its own time, so repeats sound played while every start
// point still hears the same sounds.

import { hash } from "../math";
import { BEAT } from "../timeline";
import {
  type Curve,
  hold,
  hz,
  type Mix,
  perc,
  type Pt,
  sweep,
  type Voice,
  type VoiceOpts,
} from "./mix";
import { type Note, thump } from "./sounds";

type Bus = NonNullable<VoiceOpts["bus"]>;

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
  /** The mix bus: "music" by default, "sfx" for a pluck that marks an event and must not duck under it. */
  bus?: Bus;
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
    bus: o.bus ?? "music",
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

/**
 * A bowed string (the cello under Acts 0 to II): a sawtooth, as a bowed
 * string moves, through the body's air, wood and bridge resonances, the
 * bow taking `attack` seconds to reach full pressure, a vibrato growing
 * once the note has settled, and the rosin's faint hiss.
 */
export function bowed(
  m: Mix,
  t0: number,
  t1: number,
  f: number,
  vel: number,
  o: { attack?: number; release?: number; pan?: number; send?: number } = {},
): void {
  const attack = o.attack ?? 0.14;
  // Right of centre, across from the pizzicato's plucks, so Acts 0 to II
  // do not lean left.
  const v = m.voice(hold(t0, attack, vel, t1, 0.8 * vel, o.release ?? 0.25), {
    bus: "music",
    pan: o.pan ?? 0.25,
    send: o.send ?? 0.28,
    hold: true,
  });
  if (!v) return;
  const air = v.filter("peaking", 250, 1.4);
  air.gain.value = 1;
  const body = v.filter("peaking", 520, 1.2, air);
  body.gain.value = 2;
  const bridge = v.filter("peaking", 2400, 1.3, body);
  bridge.gain.value = 2;
  const lp = v.filter(
    "lowpass",
    [
      [t0, 900],
      [t0 + attack, 3200, "exp"],
    ],
    0,
    bridge,
  );
  const saw = v.osc("sawtooth", f, 1, lp);
  if (t1 - t0 > 0.5)
    v.lfo(
      "sine",
      5.2,
      [
        [t0, 0],
        [t0 + 0.35, 0],
        [t1, 0.006 * f],
      ],
      saw.frequency,
    );
  v.noise("white", 0.04, v.filter("bandpass", 3000, 0.8, lp));
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
  bus?: Bus;
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
    bus: o.bus ?? "music",
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

/** A short brass chord pushed hard and let go: the "pah" of Acts 0 to II. */
export function brassStab(
  m: Mix,
  t: number,
  notes: readonly number[],
  vel: number,
  o: BrassOpts = {},
): void {
  brass(m, t, t + 0.11, notes, vel, {
    attack: 0.012,
    sustain: 0.45,
    release: 0.07,
    ...o,
  });
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
  o: { len?: number; send?: number; bus?: Bus } = {},
): void {
  const len = o.len ?? Math.min(1.1, 0.16 + 80 / f);
  const v = m.voice(perc(t, vel, 0.002, len), {
    bus: o.bus ?? "music",
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
 * stick's tick. Two of them, high and low, chop the Act IV sixteenths like
 * a knife on a board.
 */
export function woodblock(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  pan = 0,
  send = 0.12,
  bus: Bus = "music",
): void {
  const v = m.voice(perc(t, vel, 0.0005, 0.075), { bus, pan, send });
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
  bus: Bus = "music",
): void {
  const v = m.voice(perc(t, vel, 0.0015, len), { bus, pan, send });
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

// Keys.

const epianos = new WeakMap<BaseAudioContext, PeriodicWave>();

/**
 * An electric piano: a tine struck and ringing as a sine, frequency
 * modulated by its own pitch with an index that falls fast (the bark) and a
 * glassy ping two octaves up. `wow` wears it like old tape: a slow pitch
 * wander and a faster flutter.
 */
export function epiano(
  m: Mix,
  t: number,
  f: number,
  vel: number,
  o: {
    len?: number;
    pan?: number;
    send?: number;
    wow?: boolean;
    bus?: Bus;
  } = {},
): void {
  const len = o.len ?? 1.4;
  const v = m.voice(perc(t, vel, 0.002, len), {
    bus: o.bus ?? "music",
    pan: o.pan ?? 0,
    send: o.send ?? 0.25,
  });
  if (!v) return;
  const warm = waveOf(epianos, m.ac, (k) => (k === 1 ? 1 : k === 2 ? 0.08 : 0));
  const car = v.osc(warm, f, 0.85);
  v.lfo(
    "sine",
    f,
    [
      [t, f * 1.8],
      [t + 0.2, f * 0.35, "exp"],
      [t + len, f * 0.08, "exp"],
    ],
    car.frequency,
  );
  v.osc("sine", f * 4, perc(t, 0.07, 0.0008, 0.1));
  if (o.wow) {
    v.lfo("sine", 0.8 + 0.1 * vary(t, 5), f * 0.005, car.frequency);
    v.lfo("sine", 7.3, f * 0.0015, car.frequency);
  }
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
  bus?: Bus;
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
        bus: o.bus ?? "music",
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

// Reeds.

const clarinets = new WeakMap<BaseAudioContext, PeriodicWave>();
const freeReeds = new WeakMap<BaseAudioContext, PeriodicWave>();

export interface ReedOpts {
  bus?: Bus;
  pan?: number;
  send?: number;
  /** The lowpass ceiling. */
  bright?: number;
  /** Seconds the reed takes to speak. */
  attack?: number;
  release?: number;
}

/**
 * The lead: a clarinet-like reed. Its wave is the odd harmonics a closed
 * pipe favours, with a trace of the even; two copies a few cents apart
 * keep it alive; a breath of air rides the attack; a long note grows a
 * vibrato after it has spoken, as a singer's would.
 */
export function reed(
  m: Mix,
  t0: number,
  t1: number,
  f: number,
  vel: number,
  o: ReedOpts = {},
): void {
  const attack = o.attack ?? 0.03;
  const v = m.voice(hold(t0, attack, vel, t1, 0.86 * vel, o.release ?? 0.07), {
    bus: o.bus ?? "music",
    pan: o.pan ?? 0.12,
    send: o.send ?? 0.26,
    hold: true,
  });
  if (!v) return;
  const wave = waveOf(clarinets, m.ac, (k) => (k % 2 ? k ** -0.95 : 0.1 / k));
  const top = o.bright ?? 3000;
  const lp = v.filter(
    "lowpass",
    [
      [t0, top * 0.45],
      [t0 + attack + 0.05, top, "exp"],
    ],
    0,
  );
  const body = v.filter("peaking", 1500, 1, lp);
  body.gain.value = 3;
  const a = v.osc(wave, f, 0.55, body, -3);
  const b = v.osc(wave, f, 0.45, body, 3);
  if (t1 - t0 > 0.32) {
    const depth: Pt[] = [
      [t0, 0],
      [t0 + 0.22, 0],
      [t1, 0.0055 * f],
    ];
    v.lfo("sine", 5.3, depth, a.frequency);
    v.lfo("sine", 5.3, depth, b.frequency);
  }
  v.noise(
    "pink",
    perc(t0, 0.05, 0.004, 0.08),
    v.filter("bandpass", 2400, 0.8, lp),
    1,
    t0 + 0.12,
  );
}

/**
 * A tune on the reed: one voice a note, a note running straight into a
 * different one overlapping it by 12 ms under one breath.
 */
export function reedLine(
  m: Mix,
  notes: readonly Note[],
  vel: number,
  o: ReedOpts = {},
): void {
  notes.forEach(([a, b, n, v], i) => {
    const next = notes[i + 1];
    const slur = next && next[2] !== n && next[0] - b < 0.02;
    reed(m, a, slur ? next[0] + 0.012 : b, hz(n), vel * v, {
      attack: i && notes[i - 1][1] >= a - 0.02 ? 0.015 : undefined,
      ...o,
    });
  });
}

/**
 * A melodica: a free reed blown through a mouthpiece, hk's concertina wave
 * (odd and even harmonics, a strong second and third) with a brighter,
 * breathier formant, and its two reeds beating.
 */
export function melodica(
  m: Mix,
  t0: number,
  t1: number,
  f: number,
  vel: number,
  o: ReedOpts = {},
): void {
  const attack = o.attack ?? 0.025;
  const v = m.voice(hold(t0, attack, vel, t1, 0.85 * vel, o.release ?? 0.06), {
    bus: o.bus ?? "music",
    pan: o.pan ?? 0.15,
    send: o.send ?? 0.22,
    hold: true,
  });
  if (!v) return;
  const wave = waveOf(
    freeReeds,
    m.ac,
    (k) => k ** -0.8 * (k === 2 ? 1.35 : k === 3 ? 1.2 : 1),
    48,
  );
  const lp = v.filter(
    "lowpass",
    [
      [t0, 1600],
      [t0 + attack + 0.05, o.bright ?? 3800, "exp"],
    ],
    0,
  );
  const formant = v.filter("peaking", 1500, 1.1, lp);
  formant.gain.value = 5;
  const cents = (1200 * Math.log2(1 + 3 / f)) / 2;
  v.osc(wave, f, 0.5, formant, -cents);
  v.osc(wave, f, 0.4, formant, cents);
  v.noise(
    "pink",
    perc(t0, 0.07, 0.004, 0.07),
    v.filter("bandpass", 2600, 0.9, lp),
    1,
    t0 + 0.1,
  );
}

// Drums.

/** The kit's kick: a falling sine and the beater's click. It pumps the music when `pump`. */
export function kick(m: Mix, t: number, vel = 1, pump = true): void {
  if (pump) m.kick(t);
  const k = 1 + 0.03 * vary(t, 21);
  thump(m, t, 0.95 * vel, 135 * k, 45, 0.3, { bus: "drums" });
  const c = m.voice(perc(t, 0.22 * vel, 0.0004, 0.012), { bus: "drums" });
  if (c) c.noise("white", 1, c.filter("bandpass", 3200, 1.2), 1, t + 0.02);
}

/** A snare: a short tuned body and the wires' hiss. */
export function snare(m: Mix, t: number, vel = 1, pan = 0.06): void {
  const k = 1 + 0.03 * vary(t, 22);
  const b = m.voice(perc(t, 0.5 * vel, 0.001, 0.09), {
    bus: "drums",
    pan,
    send: 0.14,
  });
  if (b) {
    b.osc("triangle", sweep(t, 250 * k, t + 0.04, 188 * k));
    b.osc("sine", 330 * k, perc(t, 0.4, 0.001, 0.04));
  }
  const w = m.voice(perc(t, 0.42 * vel, 0.001, 0.17), {
    bus: "drums",
    pan,
    send: 0.16,
  });
  if (w)
    w.noise(
      "white",
      1,
      w.filter("bandpass", 4200, 0.6, w.filter("highpass", 1500, 0)),
      1,
      t + 0.2,
    );
}

/** A cross-stick on the rim: a dry, woody click, the one-drop's snare. */
export function rim(m: Mix, t: number, vel = 1, pan = 0.08): void {
  const v = m.voice(perc(t, 0.5 * vel, 0.0004, 0.05), {
    bus: "drums",
    pan,
    send: 0.12,
  });
  if (!v) return;
  v.osc("sine", sweep(t, 1900, t + 0.01, 1650), perc(t, 0.6, 0.0004, 0.02));
  v.osc("triangle", sweep(t, 520, t + 0.02, 430), 0.5);
  v.noise(
    "white",
    perc(t, 0.5, 0.0003, 0.01),
    v.filter("bandpass", 2600, 2),
    1,
    t + 0.03,
  );
}

/** A closed hi-hat: a short tick of high noise. */
export function hihat(m: Mix, t: number, vel: number, pan = 0.24): void {
  const v = m.voice(perc(t, 0.26 * vel, 0.0008, 0.045), {
    bus: "drums",
    pan,
  });
  if (v) v.noise("white", 1, v.filter("highpass", 7200, 0));
}

const strikes = new WeakMap<BaseAudioContext, PeriodicWave>();

/**
 * A falling sawtooth whose cycle starts on its sharp rising edge (hk's
 * strike wave): an LFO for something struck on the beat and dying away
 * until the next.
 */
const STRIKE = (k: number): number => 2 / (Math.PI * k);

/**
 * Brushes on the snare from `t0` to `t1`: the swirl, a circle a beat, the
 * bristles swelling on each beat and dying away before the next, over a
 * faint hiss. Taps (brushTap) mark the off beats. The bristles sit in a
 * narrow band round 5 kHz, above the reel's clicks and ticks (2 to 4 kHz):
 * a wide band at 4 kHz and a louder hiss buried every Act III cue and put
 * the act's top octaves 12 dB over its neighbours'.
 */
export function brushSwirl(
  m: Mix,
  t0: number,
  t1: number,
  vel: number,
  pan = -0.12,
): void {
  const v = m.voice(hold(t0, 0.25, vel, t1, vel, 0.2), {
    bus: "drums",
    pan,
    send: 0.14,
    hold: true,
  });
  if (!v) return;
  const am = v.vca(0.5);
  steadyLfo(v, strikes, STRIKE, 32, 1 / BEAT, 0.4, am.gain);
  v.noise("pink", 1, v.filter("bandpass", 5000, 1.2, am));
  v.noise("white", 0.025, v.filter("highpass", 7000, 0));
}

/**
 * A brush slapped on the snare: bristles, with a little of the drum's body.
 * Its top is rolled off from 6 kHz, which the swirl's band already fills.
 */
export function brushTap(m: Mix, t: number, vel: number, pan = 0.1): void {
  const v = m.voice(perc(t, vel, 0.002, 0.13), {
    bus: "drums",
    pan,
    send: 0.18,
  });
  if (!v) return;
  const top = v.filter("lowpass", 6000, -3);
  v.noise("white", 1, v.filter("bandpass", 3000, 0.7, top), 1, t + 0.15);
  v.osc("triangle", sweep(t, 230, t + 0.03, 180), perc(t, 0.3, 0.001, 0.035));
}

// Chops and pulses.

/**
 * An off-beat chop, the reggae skank: a clean guitar's chord struck and
 * damped at once, bright and dry, over an organ's short bubble of the same
 * chord. The top rolls off from 3.8 kHz and the octave is kept low: there the
 * chord's partials met within 5 Hz of each other (F5's 6th, C6's 4th, the
 * F6 square's 3rd at 4.19 kHz) and whistled on every chop.
 */
export function chop(
  m: Mix,
  t: number,
  notes: readonly number[],
  vel: number,
  pan = 0.3,
): void {
  const env: Pt[] = [
    [t, 0],
    [t + 0.002, vel],
    [t + 0.06, 0.55 * vel, "exp"],
    [t + 0.16, 0.12 * vel, "exp"],
    [t + 0.2, 0.0001, "exp"],
    [t + 0.204, 0],
  ];
  const v = m.voice(env, { bus: "music", pan, send: 0.14 });
  if (!v) return;
  const hp = v.filter("highpass", 380, 0);
  const pick = v.filter("peaking", 2300, 1.2, hp);
  pick.gain.value = 6;
  const lp = v.filter("lowpass", 3800, -3, pick);
  const each = 0.4 / Math.sqrt(notes.length);
  notes.forEach((n, i) => {
    v.osc("sawtooth", hz(n), each, lp, 4 * (i - 1));
    v.osc("square", hz(n) * 2, 0.2 * each, lp);
  });
  v.noise(
    "white",
    perc(t, 0.25, 0.0004, 0.008),
    v.filter("bandpass", 3400, 1.5, hp),
    1,
    t + 0.02,
  );
}

/**
 * A note of the filtered pulse: a sawtooth and a square, a hair apart,
 * through a resonant lowpass, cut short. VII's valley runs it in
 * sixteenths, its cutoff the only thing that moves.
 */
export function pulseNote(
  m: Mix,
  t: number,
  len: number,
  f: number,
  vel: number,
  cutoff: number,
  pan = 0,
): void {
  const v = m.voice(perc(t, vel, 0.004, len), {
    bus: "music",
    pan,
    send: 0.24,
  });
  if (!v) return;
  const lp = v.filter("lowpass", cutoff, 7);
  v.osc("sawtooth", f, 0.6, lp);
  v.osc("square", f * 1.004, 0.3, lp);
}
