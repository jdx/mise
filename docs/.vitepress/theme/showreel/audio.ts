// from jdx/hk@37937824 docs/.vitepress/theme/showreel/audio.ts
// The reel's soundtrack: the sound design, synthesized with the Web Audio
// API (oscillators, seeded noise, filters, and envelopes), over the bed,
// one recorded instrumental track committed beside the score (score/bed.ts;
// nothing is fetched). The renderer (showreel-video.mjs) decodes the bed in
// the page, plays both into an OfflineAudioContext and muxes the result
// into the MP4. The whole score is written in reel seconds and can start at
// any point in the reel: from there it plays the same sounds, on the same
// samples, as a render from the top (test/score.test.ts), the bed's
// included.
//
// Sound design leads: every choreographed accent in the scenes has its own
// sound. The bed supports it, and ducks under the effects.
//
// This file is the master chain and the scheduling. The score itself is in
// score/: one part per section (score/index.ts), the sound palette they
// share (score/sounds.ts, score/props.ts, score/instruments.ts), and the
// voices and curves under them (score/mix.ts).
//
// The master is mixed for delivery as AAC: about -17 LUFS integrated, with
// a lookahead limiter and a soft ceiling that keep the true peak under -2
// dBTP, so the encode's overshoot stays under -1 dBTP.
//
// Adapted for mise: no breath before the end card (hk's reel gated a
// sixteenth of silence there; plan v3 has none), and the music is the bed,
// which carries its own ending under the end card: the synthesized band
// and the sung recording it handed the end card to are gone (jdx's call).

import type { ReelFacts } from "./bible";
import { arc, compose } from "./score";
import { BED } from "./score/bed";
import {
  type Dip,
  floats,
  type Floats,
  LATENCY,
  Mix,
  roomOf,
  shared,
} from "./score/mix";
import { DURATION } from "./timeline";

/**
 * Gain from the glue compressor into the limiter: 3.6 put the band's 60
 * BPM reel at about -17 LUFS integrated (3.2 left it at -17.9). The bed's
 * own level is BED.gainDb, set against it.
 */
const MAKEUP = 3.6;
/**
 * The limiter's threshold, dBFS: peaks above it are held down before the
 * soft ceiling. At -4.5 the master's true peak was -2.4 dBTP, but an AAC
 * encode at 128 kbps overshot one woodblock-sharp transient (args beat 2,
 * where the encoder's frames fell) to -0.1 dBTP; at -5.5 that encode
 * peaked at -1.5 dBTP, for 0.1 LU of loudness. The encoder now runs at 160
 * kbps (showreel-video.mjs), which overshoots less.
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

/** The music bus's duck curve, from the accents the score recorded as it was built. */
function ducks(m: Mix): Floats {
  // The bed is dense where the band was thin, so it dips only BED.duck as
  // deep as each accent asks.
  return dips([[m.ducks, BED.duck]]);
}

/**
 * Play the bed (score/bed.ts) into `into`, starting `from` seconds into the
 * reel at context time `when`: its frame k is reel frame k, so it starts on
 * the first whole frame of the reel at or after `from` that the context
 * can play (context time 0 or later), and stops at the reel's end. It is
 * scheduled on exact frame times, not through Mix.at, which sits half a
 * frame early for the voices' sake: a buffer started between frames is
 * interpolated across them, which would play the whole bed as the average
 * of neighbouring samples, a lowpass (-3 dB at 12 kHz). Its read offset is
 * a whole number of frames too, so a render from any start plays the same
 * samples on the same output frames as a render from the top. It runs
 * LATENCY early, as every voice does, so it leaves the master's
 * compressors on the frame.
 */
function playBed(
  ac: BaseAudioContext,
  into: AudioNode,
  from: number,
  when: number,
  bed: AudioBuffer,
): void {
  const sr = ac.sampleRate;
  if (bed.sampleRate !== sr)
    throw new Error(
      `the bed is decoded at ${bed.sampleRate} Hz, not the score's ${sr} Hz`,
    );
  // The context frame reel frame 0 plays on (negative from mid-reel).
  const zero = Math.round((when - LATENCY - from) * sr);
  const first = Math.max(0, Math.ceil(from * sr - 1e-6), -zero);
  const last = Math.min(bed.length, Math.round(DURATION * sr));
  if (!(first < last)) return;
  const src = ac.createBufferSource();
  src.buffer = bed;
  const gain = ac.createGain();
  gain.gain.value = 10 ** (BED.gainDb / 20);
  src.connect(gain).connect(into);
  src.start((zero + first) / sr, first / sr);
  src.stop((zero + last) / sr);
}

/** Shared delivery chain for the source reel and the independently mixed films. */
function master(ac: BaseAudioContext, dest: AudioNode) {
  // Master: a DC blocker, a touch less sub and more air, a 16 kHz lowpass,
  // a glue compressor, makeup, a limiter and a soft ceiling. The EQ was
  // voiced on the synthesized band and kept as it was for the bed, which
  // goes through it too (score/BED.md).
  const pre = ac.createGain();
  // Falling thuds (and, on the band, its staccato bass) are gated low
  // tones, each leaving a little DC and subsonic rumble; a microphone's
  // coupling would take it out, so this does, below anything a speaker
  // plays.
  const dc = ac.createBiquadFilter();
  dc.type = "highpass";
  dc.frequency.value = 18;
  dc.Q.value = -3;
  // As voiced on the band: its bass and thuds could carry half the mix's
  // energy below 120 Hz, where laptop and phone speakers play little of it
  // and it only fed the limiter; their grit carried them above it. A 1.5
  // dB shelf was enough: at 3 dB the reel's 20 to 80 Hz sat 9 dB under
  // hk's on headphones and big speakers, where the band had no floor. The
  // air is a lift of 2 dB: the band was plucked, struck and bowed wood and
  // gut, darker than hk's tambourine and claps, and its top needed the
  // help; more tires the ear on earbuds.
  const low = ac.createBiquadFilter();
  low.type = "lowshelf";
  low.frequency.value = 100;
  low.gain.value = -1.5;
  const air = ac.createBiquadFilter();
  air.type = "highshelf";
  air.frequency.value = 7000;
  air.gain.value = 2;
  // As voiced on the band: the organ, the cello and the pizzicato's bodies
  // piled up round 330 Hz, and a laptop heard the band's articulation at 2
  // to 4 kHz: a little less of the one and more of the other put the whole
  // reel's octave balance near hk's.
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
  ceiling.curve = shared(ac).ceiling;
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
    .connect(dest);

  return { input: pre, compressor: comp };
}

/**
 * Schedule the soundtrack into `dest`, starting `from` seconds into the reel
 * at context time `when`, on a context that has not started rendering. Pass
 * the same `facts` the picture draws, so cues that follow them land with it,
 * and the decoded bed (score/bed.ts, at the context's rate) to play the
 * music under them; with none, only the sound design plays.
 */
export function playScore(
  ac: BaseAudioContext,
  dest: AudioNode,
  from: number,
  when: number,
  facts: ReelFacts | null = null,
  bed: AudioBuffer | null = null,
  effects = true,
  mastered = true,
): readonly Dip[] {
  if (!(from < DURATION - 0.03)) return [];
  const sh = shared(ac);

  const { input: pre, compressor: comp } = mastered
    ? master(ac, dest)
    : { input: ac.createGain(), compressor: null };
  if (!mastered) pre.connect(dest);

  const sfx = ac.createGain();
  sfx.connect(pre);
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

  const m = new Mix(ac, sh, from, when, sfx, verb);
  // The loudness arc rides the bed, section by section.
  m.set(music.gain, arc());
  // Every voice, recording the accents the bed ducks under.
  if (effects) compose(m, facts);
  // An offline render wires each voice in shortly before it starts, so the
  // thousands not yet due cost it nothing (score/mix.ts).
  m.wire();
  // The bed, dry: it brings its own room.
  if (bed) playBed(ac, music, from, when, bed);
  const start = m.floor;

  // A fresh compressor starts clamped down and takes ~200 ms to open; a fast
  // release until just after the first sound lets it settle at once.
  if (comp) {
    comp.release.value = 0.001;
    comp.release.setValueAtTime(0.2, m.at(start) + 0.03);
  }
  dipCurve(m, musicDuck.gain, start, ducks(m));
  return m.ducks;
}

/** Mix edited effects with uninterrupted film music, through one master. */
export function playFilmMix(
  ac: BaseAudioContext,
  dest: AudioNode,
  effects: AudioBuffer,
  bed: AudioBuffer,
  accents: readonly Dip[],
  when: number,
): void {
  const { input, compressor } = master(ac, dest);
  const start = when - LATENCY;
  compressor.release.value = 0.001;
  compressor.release.setValueAtTime(0.2, start + 0.03);
  const sfx = ac.createBufferSource();
  sfx.buffer = effects;
  sfx.connect(input);
  sfx.start(start);
  const music = ac.createBufferSource();
  music.buffer = bed;
  const gain = ac.createGain();
  gain.gain.value = 10 ** (BED.gainDb / 20);
  const duck = ac.createGain();
  const curve = dips([[accents, BED.duck]]);
  const end = Math.ceil(bed.duration / DIP_STEP) + 1;
  duck.gain.setValueCurveAtTime(
    curve.slice(0, end),
    start,
    (end - 1) * DIP_STEP,
  );
  music.connect(gain).connect(duck).connect(input);
  music.start(start);
}
