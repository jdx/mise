// Cut only the effects with the picture. Arrange the original song on its
// own musical clock, then mix both through one master. The old source
// reel's fader and music edits never enter the delivered films.
import { playFilmMix, playScore } from "./audio";
import type { ReelFacts } from "./bible";
import { cuts, filmDuration, type Edition } from "./edit";
import { musicChannel } from "./film-music";
import { type Dip, LATENCY } from "./score/mix";
import { DURATION } from "./timeline";

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
  const sourceAccents = playScore(
    sourceContext,
    sourceContext.destination,
    0,
    preRoll,
    facts,
    null,
    true,
    false,
  );
  const source = await sourceContext.startRendering();
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
    if (cut.brand) continue;
    const offset = Math.round((preRoll - LATENCY + cut.from) * rate);
    const at = Math.round(cut.start * rate);
    const n = Math.round((cut.end - cut.start) * rate);
    const incoming = !k || list[k - 1].to !== cut.from || !!list[k - 1].brand;
    const outgoing =
      k === list.length - 1 ||
      list[k + 1].from !== cut.to ||
      !!list[k + 1].brand;
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
