// from jdx/hk@37937824 docs/.vitepress/theme/showreel/score/mix.ts
// The score's engine: automation curves in reel seconds, the seeded buffers
// every score on a context shares, and the Mix, which turns envelopes into
// voices from any start point. Randomness is seeded per event, so a mid-reel
// start plays the same sounds, on the same frames, that full playback plays
// from there.

import { BEAT, DURATION } from "../timeline";
import { hash, rng, smoothstep } from "../math";

/** A sixteenth note. */
export const X = BEAT / 4;
/** Equal-tempered pitch of MIDI note `n`. */
export const hz = (n: number): number => 440 * 2 ** ((n - 69) / 12);

/**
 * DynamicsCompressorNode delays its output by a fixed 6 ms lookahead (Chromium,
 * WebKit, and Gecko share the same compressor kernel), and the master runs
 * two in series: the glue compressor and the limiter. Everything is
 * scheduled that much early so the mix leaves the graph on the frame.
 */
export const LATENCY = 2 * 0.006;
/** A sound due at most this much before the floor (Mix.floor), and not before `from`, still plays whole, that much late. */
const LATE = 0.06;

// Automation. A curve is a constant or a list of points in reel seconds; each
// point's kind shapes the segment arriving at it. Exponential segments only
// ever run between positive values, and every envelope starts and ends at 0.

type Kind = "lin" | "exp" | "set";
export type Pt = readonly [t: number, v: number, kind?: Kind];
export type Curve = number | readonly Pt[];

function valueAt(pts: readonly Pt[], t: number): number {
  if (t <= pts[0][0]) return pts[0][1];
  for (let i = 1; i < pts.length; i++) {
    const [t1, v1, k = "lin"] = pts[i];
    if (t < t1) {
      const [t0, v0] = pts[i - 1];
      const u = (t - t0) / (t1 - t0);
      if (k === "set") return v0;
      if (k === "exp" && v0 > 0 && v1 > 0) return v0 * (v1 / v0) ** u;
      return v0 + (v1 - v0) * u;
    }
  }
  return pts[pts.length - 1][1];
}

/** Linear attack to `peak`, then an exponential fall of 80 dB over `len`. */
export const perc = (
  t: number,
  peak: number,
  attack: number,
  len: number,
): Pt[] => [
  [t, 0],
  [t + attack, peak],
  [t + attack + len, 0.0001, "exp"],
  [t + attack + len + 0.004, 0],
];
/** Rise linearly to `peak` at `tp`, then fall away exponentially to silence at `t1`. */
export const ad = (t0: number, tp: number, peak: number, t1: number): Pt[] => [
  [t0, 0],
  [tp, peak],
  [t1, 0.0001, "exp"],
  [t1 + 0.004, 0],
];
/** Attack to `peak`, move to `end` by `t1`, then release over `rel`. */
export const hold = (
  t0: number,
  attack: number,
  peak: number,
  t1: number,
  end: number,
  rel: number,
): Pt[] => [
  [t0, 0],
  [t0 + attack, peak],
  [t1, end],
  [t1 + rel, 0.0001, "exp"],
  [t1 + rel + 0.004, 0],
];
/** Fade in to `a` at `ta`, swell exponentially to `peak` at `tp`, then cut away by `t1`. */
export const swell = (
  t0: number,
  ta: number,
  a: number,
  tp: number,
  peak: number,
  t1: number,
): Pt[] => [
  [t0, 0],
  [ta, a],
  [tp, peak, "exp"],
  [t1, 0.0001, "exp"],
  [t1 + 0.004, 0],
];
/** An exponential sweep from `v0` at `t0` to `v1` at `t1` (pitches, cutoffs). */
export const sweep = (t0: number, v0: number, t1: number, v1: number): Pt[] => [
  [t0, v0],
  [t1, v1, "exp"],
];
/** A straight line from `v0` at `t0` to `v1` at `t1` (pans). */
export const line = (t0: number, v0: number, t1: number, v1: number): Pt[] => [
  [t0, v0],
  [t1, v1],
];

/** Exponential glide sampled along an easing curve. */
export function glide(
  t0: number,
  t1: number,
  v0: number,
  v1: number,
  ease: (u: number) => number,
  n = 8,
): Pt[] {
  const out: Pt[] = [];
  for (let i = 0; i <= n; i++) {
    const u = i / n;
    out.push([t0 + (t1 - t0) * u, v0 * (v1 / v0) ** ease(u), "exp"]);
  }
  return out;
}

// Buffers and curves shared by every score on one context.

/** A shaper curve, typed as the constructor returns it so any TS DOM lib accepts it. */
export const floats = (n: number) => new Float32Array(n);
export type Floats = ReturnType<typeof floats>;

/** Buffers are generated the first time a score needs them. */
interface Shared {
  noise: Partial<Record<NoiseKind, AudioBuffer>>;
  room: AudioBuffer | null;
  crush: Floats;
  ceiling: Floats;
}
export type NoiseKind = "white" | "pink" | "crackle";

const sharedCache = new WeakMap<BaseAudioContext, Shared>();

type Fill = (r: () => number, d: Float32Array) => void;
const NOISE: Record<NoiseKind, readonly [seed: number, fill: Fill]> = {
  white: [
    1,
    (r, d) => {
      for (let i = 0; i < d.length; i++) d[i] = r() * 2 - 1;
    },
  ],
  // Paul Kellet's pink filter: warmer air for whooshes.
  pink: [
    2,
    (r, d) => {
      let b0 = 0;
      let b1 = 0;
      let b2 = 0;
      let peak = 0;
      for (let i = 0; i < d.length; i++) {
        const w = r() * 2 - 1;
        b0 = 0.99765 * b0 + w * 0.099046;
        b1 = 0.963 * b1 + w * 0.2965164;
        b2 = 0.57 * b2 + w * 1.0526913;
        d[i] = b0 + b1 + b2 + w * 0.1848;
        peak = Math.max(peak, Math.abs(d[i]));
      }
      for (let i = 0; i < d.length; i++) d[i] /= peak;
    },
  ],
  // Sparse, ringing clicks: tape adhesive letting go, cardboard crumbling.
  crackle: [
    3,
    (r, d) => {
      for (let i = 0; i < d.length - 16; i++) {
        if (r() < 0.004) {
          const a = (0.3 + 0.7 * r()) * (r() < 0.5 ? -1 : 1);
          for (let k = 0; k < 14; k++)
            d[i + k] += a * Math.exp(-k / 3) * (k % 2 ? -0.6 : 1);
        }
      }
    },
  ],
};

/** Two seconds of seeded noise, looped by every voice that plays it. */
export function noiseOf(
  ac: BaseAudioContext,
  sh: Shared,
  kind: NoiseKind,
): AudioBuffer {
  let buf = sh.noise[kind];
  if (!buf) {
    const [seed, fill] = NOISE[kind];
    buf = ac.createBuffer(1, Math.floor(ac.sampleRate * 2), ac.sampleRate);
    fill(rng(seed), buf.getChannelData(0));
    sh.noise[kind] = buf;
  }
  return buf;
}

export function roomOf(ac: BaseAudioContext, sh: Shared): AudioBuffer {
  sh.room ??= roomImpulse(ac);
  return sh.room;
}

/** A warm hall: decorrelated noise with early reflections that darkens as it decays. */
function roomImpulse(ac: BaseAudioContext): AudioBuffer {
  const sr = ac.sampleRate;
  const len = 2.2;
  const n = Math.floor(sr * len);
  const buf = ac.createBuffer(2, n, sr);
  for (let ch = 0; ch < 2; ch++) {
    const r = rng(0x5eed + ch * 977);
    const d = buf.getChannelData(ch);
    // The two exponentials advance by a fixed ratio per sample.
    const kDark = Math.exp(-1 / (0.35 * sr));
    const kDecay = Math.exp(-6.9 / (1.7 * sr));
    const fade0 = Math.floor(sr * (len - 0.25));
    let dark = 1;
    let decay = 1;
    let lp = 0;
    for (let i = 0; i < n; i++) {
      const t = i / sr;
      const pre = t < 0.011 ? 0 : Math.min(1, (t - 0.011) / 0.006);
      const a = 0.06 + 0.7 * dark;
      lp += a * (r() * 2 - 1 - lp);
      const tailOut = i < fade0 ? 1 : 1 - smoothstep(len - 0.25, len, t);
      d[i] = lp * decay * pre * tailOut * (1.2 - a);
      dark *= kDark;
      decay *= kDecay;
    }
    for (let k = 0; k < 14; k++) {
      const i = Math.floor(sr * (0.011 + r() * 0.075));
      d[i] += (r() * 2 - 1) * 0.6 * (1 - k / 16);
    }
  }
  return buf;
}

export function shared(ac: BaseAudioContext): Shared {
  let s = sharedCache.get(ac);
  if (s) return s;
  const curve = (n: number, f: (x: number) => number) => {
    const c = floats(n);
    for (let i = 0; i < n; i++) c[i] = f((i / (n - 1)) * 2 - 1);
    return c;
  };
  s = {
    noise: {},
    room: null,
    crush: curve(2048, (x) => Math.round(x * 5) / 5),
    // Final safety after the limiter: unity below 0.66, then a smooth knee
    // that never passes 0.75 (-2.5 dBFS), so the true peak between samples
    // stays under -2 dBTP and the AAC encode's overshoot under -1. The
    // domain is ±2 so the knee has room.
    ceiling: curve(8192, (x) => {
      const a = Math.abs(x * 2);
      const k = 0.66;
      const c = 0.75;
      return (
        Math.sign(x) * (a <= k ? a : k + (c - k) * Math.tanh((a - k) / (c - k)))
      );
    }),
  };
  sharedCache.set(ac, s);
  return s;
}

// The mix: buses, time mapping, voices.

export interface VoiceOpts {
  pan?: Curve;
  /** Reverb send level. */
  send?: number;
  /** Sustained sound: if playback starts inside it, enter already sounding with a short fade-in. */
  hold?: boolean;
}

/** True when `env` still rings 150 ms above -30 dB (of its peak) after `t`. */
function rings(env: readonly Pt[], t: number): boolean {
  let peak = 0;
  for (const p of env) peak = Math.max(peak, p[1]);
  return valueAt(env, t + 0.15) > 0.0316 * peak;
}

/**
 * The score's voices, from reel time `from` on, and the accents the duck
 * curve follows (audio.ts), recorded as the composition builds them.
 */
export class Mix {
  /** Effect accents the music ducks under: time, depth, release. */
  readonly ducks: Dip[] = [];
  /**
   * The earliest reel time a sound can start: `from`, or later when `from`
   * plays less than LATENCY into the context, before its time 0.
   */
  readonly floor: number;
  private readonly strips = new Map<string, AudioNode>();
  /** Voices built but not yet wired in: context time, output, strip. */
  private readonly pending: [at: number, out: AudioNode, into: AudioNode][] =
    [];

  constructor(
    readonly ac: BaseAudioContext,
    readonly sh: Shared,
    readonly from: number,
    readonly when: number,
    /**
     * The effects bus every voice mixes into. The music bus beside it is
     * the bed's alone (audio.ts): it ducks under these voices and rides
     * the sections' fader, and nothing the score composes plays on it.
     */
    readonly sfx: AudioNode,
    readonly verb: AudioNode,
  ) {
    this.floor = Math.max(from, from + LATENCY - when);
  }

  /**
   * Context time of reel time `t`, pinned to a sample frame so any start
   * point lands every source on the same frame as full playback does. The
   * time sits half a frame before it: Chromium rounds a start time up to the
   * next frame, and a time exactly on a frame lands a frame late whenever
   * float error puts it a hair past, which differs between start points and
   * moves sounds that share a noise stream against each other.
   */
  at(t: number): number {
    const sr = this.ac.sampleRate;
    return Math.max(
      0,
      (Math.round((this.when + (t - this.from) - LATENCY) * sr) - 0.5) / sr,
    );
  }

  /**
   * Wire a voice's output into its strip. An offline render processes
   * every node the destination can reach, sounding or not, so a voice due
   * later is wired in only shortly before it starts (wire()); a context
   * that cannot suspend is wired at once.
   */
  link(out: AudioNode, into: AudioNode, t: number): void {
    const at = this.at(t);
    if (!suspends(this.ac) || at < WIRE_EVERY) out.connect(into);
    else this.pending.push([at, out, into]);
  }

  /**
   * Wire the pending voices as the render reaches them: the context
   * suspends every WIRE_EVERY seconds and wires the voices due before the
   * next stop, a stop early, then resumes. Call once, after the score is
   * built and before rendering starts.
   */
  wire(): void {
    const ac = this.ac;
    if (!suspends(ac) || !this.pending.length) return;
    const end = ac.length / ac.sampleRate;
    const due = new Map<number, [AudioNode, AudioNode][]>();
    for (const [at, out, into] of this.pending) {
      const k = Math.max(0, Math.floor(at / WIRE_EVERY) - 1);
      if (k * WIRE_EVERY >= end) continue;
      let list = due.get(k);
      if (!list) due.set(k, (list = []));
      list.push([out, into]);
    }
    this.pending.length = 0;
    for (const [k, list] of due) {
      if (k === 0) {
        for (const [out, into] of list) out.connect(into);
        continue;
      }
      ac.suspend(k * WIRE_EVERY).then(() => {
        for (const [out, into] of list) out.connect(into);
        return ac.resume();
      });
    }
  }

  duck(t: number, depth: number, release = 0.15): void {
    this.ducks.push([t, depth, release]);
  }

  /**
   * A shared channel strip (static pan, reverb send) that voices mix into, so
   * a few dozen panners and send gains serve every voice.
   */
  strip(pan: number, send: number): AudioNode {
    const p = Math.round(pan * 20) / 20;
    const s = Math.round(send * 50) / 50;
    const key = `${p}|${s}`;
    let input = this.strips.get(key);
    if (!input) {
      input = this.ac.createGain();
      let out: AudioNode = input;
      if (p) {
        const sp = this.ac.createStereoPanner();
        sp.pan.value = p;
        out = input.connect(sp);
      }
      out.connect(this.sfx);
      if (s) {
        const g = this.ac.createGain();
        g.gain.value = s;
        out.connect(g).connect(this.verb);
      }
      this.strips.set(key, input);
    }
    return input;
  }

  /**
   * Apply a curve to a param, entering mid-curve when it starts before
   * `enter`, and heard `shift` seconds late.
   */
  set(p: AudioParam, c: Curve, enter = this.from, shift = 0): void {
    if (typeof c === "number") {
      p.value = c;
      return;
    }
    let i = 0;
    if (c[0][0] >= enter) {
      p.setValueAtTime(c[0][1], this.at(c[0][0] + shift));
      i = 1;
    } else {
      while (i < c.length && c[i][0] <= enter) i++;
      p.setValueAtTime(valueAt(c, enter), this.at(enter + shift));
    }
    for (; i < c.length; i++) {
      const [t, v, k = "lin"] = c[i];
      const ct = this.at(t + shift);
      if (k === "exp") p.exponentialRampToValueAtTime(v, ct);
      else if (k === "set") p.setValueAtTime(v, ct);
      else p.linearRampToValueAtTime(v, ct);
    }
  }

  /**
   * A voice whose amplitude follows `env` (reel seconds), or null when it
   * cannot sound. A voice due just before the floor plays whole, a moment
   * late; one that began before `from` (a mid-reel start) enters mid-sound
   * with a short fade if it is sustained or still ringing, and is skipped
   * otherwise.
   */
  voice(env: readonly Pt[], o: VoiceOpts = {}): Voice | null {
    const t0 = env[0][0];
    const t1 = env[env.length - 1][0];
    if (t1 <= this.from || t0 >= DURATION) return null;
    const { floor } = this;
    let enter = t0;
    let shift = 0;
    if (t0 < floor) {
      if (t0 >= this.from - 1e-4 && floor - t0 <= LATE) shift = floor - t0;
      else if (o.hold || rings(env, floor)) enter = floor;
      else return null;
    }
    return new Voice(this, t0, t1, enter, shift, env, o);
  }
}

export class Voice {
  readonly amp: GainNode;

  constructor(
    readonly m: Mix,
    readonly t0: number,
    readonly t1: number,
    readonly enter: number,
    readonly shift: number,
    env: readonly Pt[],
    o: VoiceOpts,
  ) {
    const ac = m.ac;
    this.amp = ac.createGain();
    this.set(this.amp.gain, env);
    let out: AudioNode = this.amp;
    if (enter > t0) {
      const fade = ac.createGain();
      m.set(fade.gain, line(enter, 0, enter + 0.03, 1), enter);
      out = out.connect(fade);
    }
    // Moving pans get their own panner; static ones share a strip.
    if (typeof o.pan === "object") {
      const p = ac.createStereoPanner();
      this.set(p.pan, o.pan);
      out = out.connect(p);
    }
    m.link(
      out,
      m.strip(typeof o.pan === "number" ? o.pan : 0, o.send ?? 0),
      enter + shift,
    );
  }

  set(p: AudioParam, c: Curve): void {
    this.m.set(p, c, this.enter, this.shift);
  }

  private run(
    src: AudioScheduledSourceNode,
    until: number,
    offset?: number,
    from = this.enter,
  ): void {
    const m = this.m;
    const start = m.at(Math.max(from, this.enter) + this.shift);
    if (offset === undefined) src.start(start);
    else (src as AudioBufferSourceNode).start(start, offset);
    src.stop(m.at(Math.max(until, this.enter) + this.shift) + 0.02);
  }

  /** A gain stage into `into`; a plain 1 connects straight through. */
  gain(level: Curve, into: AudioNode = this.amp): AudioNode {
    if (level === 1) return into;
    const g = this.m.ac.createGain();
    this.set(g.gain, level);
    g.connect(into);
    return g;
  }

  /** A gain whose level an LFO can modulate (tremolo, flutter). */
  vca(base: number, into: AudioNode = this.amp): GainNode {
    const g = this.m.ac.createGain();
    g.gain.value = base;
    g.connect(into);
    return g;
  }

  osc(
    type: OscillatorType | PeriodicWave,
    freq: Curve,
    level: Curve = 1,
    into: AudioNode = this.amp,
    detune = 0,
  ): OscillatorNode {
    const o = this.m.ac.createOscillator();
    // PeriodicWave is an empty interface, so a string check cannot narrow it away.
    if (typeof type === "string") o.type = type as OscillatorType;
    else o.setPeriodicWave(type);
    this.set(o.frequency, freq);
    if (detune) o.detune.value = detune;
    o.connect(this.gain(level, into));
    this.run(o, this.t1);
    return o;
  }

  noise(
    kind: NoiseKind,
    level: Curve = 1,
    into: AudioNode = this.amp,
    rate: Curve = 1,
    until = this.t1,
  ): void {
    const s = this.m.ac.createBufferSource();
    s.buffer = noiseOf(this.m.ac, this.m.sh, kind);
    s.loop = true;
    this.set(s.playbackRate, rate);
    s.connect(this.gain(level, into));
    // A fixed read offset per event keeps every start point identical.
    const offset = hash(Math.round(this.t0 * 9973), kind.length) * 1.8;
    this.run(s, Math.min(until, this.t1), offset);
  }

  filter(
    type: BiquadFilterType,
    freq: Curve,
    q: Curve = 0.7,
    into: AudioNode = this.amp,
  ): BiquadFilterNode {
    const f = this.m.ac.createBiquadFilter();
    f.type = type;
    this.set(f.frequency, freq);
    this.set(f.Q, q);
    f.connect(into);
    return f;
  }

  shaper(curve: Floats, into: AudioNode = this.amp): WaveShaperNode {
    const w = this.m.ac.createWaveShaper();
    w.curve = curve;
    w.connect(into);
    return w;
  }

  /**
   * An oscillator modulating `target`. `cycle` gives the first whole-cycle
   * boundary at or after a time, so an LFO that keeps time with the beat can
   * wait for it when the voice enters mid-sound instead of starting off-grid.
   */
  lfo(
    type: OscillatorType | PeriodicWave,
    rate: Curve,
    depth: Curve,
    target: AudioParam,
    cycle?: (t: number) => number,
  ): void {
    const o = this.m.ac.createOscillator();
    // PeriodicWave is an empty interface, so a string check cannot narrow it away.
    if (typeof type === "string") o.type = type as OscillatorType;
    else o.setPeriodicWave(type);
    this.set(o.frequency, rate);
    const g = this.m.ac.createGain();
    this.set(g.gain, depth);
    o.connect(g).connect(target);
    this.run(
      o,
      this.t1,
      undefined,
      cycle && this.enter > this.t0 ? cycle(this.enter) : this.enter,
    );
  }
}

/**
 * How often an offline render stops to wire in the voices due next,
 * context seconds: a voice is wired between one and two stops before it
 * starts.
 */
const WIRE_EVERY = 0.5;

/** True for an OfflineAudioContext, which can suspend at a render time. */
function suspends(ac: BaseAudioContext): ac is OfflineAudioContext {
  return (
    "startRendering" in ac &&
    typeof (ac as OfflineAudioContext).suspend === "function"
  );
}

/** A duck the music takes under an effect: time, depth, and release. */
export type Dip = readonly [t: number, depth: number, release: number];
