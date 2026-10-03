// The original song, independently arranged for each film. Picture cuts
// never move the music playhead or inherit the source reel's quiet breath.
import { filmDuration, type Edition } from "./edit";

export const MUSIC = {
  file: "docs/.vitepress/theme/showreel/score/music.opus",
  sha256: "4a4becfb7fc494bbe5ca18f1e81daa9d2b03710edc6c3377d846cd681ce57c6c",
  // Original WAV minus its 0.105 s lead-in; includes the complete ring-out.
  duration: 359.615,
} as const;

export interface MusicCut {
  from: number;
  to: number;
  start: number;
  end: number;
}

/** All joins land on the original 120 BPM downbeats, without time stretching. */
export function musicCuts(edition: Edition): MusicCut[] {
  // Tour: four intro bars, grooves/builds, then the complete final chorus
  // and wind-down. Remove the song's long breakdown and its drum breaks.
  // Overview: one groove followed by the complete wind-down and outro.
  const bars =
    edition === "tour"
      ? [
          [12, 44],
          [48, 80],
          [96, 110],
          [112, 156],
          [160, MUSIC.duration / 2],
        ]
      : [
          [16, 42],
          [168, MUSIC.duration / 2],
        ];
  let start = 0;
  return bars.map(([a, b]) => {
    const from = a * 2;
    const to = b * 2;
    const cut = { from, to, start, end: start + to - from };
    start = cut.end;
    return cut;
  });
}

/** Crossfade preceding tails; incoming downbeats stay at full level. */
export function musicChannel(
  source: Float32Array,
  edition: Edition,
  rate: number,
): Float32Array<ArrayBuffer> {
  const output = new Float32Array(Math.round(filmDuration(edition) * rate));
  const ramp = Math.round(0.15 * rate);
  for (const cut of musicCuts(edition)) {
    const at = Math.round(cut.start * rate);
    const from = Math.round(cut.from * rate);
    const length = Math.round((cut.to - cut.from) * rate);
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
