// The recording on the end card (jdx's decision 5, plan v3 §5): chorus 1's
// sung line, "It's mise-en-place for dev machines, precise and
// operational", then the record's own button and ring-out, both cut from
// score/mise-en-place.mp3 (the original theme song, kept beside the score
// since the site replaced it with mise run) and mixed after the score's master chain at
// mux time (showreel-video.mjs), at a fixed gain. Song times are seconds of
// the decoded audio, where 0 is the first decoded sample (ffmpeg's decode,
// the one the plan measured on). The end card keys its picture to the
// onsets here, not to the grid; the score owns everything before SCORE_END.

import { sec } from "../timeline";

/** Where "mise" is sung in the recording: it lands on the end card's downbeat. */
const MISE = 72.3;
/** The end card's downbeat, reel seconds. */
const E0 = sec("end").start;

export interface Segment {
  /** Song seconds, in and out. */
  from: number;
  to: number;
  /** Reel seconds the segment's first sample plays at. */
  at: number;
  fadeIn: number;
  fadeOut: number;
}

export const SONG = {
  /** From the checkout's root. */
  file: "docs/.vitepress/theme/showreel/score/mise-en-place.mp3",
  sha256: "8be51598a9d6195c7fbe852dae24b15f27754d79c8fa4aa2cfb08b89d7d08497",
  /**
   * Both segments, after the master chain: about -16 LUFS for the line, so
   * jdx's voice arrives level with the climax it answers rather than 2.5 LU
   * under it. True peaks -3.5 and -2.6 dBTP.
   */
  gainDb: -1,
  segments: [
    // Chorus 1's line, from the gap before "It's" to the bar after "-al".
    // It ends in that bar's one quiet moment: the hit on its beat 3
    // (79.06 s) has died to -40 dB RMS by 79.245, 5 ms before the band's
    // pickup into verse 3 starts (79.25), so the 0.1 s fade shapes a
    // decay instead of chopping the pickup, which a cut at 79.45 did at
    // -24 dB RMS.
    {
      from: 71.78,
      to: 79.245,
      at: E0 + (71.78 - MISE),
      fadeIn: 0.02,
      fadeOut: 0.1,
    },
    // The button and ring-out after a rest over the extra bar's beat 4:
    // its first hit (209.115 s) takes the downbeat after the extra bar.
    // The 10 ms fade-in rounds the coda's tail it enters on (-29 dB RMS)
    // before the hit.
    { from: 209.08, to: 212.9, at: E0 + 7.48, fadeIn: 0.01, fadeOut: 0.05 },
  ] satisfies Segment[],
} as const;

/** Reel time of song time `t` in segment `k`. */
export const reelTime = (k: 0 | 1, t: number): number =>
  SONG.segments[k].at + (t - SONG.segments[k].from);

/**
 * The end card's cues, reel seconds: the plan's measured onsets (Whisper and
 * the vocal stem, good to about 0.1 s for words; the hits to a few ms).
 */
export const CUE = {
  /** The recording comes in, under the score's held chord. */
  recordingIn: SONG.segments[0].at,
  its: reelTime(0, 71.87),
  mise: reelTime(0, MISE),
  en: reelTime(0, 72.66),
  place: reelTime(0, 72.98),
  dev: reelTime(0, 73.78),
  precise: reelTime(0, 75.08),
  operational: reelTime(0, 76.1),
  /** The end of "-al" (Whisper). */
  operationalEnd: reelTime(0, 77.5),
  /** The vocal stem below -50.7 dBFS. */
  voiceGone: reelTime(0, 78.55),
  hit1: reelTime(1, 209.115),
  hit2: reelTime(1, 209.864),
  /** -62 dBFS. */
  ringOut: reelTime(1, 212.8),
} as const;

/**
 * The score is silent from here: its held chord sounds whole until the
 * recording enters (SONG.segments[0].at, 3:13.08) and crossfades at equal
 * power under the recording's 20 ms fade-in (audio.ts handoff).
 */
export const SCORE_END = SONG.segments[0].at + 0.05;
