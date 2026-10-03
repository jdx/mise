// The original song, arranged for each film on the film's own clock. The
// picture's cuts never move the music: the song is cut on its own bar
// lines (120 BPM, a bar every 2 s), each join a crossfade on a downbeat,
// and its complete ending (the wind-down, the outro and the ring-out) plays
// under the end card, so the music stops where the film does.
import { filmDuration, type Edition } from "./edit";
import { CH, type Chord } from "./score/harmony";

export const MUSIC = {
  file: "docs/.vitepress/theme/showreel/score/music.opus",
  sha256: "4a4becfb7fc494bbe5ca18f1e81daa9d2b03710edc6c3377d846cd681ce57c6c",
  // Original WAV minus its 0.105 s lead-in; includes the complete ring-out.
  duration: 359.615,
} as const;

/** A bar of the song, seconds (120 BPM, 4/4). */
export const BAR = 2;
/** The song's last bar line plus its ring-out, in bars: where every arrangement ends. */
export const SONG_END = MUSIC.duration / BAR;

export interface MusicCut {
  /** Song seconds. */
  from: number;
  to: number;
  /** Film seconds. */
  start: number;
  end: number;
}

/**
 * The song's form, in bars (score/BED.md): intro 0–15 (no drums), groove
 * 16–43, break 44–47, groove 48–75, riser 76–79, breakdown 80–95 (no drums),
 * groove 96–109, break 110–111, groove 112–135, build 136–143, groove
 * 144–155, break 156–159, the loudest groove 160–167, wind-down 168–171,
 * outro 172–178, which rings out.
 *
 * Each arrangement starts in the intro's last bars, so the first groove
 * drops as the open's name card leaves and the first demonstration begins
 * (film 10 s, edit.ts OPEN_TO), and ends on the song's own ending, timed so
 * the ring-out dies out a few frames before the film ends: the sum of the
 * cuts is the film's length (edit.ts) less the ring-out's tail.
 *
 * Tour (298 s): bars 11–79 straight through (the first two grooves, the
 * break under `mise use jq`'s install, the riser ending in depends), then
 * 96–151 (the third and fourth grooves, with the build under bootstrap),
 * skipping only the drumless breakdown and the last phrase of the fifth
 * groove, then 156 to the end: the break under the clone command typing,
 * the loudest groove dropping on its install rows and carrying the climax
 * (`[ci] api ready`), the wind-down under the morph's lift, the outro's
 * soft kick on the toque's drop, and the ring-out under the end card.
 *
 * Overview (82 s): bars 11–39 (the intro's tail and six phrases of the
 * first groove, under the open, the switch and `mise use jq`), then 168 to
 * the end: the wind-down under depends' lanes and the outro under the end
 * card.
 */
const BARS: Record<Edition, readonly (readonly [from: number, to: number])[]> =
  {
    tour: [
      [11, 80],
      [96, 152],
      [156, SONG_END],
    ],
    overview: [
      [11, 40],
      [168, SONG_END],
    ],
  };

/** All joins land on the song's downbeats, without time stretching. */
export function musicCuts(edition: Edition): MusicCut[] {
  let start = 0;
  return BARS[edition].map(([a, b]) => {
    const from = a * BAR;
    const to = b * BAR;
    const cut = { from, to, start, end: start + to - from };
    start = cut.end;
    return cut;
  });
}

/** Where the music ends on the film clock: the end of its ring-out. */
export const musicEnd = (edition: Edition): number =>
  musicCuts(edition).at(-1)!.end;

/**
 * The song's chords, a bar line at a time (score/harmony.ts CH): read off
 * the track's chroma, bar by bar. The grooves vamp round F minor, C minor
 * and B♭ (four bars each, F minor eight at a time); the breaks, riser,
 * build and the loudest groove sit on F minor; the outro moves C minor,
 * B♭, F minor and rings out on F minor. Through bars 112–135 the F's third
 * is unclear (score/BED.md), so the score's F there leaves it out (F5), as
 * the source's parts do. The breakdown (80–95), which no film plays, is
 * given to the nearest bar line of its wandering changes.
 */
export const SONG_CHART: readonly (readonly [bar: number, chord: Chord])[] = [
  [0, CH.Fm],
  [4, CH.Cm],
  [7, CH.Bb],
  [12, CH.Fm],
  [15, CH.Cm],
  [16, CH.Fm],
  [20, CH.Cm],
  [24, CH.Bb],
  [28, CH.Fm],
  [36, CH.Cm],
  [40, CH.Bb],
  [44, CH.Fm],
  [48, CH.Fm],
  [52, CH.Cm],
  [56, CH.Bb],
  [60, CH.Fm],
  [68, CH.Cm],
  [72, CH.Bb],
  [76, CH.Fm],
  [80, CH.Fm],
  [84, CH.Cm],
  [88, CH.Bb],
  [92, CH.Fm],
  [96, CH.Fm],
  [100, CH.Cm],
  [104, CH.Bb],
  [108, CH.Fm],
  [112, CH.F5],
  [116, CH.Cm],
  [120, CH.Bb],
  [124, CH.F5],
  [132, CH.Cm],
  [136, CH.Bb],
  [140, CH.Fm],
  [144, CH.Fm],
  [148, CH.Cm],
  [152, CH.Fm],
  [156, CH.Fm],
  [160, CH.Fm],
  [168, CH.Fm],
  [172, CH.Cm],
  [173, CH.Bb],
  [176, CH.Fm],
];

/** The chord the song plays at song second `t`. */
export function songChordAt(t: number): Chord {
  const bar = t / BAR;
  let c = SONG_CHART[0][1];
  for (const [at, ch] of SONG_CHART) if (at <= bar + 1e-6) c = ch;
  return c;
}

/**
 * The chord under film second `time` of `edition`: the song's at the bar
 * the arrangement plays there, or null after the music has ended.
 */
export function filmChordAt(edition: Edition, time: number): Chord | null {
  const cut = musicCuts(edition).find((c) => time >= c.start && time < c.end);
  return cut ? songChordAt(cut.from + time - cut.start) : null;
}

/**
 * The crossfade at a join, seconds: a splice long enough to carry no click,
 * short enough that the outgoing tail does not notch the incoming downbeat.
 */
const JOIN = 0.05;

/** Crossfade preceding tails; incoming downbeats stay at full level. */
export function musicChannel(
  source: Float32Array,
  edition: Edition,
  rate: number,
): Float32Array<ArrayBuffer> {
  const output = new Float32Array(Math.round(filmDuration(edition) * rate));
  const ramp = Math.round(JOIN * rate);
  for (const cut of musicCuts(edition)) {
    const at = Math.round(cut.start * rate);
    const from = Math.round(cut.from * rate);
    const length = Math.min(
      Math.round((cut.to - cut.from) * rate),
      output.length - at,
    );
    output.set(source.subarray(from, from + length), at);
    if (at) {
      for (let i = 0; i < ramp; i++) {
        const t = i / ramp;
        output[at - ramp + i] =
          output[at - ramp + i] * Math.cos((t * Math.PI) / 2) +
          source[from - ramp + i] * Math.sin((t * Math.PI) / 2);
      }
    }
  }
  // Only a click guard at the opening. The song supplies its own ending.
  const opening = Math.round(0.005 * rate);
  for (let i = 0; i < opening; i++) output[i] *= i / opening;
  return output;
}
