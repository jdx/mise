// from jdx/hk@37937824 docs/.vitepress/theme/showreel/audio.ts
// The reel's soundtrack, synthesized with the Web Audio API: oscillators,
// seeded noise, filters, and envelopes, with no samples and no network. The
// renderer (showreel-video.mjs) plays it into an OfflineAudioContext and
// muxes it into the MP4. The whole score is written in reel seconds and can
// start at any point in the reel: from there it plays the same sounds, on
// the same samples, as a render from the top (test/score.test.ts).
//
// Sound design leads: every choreographed accent in the scenes has its own
// sound. Music supports it, and ducks under the effects.
//
// This file is the master chain and the scheduling. The score itself is in
// score/: one part per section (score/index.ts), the sound palette they
// share (score/sounds.ts), and the voices and curves under them
// (score/mix.ts).
//
// The master is mixed for delivery as AAC: about -17 LUFS integrated, with
// a lookahead limiter and a soft ceiling that keep the true peak under -2
// dBTP, so the encode's overshoot stays under -1 dBTP.
//
// Adapted for mise: no breath before the end card (hk's reel gated a
// sixteenth of silence there; plan v3 has none). The score ends on the
// morph's held B♭ minor, which sounds whole until the recording enters
// (half a second before the end card) and crossfades at equal power under
// the recording's 20 ms fade-in, silent by SCORE_END; the recording's sung
// line and button on the end card (jdx's decision 5, score/song.ts) are
// mixed after this chain, at mux time (showreel-video.mjs), so the
// crossfade is timed on the output's own clock.

import type { ReelFacts } from "./bible";
import { arc, compose } from "./score";
import { SCORE_END, SONG } from "./score/song";
import {
  type Dip,
  floats,
  type Floats,
  LATENCY,
  Mix,
  type Pt,
  roomOf,
  shared,
} from "./score/mix";
import { DURATION } from "./timeline";

/** The groove's buses under the effects, before the loudness arc (score/index.ts) rides them. */
const DRUMS = 0.6;
const MUSIC = 0.78;

/**
 * Gain from the glue compressor into the limiter: 3.6 puts the 60 BPM reel
 * at about -17 LUFS integrated (3.2 left it at -17.9).
 */
const MAKEUP = 3.6;
/**
 * The limiter's threshold, dBFS: peaks above it are held down before the
 * soft ceiling. At -4.5 the master's true peak was -2.4 dBTP, but the
 * 128 kbps AAC encode overshot one woodblock-sharp transient (args beat 2,
 * where the encoder's frames fell) to -0.1 dBTP; at -5.5 the encode
 * peaks at -1.5 dBTP, for 0.1 LU of loudness.
 */
const LIMIT = -5.5;

/** Duck curves are sampled on a fixed grid from reel time 0. */
const DIP_STEP = 0.0025;

/**
 * A gain curve over the whole reel that is the product of dips: each accent
 * pulls the gain down over 8 ms and lets it recover exponentially (to under
 * 1% of its depth after five time constants).
 */
function dips(lists: readonly (readonly [readonly Dip[], number])[]): Floats {
  const n = Math.ceil((DURATION + 0.1) / DIP_STEP) + 1;
  const vals = floats(n).fill(1);
  for (const [list, scale] of lists) {
    for (const [te, d, r] of list) {
      const i0 = Math.max(0, Math.ceil((te - 0.008) / DIP_STEP));
      const i1 = Math.min(n - 1, Math.floor((te + r * 5) / DIP_STEP));
      const k = Math.exp(-DIP_STEP / r);
      let s = 0;
      for (let i = i0; i <= i1; i++) {
        const dt = i * DIP_STEP - te;
        if (dt < 0) vals[i] *= 1 - d * scale * ((dt + 0.008) / 0.008);
        else {
          // The recovery advances by a fixed ratio per step.
          s = s ? s * k : Math.exp(-dt / r);
          vals[i] *= 1 - d * scale * s;
        }
      }
    }
  }
  return vals;
}

/** Play a whole-reel dip curve on `p`, joining it at reel time `t0`. */
function dipCurve(m: Mix, p: AudioParam, t0: number, vals: Floats): void {
  const k = Math.min(vals.length - 2, Math.ceil(t0 / DIP_STEP - 1e-9));
  const rest = vals.slice(k);
  p.setValueCurveAtTime(rest, m.at(k * DIP_STEP), (rest.length - 1) * DIP_STEP);
}

/**
 * The two duck curves, from the accents and the kicks the score recorded as
 * it was built.
 */
function ducks(m: Mix): { drums: Floats; music: Floats } {
  // A gentler pump than a club kick's: the music breathes with the kicks,
  // and a tune note on the downbeat still speaks.
  const pump = m.kicks.map((t): Dip => [t, 0.25, 0.1]);
  return {
    // The groove ducks under the effects; the bass and the pads also pump with the kicks.
    drums: dips([[m.ducks, 0.6]]),
    music: dips([
      [m.ducks, 1],
      [pump, 1],
    ]),
  };
}

/**
 * Schedule the soundtrack into `dest`, starting `from` seconds into the reel
 * at context time `when`, on a context that has not started rendering. Pass
 * the same `facts` the picture draws, so cues that follow them land with it.
 */
export function playScore(
  ac: BaseAudioContext,
  dest: AudioNode,
  from: number,
  when: number,
  facts: ReelFacts | null = null,
): void {
  if (!(from < DURATION - 0.03)) return;
  const sh = shared(ac);

  // Master: a DC blocker, a touch less sub and more air, a 16 kHz lowpass,
  // a glue compressor, makeup, a limiter, a soft ceiling, and the handoff
  // to the recording.
  const pre = ac.createGain();
  // Staccato bass and falling thuds are gated low tones, each leaving a
  // little DC and subsonic rumble; a microphone's coupling would take it
  // out, so this does, below anything a speaker plays.
  const dc = ac.createBiquadFilter();
  dc.type = "highpass";
  dc.frequency.value = 18;
  dc.Q.value = -3;
  // A bass and its thuds can carry half the mix's energy below 120 Hz,
  // where laptop and phone speakers play little of it and it only feeds the
  // limiter; their grit carries them above it. A 1.5 dB shelf is enough:
  // at 3 dB the reel's 20 to 80 Hz sat 9 dB under hk's on headphones and
  // big speakers, where the band had no floor. The air is a lift of 2 dB:
  // mise's band is plucked, struck and bowed wood and gut, darker than
  // hk's tambourine and claps, and its top needs the help; more tires the
  // ear on earbuds.
  const low = ac.createBiquadFilter();
  low.type = "lowshelf";
  low.frequency.value = 100;
  low.gain.value = -1.5;
  const air = ac.createBiquadFilter();
  air.type = "highshelf";
  air.frequency.value = 7000;
  air.gain.value = 2;
  // The organ, the cello and the pizzicato's bodies pile up round 330 Hz,
  // and a laptop hears the band's articulation at 2 to 4 kHz: a little
  // less of the one and more of the other, which puts the whole reel's
  // octave balance near hk's.
  const mud = ac.createBiquadFilter();
  mud.type = "peaking";
  mud.frequency.value = 330;
  mud.Q.value = 0.9;
  mud.gain.value = -2;
  const presence = ac.createBiquadFilter();
  presence.type = "peaking";
  presence.frequency.value = 2800;
  presence.Q.value = 0.7;
  presence.gain.value = 1.5;
  // Nothing above 16 kHz: the AAC encode drops it anyway, and a click's
  // top octave removed after the limiter comes back as overshoot. Two
  // Butterworth sections, 24 dB an octave.
  const tops = [0.54, 1.31].map((q) => {
    const f = ac.createBiquadFilter();
    f.type = "lowpass";
    f.frequency.value = 16000;
    // Q here is resonance in dB: the two sections' Butterworth Qs.
    f.Q.value = 20 * Math.log10(q);
    return f;
  });
  const comp = ac.createDynamicsCompressor();
  comp.threshold.value = -18;
  comp.knee.value = 10;
  comp.ratio.value = 2;
  comp.attack.value = 0.005;
  const makeup = ac.createGain();
  makeup.gain.value = MAKEUP;
  const limiter = ac.createDynamicsCompressor();
  limiter.threshold.value = LIMIT;
  limiter.knee.value = 0;
  limiter.ratio.value = 20;
  limiter.attack.value = 0;
  limiter.release.value = 0.12;
  const trim = ac.createGain();
  // Undo the limiter's automatic makeup (the spec's 0.6 power of the gain
  // its curve applies at full scale), and halve, because the ceiling
  // curve's domain is ±2.
  trim.gain.value = 0.5 * 10 ** ((0.6 * LIMIT * (1 - 1 / 20)) / 20);
  const ceiling = ac.createWaveShaper();
  ceiling.curve = sh.ceiling;
  const tail = ac.createGain();
  pre
    .connect(dc)
    .connect(low)
    .connect(air)
    .connect(mud)
    .connect(presence)
    .connect(tops[0])
    .connect(tops[1])
    .connect(comp)
    .connect(makeup)
    .connect(limiter)
    .connect(trim)
    .connect(ceiling)
    .connect(tail)
    .connect(dest);

  const sfx = ac.createGain();
  sfx.connect(pre);
  const drums = ac.createGain();
  const drumDuck = ac.createGain();
  drums.connect(drumDuck).connect(pre);
  const music = ac.createGain();
  const musicDuck = ac.createGain();
  music.connect(musicDuck).connect(pre);

  const verb = ac.createGain();
  const conv = ac.createConvolver();
  conv.buffer = roomOf(ac, sh);
  const verbLow = ac.createBiquadFilter();
  verbLow.type = "highpass";
  verbLow.frequency.value = 220;
  const verbOut = ac.createGain();
  verbOut.gain.value = 2.5;
  verb.connect(conv).connect(verbLow).connect(verbOut).connect(pre);

  const m = new Mix(ac, sh, from, when, { sfx, drums, music }, verb);
  // The loudness arc rides the groove's two buses, section by section.
  const fader = arc();
  m.set(
    drums.gain,
    fader.map(([t, v]): Pt => [t, DRUMS * v]),
  );
  m.set(
    music.gain,
    fader.map(([t, v]): Pt => [t, MUSIC * v]),
  );
  // Every voice, recording the accents and kicks the ducks follow.
  compose(m, facts);
  // An offline render wires each voice in shortly before it starts, so the
  // thousands not yet due cost it nothing (score/mix.ts).
  m.wire();
  const p = ducks(m);
  const start = m.floor;

  // A fresh compressor starts clamped down and takes ~200 ms to open; a fast
  // release until just after the first sound lets it settle at once.
  comp.release.value = 0.001;
  comp.release.setValueAtTime(0.2, m.at(start) + 0.03);
  dipCurve(m, drumDuck.gain, start, p.drums);
  dipCurve(m, musicDuck.gain, start, p.music);
  // Everything, reverb included, holds until the recording enters and
  // crossfades out under its 20 ms fade-in at equal power, so the handoff
  // keeps its loudness; it is silent from then to SCORE_END. It sits after
  // the limiter, so it runs on the output's clock, LATENCY late, as the
  // recording does at mux time.
  m.set(tail.gain, handoff(), from, LATENCY);
}

/**
 * The master's last gain, on the output's clock: 1 until the recording
 * enters, then an equal-power crossfade against the recording's linear
 * fade-in (score/song.ts), so the two together hold the chord's loudness,
 * silent once the recording is whole and until SCORE_END and after.
 */
export function handoff(): Pt[] {
  const end = Math.min(SCORE_END, DURATION);
  const [line] = SONG.segments;
  const enter = Math.min(line.at, end);
  const fade = Math.min(line.fadeIn, end - enter);
  const fall: Pt[] = [[enter, 1]];
  for (let k = 1; k <= 8; k++) {
    const u = k / 8;
    fall.push([enter + fade * u, Math.sqrt(Math.max(0, 1 - u * u))]);
  }
  fall.push([end, 0]);
  return fall;
}
