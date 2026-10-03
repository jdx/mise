// The score's harmony: the chords the bed (bed.ts) plays under the
// stingers, so a pitched stinger is struck on the chord of the moment. A
// section whose stingers read the harmony lays out its changes on its own
// beats, from the bed's chord chart (chords.json, from the fit tool,
// docs/.vitepress/showreel-bed), checked against the bed's chroma; its cues
// read the chord under any beat from them (chordAt).
//
// The bed is in F minor and grooves round F minor, C minor and B♭, a 2/4
// bar a chord or more, over a C that rings above them almost throughout.
// Where it moves to F, its third wavers between A♭ and A (the bass sounds
// both), so the score's F there leaves the third out.

/** A chord: a close voicing in the middle register, and an octave over it (MIDI). */
export interface Chord {
  name: string;
  /** Three notes round G3 to F4: the slams' brass (props.ts slam). */
  mid: readonly number[];
  /**
   * The same an octave higher: the lanes' pizzicato and marimba (lanes.ts),
   * the climax's lights and pieces landing home (clone.ts), and the
   * switch's runs (switch.ts).
   */
  high: readonly number[];
}

const chord = (name: string, mid: readonly number[]): Chord => ({
  name,
  mid,
  high: mid.map((n) => n + 12),
});

/**
 * The chords the bed plays under pitched stingers. Every voicing sits
 * between G3 and F4 and keeps the tones the chords share (G, C, F) on the
 * same pitches.
 */
export const CH = {
  /** i: F A♭ C. */
  Fm: chord("Fm", [56, 60, 65]),
  /** I with its third left out, for the bed's F: G C F. */
  F5: chord("F5", [55, 60, 65]),
  /** v: C E♭ G. */
  Cm: chord("Cm", [55, 60, 63]),
  /** ♭VII: B♭ D F. */
  Bb: chord("Bb", [58, 62, 65]),
} as const satisfies Record<string, Chord>;

/** A section's changes: from each section beat on, a chord, until the next. */
export type Changes = readonly (readonly [beat: number, chord: Chord])[];

/**
 * A film's chart: the chord under reel second `t` where a film's music is
 * not the bed (film-audio.ts sets it round its render of the effects from
 * film-music.ts's song chart), or null where the film has no music under
 * that moment. Null in the source reel, whose parts follow their own
 * changes, set against the bed.
 */
type FilmChart = (t: number) => Chord | null;
let film: FilmChart | null = null;

/** Use `chart` for the chords under the reel's moments (null: the parts' own changes). */
export function setFilmChart(chart: FilmChart | null): void {
  film = chart;
}

/**
 * The chord sounding at section beat `b`: the film's, when a film chart is
 * set and the moment's reel second `t` is given, else the changes'.
 */
export function chordAt(changes: Changes, b: number, t?: number): Chord {
  if (film && t !== undefined) {
    const c = film(t);
    if (c) return c;
  }
  let c = changes[0][1];
  for (const [at, ch] of changes) if (at <= b + 1e-6) c = ch;
  return c;
}

/**
 * The chord's root among its high voicing: the tone a fourth above another
 * of its tones (C over G in F5, which has two, gives F over C, the higher).
 */
export function rootOf(c: Chord): number {
  const pcs = c.high.map((n) => n % 12);
  const roots = c.high.filter((n) => pcs.includes((n + 7) % 12));
  return roots[roots.length - 1];
}
