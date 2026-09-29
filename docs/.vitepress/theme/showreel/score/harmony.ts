// The score's harmony: F minor, the song's key, with the chords its verse
// and chorus use (i–iv–V–i; i–iv–♭VII–♭III), and D♭, the song's ♭VI, the
// one key the reel moves to (`cd ../dashboard`, until `cd ../api`). Each
// section lays out its changes on its own beats; the band's builders
// (score/band.ts) read the chord under any beat from them, so a change can
// land on a picture's cue as well as on the grid.

import type { Section } from "../timeline";

/** A chord: its bass root, the bass's fifth, and a close voicing in the middle of the band (MIDI). */
export interface Chord {
  name: string;
  /** The bass's root, in the second octave: between B♭1 and B♭2. */
  root: number;
  /** The bass's other note on the off beats: the fifth above the root, or a third where it sits better. */
  fifth: number;
  /** Three notes for the pizzicato, the brass and the organ, round A♭3 to F4. */
  mid: readonly number[];
  /** The chord's tones an octave higher, for the marimba and the chops. */
  high: readonly number[];
}

const chord = (
  name: string,
  root: number,
  fifth: number,
  mid: readonly number[],
): Chord => ({ name, root, fifth, mid, high: mid.map((n) => n + 12) });

/**
 * The chords the reel uses. Every voicing keeps the notes the chords share
 * (F, A♭, C, D♭) on the same pitches, so the changes move one or two
 * voices at a time.
 */
export const CH = {
  /** i: F A♭ C. */
  Fm: chord("Fm", 41, 48, [56, 60, 65]),
  /** iv: B♭ D♭ F. */
  Bbm: chord("Bbm", 46, 41, [58, 61, 65]),
  /** V: C E G, with its third on top for the pull home. */
  C: chord("C", 36, 43, [55, 60, 64]),
  /** V7: B♭ C E, the dominant with its seventh. */
  C7: chord("C7", 36, 43, [58, 60, 64]),
  /** ♭VI: D♭ F A♭, the dashboard's key. */
  Db: chord("Db", 37, 44, [56, 61, 65]),
  /** ♭VII: E♭ G B♭. */
  Eb: chord("Eb", 39, 46, [55, 58, 63]),
  /** ♭III: A♭ C E♭. */
  Ab: chord("Ab", 44, 39, [56, 60, 63]),
  /** In D♭ major, its IV: G♭ B♭ D♭. */
  Gb: chord("Gb", 42, 37, [58, 61, 66]),
} as const satisfies Record<string, Chord>;

/** A section's changes: from each section beat on, a chord, until the next. */
export type Changes = readonly (readonly [beat: number, chord: Chord])[];

/** The chord sounding at section beat `b`. */
export function chordAt(changes: Changes, b: number): Chord {
  let c = changes[0][1];
  for (const [at, ch] of changes) if (at <= b + 1e-6) c = ch;
  return c;
}

/** One chord a bar, from the section's first downbeat. */
export const perBar = (...chords: Chord[]): Changes =>
  chords.map((c, i) => [4 * i, c]);

/** The spans of section `s` (section beats) where each chord of `changes` sounds. */
export function spans(
  s: Section,
  changes: Changes,
  to = s.beats,
): { from: number; to: number; chord: Chord }[] {
  const out: { from: number; to: number; chord: Chord }[] = [];
  changes.forEach(([at, c], i) => {
    const end = Math.min(to, i + 1 < changes.length ? changes[i + 1][0] : to);
    if (end > at) out.push({ from: at, to: end, chord: c });
  });
  return out;
}
