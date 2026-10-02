// The bed: the reel's music, one recorded instrumental track under the
// sound design, arranged on reel time. The synthesized band it replaces
// (jdx's call: the band goes, every effect stays) was a voice per act; the
// bed is a single file, so it can be a calmer, produced track, and it
// carries its own ending under the end card.
//
// The track is "mise screenreel", an original instrumental jdx made in
// Suno (2026-10-02; provenance and ownership in BED.md): a 5:59.72 WAV on
// an exact 120 BPM clock, so it is trimmed to its first downbeat (0.105
// s), not stretched, and its bars arranged act by act onto the reel's 218
// by docs/.vitepress/showreel-bed/bed.toml (`mise run
// docs:showreel-bed`), its outro ringing out under the end card. The fit
// levels it at -32 LUFS; gainDb and duck below were set on it in the
// render.
//
// The file is Ogg Opus, 48 kHz stereo, and already on reel time: decoded
// sample 0 (after Opus's pre-skip, which Chromium and ffmpeg both honour)
// is reel time 0, and it runs the whole reel, end card included, 436 s.
// The renderer (showreel-video.mjs) reads it in node, checks its sha256,
// hands its bytes to the page and decodes it there with decodeAudioData;
// audio.ts plays it on the music bus, under the effects: it ducks under
// the accents that ask for it (Mix.duck: the seats, thunks, stabs, slams,
// the bell, the pilot light, the rewind and Act III's clicks), follows the
// sections' fader (score/index.ts arc), and goes
// through the master chain and its limiter like everything else. Nothing
// is fetched: the file is committed beside the score.

export const BED = {
  /** From the checkout's root. */
  file: "docs/.vitepress/theme/showreel/score/bed.opus",
  /** The renderer refuses any other file. */
  sha256: "251d70c8dbee11bc82401bc4379ee3e510dab9734b2239c1b7304e534c01924a",
  /**
   * Its gain into the music bus, dB, before the fader and the master.
   * The fit tool levels a bed at -32 LUFS, where the master's makeup lifts
   * it about 11 dB; this trims the whole reel onto the -17 LUFS integrated
   * target (a whole-reel render, ffmpeg ebur128; BED.md).
   */
  gainDb: 0.7,
  /**
   * How deep the bed ducks under the effects, as a fraction of the depth
   * each accent asks for (Mix.duck): the band's thin, articulated parts
   * ducked at full depth; a produced, dense bed pumps audibly at that, so
   * it dips about half as far.
   */
  duck: 0.5,
} as const;
