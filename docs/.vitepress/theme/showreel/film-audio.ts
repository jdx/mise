// Cut only the effects with the picture. Arrange the original song on its
// own musical clock, then mix both through one master. The source reel's
// section fader and its bed's arrangement never enter the delivered films.
// The pitched effects are struck on the chord the film's music plays under
// them (film-music.ts SONG_CHART, score/harmony.ts setFilmChart), not on the
// bed's chord at the source moment.
import { playFilmMix, playScore } from "./audio";
import type { ReelFacts } from "./bible";
import { continuous, cuts, filmDuration, type Edition } from "./edit";
import { filmChordAt, musicChannel } from "./film-music";
import { type Chord, setFilmChart } from "./score/harmony";
import { type Dip, LATENCY } from "./score/mix";
import { DURATION } from "./timeline";

/**
 * The chord the film's music plays under reel second `t`: the song's at the
 * film moment the picture's cut maps `t` to, or null where the film does
 * not play that moment (a cut section) or its music has ended.
 */
export function reelChord(edition: Edition, t: number): Chord | null {
  const cut = cuts(edition).find((c) => t >= c.from && t < c.to);
  return cut ? filmChordAt(edition, cut.start + (t - cut.from)) : null;
}

/** Render edited effects and independently arranged music at the supplied rate. */
export async function filmSoundtrack(
  facts: ReelFacts | null,
  song: AudioBuffer,
  edition: Edition,
  rate: number,
): Promise<AudioBuffer> {
  const preRoll = 0.2;
  const sourceContext = new OfflineAudioContext(
    2,
    Math.ceil(rate * (DURATION + preRoll)),
    rate,
  );
  // The parts read their chords while they are wired in, which the offline
  // render does as it goes, so the film's chart stays set until it is done.
  setFilmChart((t) => reelChord(edition, t));
  let source: AudioBuffer;
  let sourceAccents: readonly Dip[];
  try {
    sourceAccents = playScore(
      sourceContext,
      sourceContext.destination,
      0,
      preRoll,
      facts,
      null,
      true,
      false,
    );
    source = await sourceContext.startRendering();
  } finally {
    setFilmChart(null);
  }
  const length = Math.round(filmDuration(edition) * rate);
  const effects = new AudioBuffer({
    numberOfChannels: 2,
    length,
    sampleRate: rate,
  });
  const music = new AudioBuffer({
    numberOfChannels: 2,
    length,
    sampleRate: rate,
  });
  const accents: Dip[] = [];
  const list = cuts(edition);
  for (let k = 0; k < list.length; k++) {
    const cut = list[k];
    const offset = Math.round((preRoll - LATENCY + cut.from) * rate);
    const at = Math.round(cut.start * rate);
    const n = Math.round((cut.end - cut.start) * rate);
    // A cut the source did not make gets a short ramp either side, so no
    // effect's tail or attack is cut mid-sample; the source's own bar lines
    // carry straight on.
    const incoming = !k || !continuous(list[k - 1], cut);
    const outgoing = k === list.length - 1 || !continuous(cut, list[k + 1]);
    const ramp = Math.round(rate * 0.008);
    for (let channel = 0; channel < 2; channel++) {
      const from = source.getChannelData(channel);
      const to = effects.getChannelData(channel);
      for (let i = 0; i < n; i++) {
        const gain = Math.min(
          1,
          incoming ? i / ramp : 1,
          outgoing ? (n - 1 - i) / ramp : 1,
        );
        to[at + i] = from[offset + i] * gain;
      }
    }
    for (const [time, depth, release] of sourceAccents) {
      if (time >= cut.from && time < cut.to)
        accents.push([cut.start + time - cut.from, depth, release]);
    }
  }
  for (let channel = 0; channel < 2; channel++)
    music.copyToChannel(
      musicChannel(song.getChannelData(channel), edition, rate),
      channel,
    );
  const context = new OfflineAudioContext(
    2,
    length + Math.round(preRoll * rate),
    rate,
  );
  playFilmMix(context, context.destination, effects, music, accents, preRoll);
  const mixed = await context.startRendering();
  const output = new AudioBuffer({
    numberOfChannels: 2,
    length,
    sampleRate: rate,
  });
  for (let channel = 0; channel < 2; channel++)
    output.copyToChannel(
      mixed.getChannelData(channel).subarray(Math.round(preRoll * rate)),
      channel,
    );
  return output;
}
