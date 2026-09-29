// The grooves, one builder per band, over a section's changes (harmony.ts).
// The song's own count is a quarter at about 159, and its quarter is the
// reel's eighth, so its "bass on 1 and 3, chords on 2 and 4" is, on the
// reel's clock, the bass on every beat and a chord on every off-beat eighth
// (the "and"): the two-beat feel the song has, at 0.75× its speed (60 BPM).
// Every builder takes the section beats it plays over and the rests it
// leaves for a cue to be the only attack there; the sixteenth figures also
// take the spans where they move in eighths instead (`calm`), under the
// picture's long holds.

import { BEAT, type Section } from "../timeline";
import { hash } from "../math";
import { type Changes, chordAt, spans } from "./harmony";
import {
  bowed,
  brassStab,
  brushSwirl,
  brushTap,
  chop,
  hihat,
  kick,
  marimba,
  organ,
  type OrganOpts,
  pizzChord,
  pulseNote,
  rim,
  snare,
  woodblock,
} from "./instruments";
import { hz, type Mix, X } from "./mix";
import { bassRun, type Note } from "./sounds";

/** Section beats a builder rests over: [from, to). */
export type Rests = readonly (readonly [from: number, to: number])[];

/** Where in the section a builder plays: from and to (section beats) less its rests. */
export interface Span {
  from?: number;
  to?: number;
  rests?: Rests;
  /**
   * Section beats where a figure in sixteenths (the marimba's, lint's
   * woodblocks, the pulse) plays only its eighths: under a long hold, where
   * the picture is still and being read, four notes a second would hurry it.
   */
  calm?: Rests;
}

const inside = (spans: Rests | undefined, b: number): boolean =>
  (spans ?? []).some(([a, z]) => b >= a - 1e-6 && b < z - 1e-6);

const plays = (o: Span, b: number, s: Section): boolean =>
  b >= (o.from ?? 0) - 1e-6 &&
  b < (o.to ?? s.beats) - 1e-6 &&
  !inside(o.rests, b);

/** Sixteenth `k` of a figure falls silent here: it is an off-eighth in a calm span. */
const hushed = (o: Span, k: number): boolean =>
  k % 2 === 1 && inside(o.calm, k / 4);

/**
 * The off-beat eighths (each beat's "and") within `before` beats before and
 * `after` beats after any of `times` (global seconds), as rests: where a
 * chop or a brush tap would fall on a click that must speak alone.
 */
export function offbeatsNear(
  s: Section,
  times: readonly number[],
  before: number,
  after: number,
): Rests {
  const out: [number, number][] = [];
  for (const t of times) {
    const b = (t - s.start) / BEAT;
    for (
      let a = Math.ceil(b - before - 0.5 - 1e-6) + 0.5;
      a <= b + after + 1e-6;
      a++
    )
      if (a >= 0 && a < s.beats) out.push([a - 0.01, a + 0.01]);
  }
  return out;
}

/** A per-event variation in [-1, 1]: the same for the same event from any start point. */
const vary = (t: number, seed: number): number =>
  hash(Math.round(t * 9973), seed) * 2 - 1;

/** True when a chord change falls on section beat `b`. */
const changesAt = (changes: Changes, b: number): boolean =>
  changes.some(([at]) => Math.abs(at - b) < 1e-6);

export interface OomOpts extends Span {
  level?: number;
  /** Each note's length, sixteenths. */
  len?: number;
  grit?: number;
  cutoff?: number;
  /** Beats between notes: 1, or 2 for half time. */
  every?: number;
}

/**
 * The oom: the bass on every beat, staccato, the root on beats 1 and 3 and
 * the fifth on 2 and 4; a change always lands on its root.
 */
export function oom(
  m: Mix,
  s: Section,
  changes: Changes,
  o: OomOpts = {},
): void {
  const notes: Note[] = [];
  const every = o.every ?? 1;
  for (let b = Math.ceil(o.from ?? 0); b < s.beats; b += every) {
    if (!plays(o, b, s)) continue;
    const c = chordAt(changes, b);
    const root = b % 2 === 0 || changesAt(changes, b);
    notes.push([
      s.beat(b),
      s.beat(b) + (o.len ?? 2) * X,
      root ? c.root : c.fifth,
      root ? 1 : 0.8,
    ]);
  }
  if (notes.length)
    bassRun(m, notes, o.level ?? 0.08, o.cutoff ?? 360, o.grit ?? 0.45, true);
}

export interface PahOpts extends Span {
  /** The pizzicato chord's level. */
  vel?: number;
  /** The brass doubling on the and of 2 and the and of 4, 0 for none. */
  brass?: number;
  pan?: number;
}

/**
 * The pah: a plucked chord on every "and", the brass pushing the and of 2
 * and of 4. Just left of centre: at -0.22, eight plucks a bar against the
 * brass's two leaned Acts 0 to II 2 dB left.
 */
export function pah(
  m: Mix,
  s: Section,
  changes: Changes,
  o: PahOpts = {},
): void {
  const pan = o.pan ?? -0.1;
  for (let b = Math.floor(o.from ?? 0); b < s.beats; b++) {
    const at = b + 0.5;
    if (!plays(o, at, s)) continue;
    const c = chordAt(changes, at);
    const t = s.beat(at);
    pizzChord(m, t, c.mid, (o.vel ?? 0.05) * (1 + 0.05 * vary(t, 3)), pan, {
      bright: 10,
    });
    if (o.brass && b % 2 === 1)
      brassStab(m, t + 0.003, c.mid, o.brass, { pan: -pan, send: 0.22 });
  }
}

/**
 * A figure in sixteenths: each step an index into the chord's tones an
 * octave up (0, 1, 2, and 3 for the lowest an octave higher), or null for
 * a rest. It repeats every `figure.length` sixteenths from the section's
 * first downbeat.
 */
export type Figure = readonly (number | null)[];

export interface MalletOpts extends Span {
  vel?: number;
  pan?: number;
  /** Octaves above the chord's high voicing. */
  octave?: number;
}

/** The marimba's figure over the changes. */
export function mallets(
  m: Mix,
  s: Section,
  changes: Changes,
  figure: Figure,
  o: MalletOpts = {},
): void {
  const vel = o.vel ?? 0.07;
  for (let k = Math.ceil((o.from ?? 0) * 4 - 1e-6); k < s.beats * 4; k++) {
    const b = k / 4;
    const step = figure[k % figure.length];
    if (step === null || !plays(o, b, s) || hushed(o, k)) continue;
    const c = chordAt(changes, b);
    const tones = [...c.high, c.high[0] + 12];
    const t = s.beat(b);
    const accent = k % 4 === 0 ? 1 : k % 2 === 0 ? 0.85 : 0.72;
    marimba(
      m,
      t,
      hz(tones[step] + 12 * (o.octave ?? 0)),
      vel * accent * (1 + 0.06 * vary(t, 4)),
      (o.pan ?? 0.28) + 0.1 * (step - 1.5) * 0.3,
    );
  }
}

export interface CelloOpts extends Span {
  vel?: number;
  /** The first note's MIDI pitch to lead from; the line moves to the nearest chord tone. */
  from0?: number;
}

/**
 * The cello: a long bowed note for each chord of the changes, in the tenor
 * under the band, moving to the nearest tone of each new chord, so the
 * line walks by step and sustains between the plucks.
 */
export function cello(
  m: Mix,
  s: Section,
  changes: Changes,
  o: CelloOpts = {},
): void {
  const from = o.from ?? 0;
  const to = o.to ?? s.beats;
  let prev = o.from0 ?? 56;
  for (const sp of spans(s, changes, to)) {
    const a = Math.max(from, sp.from);
    // Too short to bow: the next chord's note takes it.
    if (sp.to - a < 0.5) continue;
    const tones = sp.chord.mid.map((n) => n - 12);
    const n = tones.reduce((best, x) =>
      Math.abs(x - prev) < Math.abs(best - prev) ? x : best,
    );
    prev = n;
    bowed(m, s.beat(a) + 0.01, s.beat(sp.to) - 0.06, hz(n), o.vel ?? LV.cello);
  }
}

export interface OrganBarsOpts extends Span, OrganOpts {
  vel?: number;
  /** Voicing: the chord's middle notes, or with its root under them. */
  root?: boolean;
}

/** The organ holding each chord of the changes. */
export function organBars(
  m: Mix,
  s: Section,
  changes: Changes,
  o: OrganBarsOpts = {},
): void {
  const from = o.from ?? 0;
  const to = o.to ?? s.beats;
  for (const sp of spans(s, changes, to)) {
    const a = Math.max(from, sp.from);
    if (sp.to - a < 0.25) continue;
    const notes = o.root ? [sp.chord.root + 12, ...sp.chord.mid] : sp.chord.mid;
    organ(m, s.beat(a), s.beat(sp.to) - 0.01, notes, o.vel ?? 0.06, o);
  }
}

export interface BrushOpts extends Span {
  /** The swirl's level. */
  vel?: number;
  /** The taps on the "and"s. */
  taps?: number;
}

/** Brushes: the swirl under it all, and a tap on every "and", the 2 and 4 of the song's count leaning in. */
export function brushes(m: Mix, s: Section, o: BrushOpts = {}): void {
  const from = o.from ?? 0;
  const to = o.to ?? s.beats;
  brushSwirl(m, s.beat(from), s.beat(to), o.vel ?? 0.05);
  for (let b = Math.floor(from); b < to; b++) {
    const at = b + 0.5;
    if (!plays(o, at, s)) continue;
    brushTap(m, s.beat(at), (o.taps ?? 0.12) * (b % 2 ? 1 : 0.8));
  }
}

export interface TrioOpts extends Span {
  /** Each lane's voice, by level; 0 or absent is silent. */
  lint?: number;
  test?: number;
  build?: number;
}

/** lint's two woodblocks chopping sixteenths like a knife on a board: 1 high, 0 low. */
const KNIFE: Figure = [
  1,
  null,
  0,
  1,
  null,
  1,
  0,
  null,
  1,
  null,
  0,
  1,
  null,
  1,
  0,
  0,
];
/** build's marimba running up and down the chord. */
const RUN: Figure = [
  0,
  null,
  1,
  2,
  3,
  null,
  2,
  1,
  0,
  null,
  1,
  2,
  3,
  2,
  null,
  1,
];

/**
 * Act IV's groove, a voice per task lane (plan v3 §5): lint on the
 * woodblocks, test on the pizzicato's off-beat chords, build on the
 * marimba's run, each at its own level, so a lane can enter alone.
 */
export function trio(
  m: Mix,
  s: Section,
  changes: Changes,
  o: TrioOpts = {},
): void {
  if (o.lint) {
    for (let k = Math.ceil((o.from ?? 0) * 4 - 1e-6); k < s.beats * 4; k++) {
      const b = k / 4;
      const step = KNIFE[k % 16];
      if (step === null || !plays(o, b, s) || hushed(o, k)) continue;
      const t = s.beat(b);
      woodblock(
        m,
        t,
        step ? 1250 : 880,
        o.lint * (k % 4 === 0 ? 1 : 0.75) * (1 + 0.08 * vary(t, 6)),
        step ? -0.35 : -0.25,
      );
    }
  }
  if (o.test) pah(m, s, changes, { ...o, vel: o.test, brass: 0, pan: -0.1 });
  if (o.build) mallets(m, s, changes, RUN, { ...o, vel: o.build, pan: 0.32 });
}

export interface SkankOpts extends Span {
  /** The chops' level. */
  chop?: number;
  /**
   * Section beats where only the chops rest, the groove going on under
   * them: the eighth before a click that must speak alone.
   */
  chopRests?: Rests;
  /** The thin bass's level. */
  bass?: number;
  /** The one-drop's level. */
  drums?: number;
  /** The hats' level. */
  hats?: number;
}

/** The thin bass's bar in sixteenths: [start, length, interval above the root, velocity]. */
const THIN: readonly (readonly [number, number, number, number])[] = [
  [0, 2, 0, 1],
  [3, 1, 0, 0.7],
  [6, 2, 7, 0.85],
  [8, 2, 12, 0.9],
  [11, 1, 7, 0.7],
  [14, 2, 0, 0.8],
];

/**
 * Acts V and VI: the skank. A chop on every "and", a thin bass walking
 * the root, fifth and octave, and the one-drop, the kick and the rim
 * together on the song's beat 3 (the reel's beats 2 and 4), over hats on
 * the eighths.
 */
export function skank(
  m: Mix,
  s: Section,
  changes: Changes,
  o: SkankOpts = {},
): void {
  const chopVel = o.chop ?? 0.07;
  for (let b = Math.floor(o.from ?? 0); b < s.beats; b++) {
    const at = b + 0.5;
    if (!plays(o, at, s) || !plays({ rests: o.chopRests }, at, s)) continue;
    const c = chordAt(changes, at);
    chop(m, s.beat(at), c.high, chopVel * (1 + 0.05 * vary(s.beat(at), 7)));
  }
  if (o.bass !== 0) {
    const notes: Note[] = [];
    for (let bar = 0; bar < s.beats / 4; bar++) {
      for (const [st, len, iv, vel] of THIN) {
        const b = 4 * bar + st / 4;
        if (!plays(o, b, s)) continue;
        const c = chordAt(changes, b);
        notes.push([s.beat(b), s.beat(b) + len * X, c.root + iv, vel]);
      }
    }
    if (notes.length) bassRun(m, notes, o.bass ?? 0.04, 700, 0.3, true);
  }
  const drums = o.drums ?? 1;
  // The hats kept under the chops: white noise above 7 kHz on every eighth
  // was the fizz on top of Acts V and VI.
  const hats = o.hats ?? 0.55;
  for (let k = Math.ceil((o.from ?? 0) * 4 - 1e-6); k < s.beats * 4; k++) {
    const b = k / 4;
    if (!plays(o, b, s)) continue;
    const t = s.beat(b);
    if (drums && k % 8 === 4) {
      kick(m, t, 0.55 * drums, false);
      rim(m, t, 1.15 * drums);
    }
    if (hats && k % 2 === 0) hihat(m, t, hats * (k % 4 === 2 ? 1 : 0.65));
  }
}

export interface KitOpts extends Span {
  vel?: number;
  hats?: number;
}

/**
 * The whole kit, the song's chorus and coda: the kick on every beat, the
 * snare on every "and" (the song's backbeat), and hats in sixteenths. The
 * kicks on 1 and 3 pump the music.
 */
export function kit(m: Mix, s: Section, o: KitOpts = {}): void {
  const vel = o.vel ?? 1;
  const hats = o.hats ?? 0.7;
  for (let k = Math.ceil((o.from ?? 0) * 4 - 1e-6); k < s.beats * 4; k++) {
    const b = k / 4;
    if (!plays(o, b, s)) continue;
    const t = s.beat(b);
    if (k % 4 === 0) kick(m, t, vel * (k % 8 === 0 ? 1 : 0.85), k % 8 === 0);
    if (k % 4 === 2) snare(m, t, vel * 0.85);
    if (hats) hihat(m, t, hats * (k % 2 === 0 ? 1 : 0.55));
  }
}

export interface PulseOpts extends Span {
  /** MIDI. */
  note: number;
  vel?: number;
  /** The cutoff at a section beat. */
  cutoff: (b: number) => number;
}

/** VII's valley: a filtered pulse in sixteenths, accented on the beats, the octave on each "and". */
export function pulse(m: Mix, s: Section, o: PulseOpts): void {
  for (let k = Math.ceil((o.from ?? 0) * 4 - 1e-6); k < s.beats * 4; k++) {
    const b = k / 4;
    if (!plays(o, b, s) || hushed(o, k)) continue;
    const t = s.beat(b);
    const accent = k % 4 === 0 ? 1 : k % 4 === 2 ? 0.8 : 0.55;
    pulseNote(
      m,
      t,
      0.17,
      hz(o.note + (k % 4 === 2 ? 12 : 0)),
      (o.vel ?? 0.05) * accent,
      o.cutoff(b),
      0.04 * vary(t, 8),
    );
  }
}

/**
 * The band's levels, so each section's balance reads as a change from
 * these: the lead over the chords, the bass under them, the figures between.
 */
export const LV = {
  lead: 0.075,
  bass: 0.06,
  pah: 0.12,
  brass: 0.07,
  mallet: 0.13,
  organ: 0.14,
  cello: 0.05,
  chop: 0.42,
  lint: 0.12,
  test: 0.12,
  build: 0.13,
  swirl: 0.045,
  taps: 0.24,
} as const;

/**
 * Act III's brush taps, under its clicks: the quiet act's cues are soft,
 * and at LV.taps its brushes filled 2 to 8 kHz, where every click sits.
 */
export const ACT3_TAPS = 0.75 * LV.taps;

/**
 * How far the band dips under an Act III click: the brushes swell on each
 * beat in the clicks' own band, so the quiet act's clicks take them down a
 * little.
 */
export const ACT3_DUCK = 0.35;
