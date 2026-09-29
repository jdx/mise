// The hook of "mise-en-place" (jdx), as the score quotes it: the chorus
// melody transcribed from the recording (plan v3 §5, the song report §2.8,
// scratch v2/song/hook.json), with no words. Times are the song's own
// eighths, which the reel plays as its sixteenths (0.25 s against the
// record's 0.188 s: 0.75× its speed at 60 BPM). e = 0 is the downbeat of chorus bar
// 1, on "short"; a chorus bar is 8 eighths, 2 reel beats. Pitches are MIDI,
// in F minor.
//
// The score quotes M1 whole at the open and the climax, a fragment of it
// on each act's ticket (the head or the sigh), the "C-I both" cell at the
// packslip's skills link, chorus bars 7 and 8 into the morph, and bars 9 to
// 12 through it. M2, the cadence, is here only so test/motif.test.ts can
// hold that the score never plays it: the end card's recording is the one
// place it is heard, in jdx's voice.

import { X } from "./mix";
import type { Note } from "./sounds";

/** A note of the hook: start and length in song eighths, MIDI note. */
export type HookNote = readonly [e: number, len: number, midi: number];

const C3 = 48;
const F3 = 53;
const G3 = 55;
const A3 = 57;
const Bb3 = 58;
const C4 = 60;
const Db4 = 61;
const Eb4 = 63;
const E4 = 64;
const F4 = 65;
const G4 = 67;
const Ab4 = 68;

/**
 * The chorus as transcribed, bars 1 to 12 and the name line (hook.json).
 * "interoperable" (e 80, 10 eighths) is recited on D♭4 and dips to C4; it
 * is written here as the score articulates it, four notes on the syllables.
 */
export const CHORUS: readonly HookNote[] = [
  // M1: "In SHORT, it's Nix for PEO-ple WHO have"
  [-2, 2, C4],
  [0, 3, F4],
  [3, 1, C4],
  [4, 1, C4],
  [5, 1, C4],
  [6, 1, Ab4],
  [7, 2, G4],
  [9, 2, F4],
  [11, 3, C4],
  // "ac-tual work to do now (now)"
  [14, 1, Eb4],
  [15, 1, Db4],
  [17, 3, Db4],
  [20, 2, Db4],
  [22, 1, C4],
  [23, 1, C4],
  [24, 3, Db4],
  // "No wrest-ling stu-pid flakes to"
  [28, 3, Eb4],
  [31, 2, Eb4],
  [33, 1, Eb4],
  [34, 2, Bb3],
  [36, 2, Bb3],
  [38, 2, Eb4],
  [40, 1, Eb4],
  // "make a shell that sim-ply starts for you"
  [43, 2, Bb3],
  [45, 1, C4],
  [46, 3, C4],
  [49, 2, C4],
  [51, 1, C4],
  [52, 1, C4],
  [54, 2, C4],
  [57, 2, E4],
  [59, 3, C4],
  // "The lap-top and the C-I both be-come in-ter-op-er-a-ble"
  [62, 2, C4],
  [64, 2, F4],
  [66, 1, C4],
  [67, 3, C4],
  [70, 1, G4],
  [71, 1, G4],
  [72, 1, G4],
  [73, 3, F4],
  [76, 2, C4],
  [78, 2, Db4],
  [80, 3, Db4],
  [83, 2, Db4],
  [85, 1, C4],
  [86, 4, Db4],
  // "It's mise-en-place for dev ma-chines"
  [95, 1, C4],
  [96, 2, C4],
  [98, 2, Db4],
  [100, 2, C4],
  [103, 1, C4],
  [104, 1, C4],
  [105, 1, A3],
  [106, 4, G3],
  // M2: "pre-CISE and O-pe-RA-tion-al"
  [111, 1, Bb3],
  [112, 2, C4],
  [116, 2, C3],
  [118, 2, G3],
  [120, 2, G3],
  [122, 2, F3],
  [124, 9, F3],
];

/** The notes of the chorus from eighth `from` up to (not including) `to`. */
const span = (from: number, to: number): HookNote[] =>
  CHORUS.filter(([e]) => e >= from && e < to);

/**
 * M1, the signature phrase: the rising fourth to the tonic, a reciting
 * tone on the dominant, and the falling ♭3–2–1 sigh. 5 → 1 5 5 5 ♭3 2 1 (5).
 */
export const M1 = span(-2, 14);
/** M1's head, "In SHORT": the dominant rising a fourth to the tonic, C → F. */
export const HEAD = span(-2, 3);
/** M1's sigh, "PEO-ple WHO": ♭3–2–1, A♭ G F. */
export const SIGH = span(6, 11);
/** "the C-I both": G G G F, the sigh's cell recited on the second. */
export const BOTH = span(70, 76);
/**
 * Chorus bars 7 and 8, "that sim-ply starts for you", and the pickup "The"
 * whose C4 lands the next note, F4, on bar 9's downbeat.
 */
export const STARTS = span(49, 64);
/** Chorus bars 9 to 12: "The laptop and the CI both become interoperable". */
export const LAPTOP = span(64, 95);
/** M2, the cadence. Never played: the recording sings it on the end card. */
export const M2 = span(111, 133);

/**
 * A phrase on the reel's clock: each note from `e0`, the reel time of the
 * phrase's e = 0 (M1's F), one song eighth to a reel sixteenth, raised
 * `transpose` semitones. Each note is held `gate` of its length, so
 * repeated notes speak; a note straight into a different one runs on.
 */
export function onReel(
  phrase: readonly HookNote[],
  e0: number,
  vel = 1,
  transpose = 0,
  gate = 0.9,
): Note[] {
  return phrase.map(([e, len, n], i) => {
    const next = phrase[i + 1];
    const legato = next && next[0] === e + len && next[2] !== n;
    const hold = legato ? len : len * gate;
    return [e0 + e * X, e0 + (e + hold) * X, n + transpose, vel];
  });
}

/** Where a phrase's e = 0 falls so that its note at eighth `e` lands on reel time `t`. */
export const landing = (e: number, t: number): number => t - e * X;
