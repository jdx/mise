#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "librosa>=0.11",
#   "numpy>=2",
#   "scipy>=1.13",
#   "soundfile>=0.13",
#   "soxr>=0.5",
# ]
#
# [tool.uv]
# # Resolve only what was published by this date, so the same packages are
# # used on every machine until it is moved on purpose.
# exclude-newer = "2026-10-02T00:00:00Z"
# ///
"""Fit a downloaded track to the showreel's grid as score/bed.opus.

usage: bed.py sections
       bed.py analyse SRC [--bpm X] [--first-downbeat S] [--beats-per-bar N] ...
       bed.py build SRC [--spec bed.toml] [--out bed.opus] [--semitones N|auto] ...
       bed.py verify FILE

The reel (docs/.vitepress/theme/showreel) plays its bed on the music bus from
sample 0 at reel time 0, for exactly the reel's 436 s, and lays every SFX and
picture cue on a 60 BPM grid. The bed is a 120 BPM 4/4 track, so one of its
bars is 2.000 s, 96000 frames at 48 kHz, two of the reel's beats, and every
one of its bar lines must sit on that grid: a reel beat is two track beats.
A track off Suno is at or near 120 BPM, starts its first bar wherever its
intro puts it, and lasts as long as it likes, so this tool:

  (a) decodes SRC with ffmpeg and resamples it to 48 kHz stereo float with
      soxr (Homebrew's ffmpeg has no soxr; the Python binding is the same
      library, at its very-high-quality setting)
  (b) tracks the beats (librosa), folds a half- or double-time read onto
      the tempo near --expect-bpm, refines each beat to its attack (1 ms),
      fits one constant tempo through them and reports the residuals, picks
      the bar's phase (the downbeat) from the accents, the low end and the
      harmonic changes, and guesses the key and a chord per bar
  (c) stretches the track with Rubber Band's R3 engine through a time map
      that sends source downbeat k to frame k * 96000, optionally shifting
      the pitch (-p) in the same pass; a track already on the grid (an
      exact 120 BPM clock, read with --bpm 120 --first-downbeat S) with no
      shift is only trimmed to its first downbeat
  (d) arranges the stretched bars on reel time from a spec (bed.toml):
      segments of source bars placed at reel bars, equal-power crossfades
      at the bar lines, and a ringing tail that must be silent by 436.0 s
  (e) sets one gain so the bed measures --lufs integrated (EBU R128, ffmpeg
      ebur128) with its sample peak at or below --peak dBFS. The default,
      -32 LUFS, is staged against the master chain (audio.ts), which lifts
      the bed about 11 dB (its makeup, less the glue compressor): measured
      on "mise screenreel" at BED.gainDb 0.7, the whole reel comes out at
      -17.0 LUFS integrated, the target; a bed at -20 LUFS put it at -11.6,
      with the limiter clamped throughout
  (f) encodes Ogg Opus 48 kHz stereo (libopus, 192 kb/s, bitexact so the
      same input gives the same bytes and the same sha256)
  (g) verifies the file it wrote: decodes it, checks its length frame for
      frame, tracks the beats again and measures every bar line with a
      clear onset (a rise of CLEAR_DB) against the 2 s grid, failing on
      the median and the 90th percentile (5 and 15 ms), then prints the
      sha256 line for score/bed.ts (BED's other fields are set by ear in
      the render, not here)

Every step's intermediate file stays in --work (stretched.wav if it
stretched, arranged.wav, bed.wav, analysis.json, chords.json), so each one
can be listened to.

Nothing here is read by the render: the render cache key hashes the reel's
directory, where only bed.opus lands. See README.md for the workflow.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent
REEL_DIR = ROOT / "docs/.vitepress/theme/showreel"
DEFAULT_SPEC = HERE / "bed.toml"
# Where bed.ts expects it, from the checkout's root (like SONG.file was).
BED_FILE = "docs/.vitepress/theme/showreel/score/bed.opus"

SR = 48000
# The bed's bar: 4/4 at 120 BPM. Fixed, not a parameter: the reel's grid
# (60 BPM, a beat a second) is what the bar has to divide.
TRACK_BPM = 120.0
BAR_S = 2.0
BAR_FRAMES = int(BAR_S * SR)
# A clear onset, for the grid check: a rise of this many dB over 4 ms (a
# kick, a click), not a pad's or an arpeggio's.
CLEAR_DB = 20.0
# librosa's tuned defaults for beat tracking and chroma.
AN_SR = 22050
AN_HOP = 512

NOTES = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"]
# Krumhansl–Kessler key profiles.
KK_MAJOR = np.array([6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88])
KK_MINOR = np.array([6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17])


def say(msg: str) -> None:
    print(f"showreel-bed: {msg}", file=sys.stderr, flush=True)


def die(msg: str, code: int = 1) -> None:
    say(f"error: {msg}")
    sys.exit(code)


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if p.returncode != 0:
        die(f"{cmd[0]} failed ({p.returncode}):\n{p.stderr.strip()[-2000:]}")
    return p


def clock(t: float) -> str:
    m, s = divmod(t, 60)
    return f"{int(m)}:{s:06.3f}"


def db(x: float) -> float:
    return 20 * math.log10(x) if x > 0 else -math.inf


# --- the reel ----------------------------------------------------------------


@dataclass
class ReelSection:
    id: str
    act: str
    beats: int
    start: float  # seconds
    end: float

    @property
    def bar(self) -> float:
        return self.start / BAR_S

    @property
    def bars(self) -> float:
        return (self.end - self.start) / BAR_S


@dataclass
class Reel:
    bpm: float
    sections: list[ReelSection]

    @property
    def seconds(self) -> float:
        return self.sections[-1].end

    @property
    def bars(self) -> int:
        return int(round(self.seconds / BAR_S))

    @property
    def frames(self) -> int:
        return int(round(self.seconds * SR))

    def at(self, t: float) -> ReelSection:
        for s in self.sections:
            if s.start <= t + 1e-9 < s.end:
                return s
        return self.sections[-1]


def load_reel() -> Reel:
    """The reel's sections from sections.json, on the clock timeline.ts sets."""
    m = re.search(r"export const BPM = (\d+(?:\.\d+)?);", (REEL_DIR / "timeline.ts").read_text())
    if not m:
        die("no `export const BPM = N;` in timeline.ts")
    bpm = float(m.group(1))
    sec = 60.0 / bpm
    data = json.loads((REEL_DIR / "sections.json").read_text())
    sections = [
        ReelSection(s["id"], s.get("actLabel", s["act"]), s["beats"], s["start"] * sec, s["end"] * sec)
        for s in data
    ]
    reel = Reel(bpm, sections)
    if abs(reel.seconds / BAR_S - reel.bars) > 1e-9:
        die(f"the reel is {reel.seconds} s, not a whole number of {BAR_S} s bars")
    return reel


def print_sections(reel: Reel) -> None:
    print(f"The reel: {len(reel.sections)} sections, {reel.seconds:g} s at {reel.bpm:g} BPM = "
          f"{reel.bars} bars of the bed ({TRACK_BPM:g} BPM 4/4, {BAR_S:g} s, {BAR_FRAMES} frames).")
    print("A reel beat is two bed beats; a section of N reel beats is N/2 bed bars.\n")
    print(f"  {'#':>2}  {'section':<10} {'act':<14} {'beats':>5}  {'starts':>9}  {'bar':>6}  {'bars':>5}")
    for i, s in enumerate(reel.sections):
        print(f"  {i:>2}  {s.id:<10} {s.act:<14} {s.beats:>5}  {clock(s.start):>9}  "
              f"{s.bar:>6g}  {s.bars:>5g}")
    print(f"  {'':>2}  {'(end)':<10} {'':<14} {'':>5}  {clock(reel.seconds):>9}  {reel.bars:>6}")


# --- audio i/o ---------------------------------------------------------------


def need(tool: str, hint: str) -> None:
    if not shutil.which(tool):
        die(f"{tool} is not on PATH ({hint})")


def decode48(src: Path, dst: Path) -> tuple[np.ndarray, int]:
    """SRC as 48 kHz stereo float32 at dst; returns (frames x 2, the source's rate)."""
    import soundfile as sf
    import soxr

    raw = dst.with_name("decoded.wav")
    run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(src), "-map", "0:a:0",
         "-ac", "2", "-c:a", "pcm_f32le", str(raw)])
    x, rate = sf.read(raw, dtype="float32", always_2d=True)
    if rate != SR:
        say(f"resampling {rate} Hz -> {SR} Hz (soxr VHQ)")
        x = soxr.resample(x, rate, SR, quality="VHQ").astype(np.float32)
    sf.write(dst, x, SR, subtype="FLOAT")
    raw.unlink()
    return x, rate


def probe(path: Path) -> dict:
    p = run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
             "stream=codec_name,sample_rate,channels", "-of", "json", str(path)])
    return json.loads(p.stdout)["streams"][0]


def loudness(path: Path) -> dict:
    """EBU R128 integrated loudness, range and true peak, as ffmpeg's ebur128 measures them."""
    p = subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-nostats", "-i", str(path),
                        "-af", "ebur128=peak=true:framelog=quiet", "-f", "null", "-"],
                       capture_output=True, text=True)
    if p.returncode != 0:
        die(f"ffmpeg ebur128 failed:\n{p.stderr[-2000:]}")
    text = p.stderr[p.stderr.rfind("Summary:"):]

    def num(pat: str) -> float:
        m = re.search(pat, text)
        if not m:
            die(f"no {pat!r} in ebur128's summary:\n{text}")
        return -math.inf if m.group(1) == "-inf" else float(m.group(1))

    return {"lufs": num(r"I:\s+(-inf|-?[\d.]+) LUFS"), "lra": num(r"LRA:\s+(-inf|-?[\d.]+) LU"),
            "true_peak": num(r"Peak:\s+(-inf|-?[\d.]+) dBFS")}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# --- beats, bars, key --------------------------------------------------------


def attack(y: np.ndarray, low: bool = False) -> np.ndarray:
    """A 1 ms attack function of mono 48 kHz audio: how far (dB) each 1 ms block
    rises over the block 4 ms before it. Pre-emphasised (first difference) so
    it hears transients over sustained pads; `low` reads the kick's band
    instead (below 150 Hz). Peaks on the block an onset starts in, so a
    beat refined to it is within a millisecond of the attack (librosa's
    beats are 23 ms frames, and a spectral-flux peak sits up to half a
    window early)."""
    from scipy.signal import butter, sosfilt

    if low:
        y = sosfilt(butter(4, 150, "lowpass", fs=SR, output="sos"), y)
    else:
        y = np.diff(y, prepend=y[:1])
    hop = SR // 1000
    n = len(y) // hop
    e = (y[: n * hop].astype(np.float64).reshape(n, hop) ** 2).mean(1)
    level = 10 * np.log10(e + 1e-12)
    rise = np.zeros(n)
    rise[4:] = level[4:] - level[:-4]
    return np.maximum(rise, 0), level


@dataclass
class Grid:
    period: float  # seconds per beat
    beats_per_bar: int
    first_downbeat: float
    downbeats: list[float]  # source seconds, k = 0..K
    phase_scores: list[float]
    fit: dict
    beats: list[float] = field(default_factory=list)
    strengths: list[float] = field(default_factory=list)

    @property
    def bpm(self) -> float:
        return 60.0 / self.period


def track(y48: np.ndarray, expect_bpm: float, bpm: float | None) -> tuple[np.ndarray, float, np.ndarray]:
    """librosa's beats (seconds) on the tempo octave nearest expect_bpm, each
    refined to its attack; returns (beats, tempo, attack strength in dB)."""
    import librosa
    import soxr

    y = soxr.resample(y48, SR, AN_SR, quality="HQ")
    env = librosa.onset.onset_strength(y=y, sr=AN_SR, hop_length=AN_HOP, aggregate=np.median)
    tempo, frames = librosa.beat.beat_track(onset_envelope=env, sr=AN_SR, hop_length=AN_HOP,
                                            start_bpm=bpm or expect_bpm, bpm=bpm, units="frames")
    tempo = float(np.atleast_1d(tempo)[0])
    t = librosa.frames_to_time(frames, sr=AN_SR, hop_length=AN_HOP)
    if len(t) < 8:
        die("fewer than 8 beats tracked: is the track silent? pass --bpm and --first-downbeat")
    rise, _ = attack(y48)
    if bpm is None:
        # A half- or double-time read: fold it onto the octave near the
        # expected tempo, interpolating the missing beats or keeping the
        # stronger of the two interleaved ones.
        ratio = 2.0 ** round(math.log2(expect_bpm / tempo))
        if ratio > 1:
            say(f"tempo read {tempo:.2f} BPM: half time, doubling to {tempo * ratio:.2f}")
            n = int(ratio)
            t = np.concatenate([t[:-1, None] + np.diff(t)[:, None] * np.arange(n) / n, t[-1:, None]], axis=None)
            t = np.sort(t)
        elif ratio < 1:
            n = int(round(1 / ratio))
            say(f"tempo read {tempo:.2f} BPM: double time, halving to {tempo * ratio:.2f}")
            score = [np.mean([rise[int(x * 1000)] for x in t[p::n] if int(x * 1000) < len(rise)]) for p in range(n)]
            t = t[int(np.argmax(score))::n]
        tempo *= ratio
    # Refine each beat to the strongest attack within 40 ms; a beat with no
    # attack there (a pad, a held chord) keeps the tracker's time.
    out, strength = [], []
    for x in t:
        c = int(round(x * 1000))
        lo, hi = max(0, c - 40), min(len(rise), c + 41)
        if hi <= lo:
            continue
        j = lo + int(np.argmax(rise[lo:hi]))
        s = float(rise[j])
        out.append(j / 1000 if s >= 3 else x)
        strength.append(s)
    return np.array(out), tempo, np.array(strength)


def fit_tempo(t: np.ndarray, w: np.ndarray, period: float, fixed: bool) -> tuple[float, float, np.ndarray, np.ndarray, np.ndarray]:
    """One constant tempo through the beats: t ~ a + P * i. Returns (a, P,
    index, residual, kept). Beats more than 4 MADs (30 ms at least) off the
    line are left out of the fit, refit, three times."""
    # Index the beats by counting them, gap by gap, rather than by dividing
    # by a period: the tracker's tempo is a coarse bin (117.5 for 118.7),
    # and a 1% error slips a whole beat every 50.
    if not fixed:
        gaps = np.diff(t)
        n = np.round(gaps / period)
        ok = (n >= 1) & (np.abs(gaps / period - n) < 0.25)
        if ok.any():
            period = float(np.median(gaps[ok] / n[ok]))
    idx = np.concatenate([[0], np.cumsum(np.maximum(1, np.round(np.diff(t) / period)))])
    a, P = t[0], period
    keep = np.ones(len(t), bool)
    for it in range(4):
        if it:
            idx = np.round((t - a) / P)
        sel = keep & (w >= 3) if np.sum(keep & (w >= 3)) >= 8 else keep
        if fixed:
            a = float(np.median(t[sel] - P * idx[sel]))
        else:
            P, a = (float(v) for v in np.polyfit(idx[sel], t[sel], 1))
        res = t - (a + P * idx)
        mad = float(np.median(np.abs(res[sel] - np.median(res[sel])))) * 1.4826
        keep = np.abs(res) <= max(0.030, 4 * mad)
    idx = np.round((t - a) / P)
    return a, P, idx.astype(int), t - (a + P * idx), keep


def beat_features(y48: np.ndarray, times: np.ndarray) -> np.ndarray:
    """Per beat: how loud its attack is, how much low end it has, and how far
    the harmony moves across it (chroma before vs after). z-scored; a
    downbeat scores high on all three."""
    import librosa
    import soxr

    _, level = attack(y48)
    _, low = attack(y48, low=True)
    y = soxr.resample(y48, SR, AN_SR, quality="HQ")
    chroma = librosa.feature.chroma_cqt(y=y, sr=AN_SR, hop_length=AN_HOP)
    ct = librosa.frames_to_time(np.arange(chroma.shape[1]), sr=AN_SR, hop_length=AN_HOP)
    P = float(np.median(np.diff(times))) if len(times) > 1 else 0.5

    def peak(lv: np.ndarray, x: float) -> float:
        c = int(round(x * 1000))
        seg = lv[max(0, c - 10): c + 40]
        return float(seg.max()) if len(seg) else -120.0

    def mean_chroma(a: float, b: float) -> np.ndarray:
        m = (ct >= a) & (ct < b)
        v = chroma[:, m].mean(1) if m.any() else np.zeros(12)
        return v - v.mean()

    feats = []
    for x in times:
        before, after = mean_chroma(x - P, x), mean_chroma(x, x + P)
        den = np.linalg.norm(before) * np.linalg.norm(after)
        nov = 1 - float(before @ after / den) if den > 0 else 0.0
        feats.append((peak(level, x), peak(low, x), nov))
    f = np.array(feats)
    sd = f.std(0)
    sd[sd == 0] = 1
    return (f - f.mean(0)) / sd


def phase_scores(feats: np.ndarray, idx: np.ndarray, bpb: int) -> list[float]:
    return [float(feats[idx % bpb == p].sum(1).mean()) if np.any(idx % bpb == p) else -math.inf
            for p in range(bpb)]


def key_of(chroma_mean: np.ndarray) -> list[tuple[float, str, int, str]]:
    """(correlation, name, tonic, mode) for the 24 keys, best first."""
    out = []
    for tonic in range(12):
        for mode, prof in (("major", KK_MAJOR), ("minor", KK_MINOR)):
            r = float(np.corrcoef(chroma_mean, np.roll(prof, tonic))[0, 1])
            out.append((r, f"{NOTES[tonic]} {mode}", tonic, mode))
    return sorted(out, reverse=True)


def chord_of(v: np.ndarray) -> tuple[str, float]:
    """The major or minor triad whose template best matches a chroma vector
    (cosine, both mean-removed), named as harmony.ts names them (Fm, Db)."""
    v = v - v.mean()
    nv = np.linalg.norm(v)
    if nv == 0:
        return "N", 0.0
    best = ("N", -1.0)
    for root in range(12):
        for suffix, third in (("", 4), ("m", 3)):
            tpl = np.zeros(12)
            tpl[[root, (root + third) % 12, (root + 7) % 12]] = 1
            tpl -= tpl.mean()
            s = float(v @ tpl / (nv * np.linalg.norm(tpl)))
            if s > best[1]:
                best = (NOTES[root] + suffix, s)
    return best


def bar_chords(y48: np.ndarray, starts: list[float]) -> tuple[list[tuple[str, float]], np.ndarray]:
    """A chord guess per bar [starts[k], starts[k+1]), and the track's mean chroma."""
    import librosa
    import soxr

    y = soxr.resample(y48, SR, AN_SR, quality="HQ")
    chroma = librosa.feature.chroma_cqt(y=y, sr=AN_SR, hop_length=AN_HOP)
    ct = librosa.frames_to_time(np.arange(chroma.shape[1]), sr=AN_SR, hop_length=AN_HOP)
    rms = librosa.feature.rms(y=y, hop_length=AN_HOP)[0]
    rt = librosa.frames_to_time(np.arange(len(rms)), sr=AN_SR, hop_length=AN_HOP)
    out = []
    for a, b in zip(starts[:-1], starts[1:]):
        m = (ct >= a) & (ct < b)
        r = rms[(rt >= a) & (rt < b)]
        if not m.any() or not len(r) or db(float(np.sqrt(np.mean(r**2)))) < -50:
            out.append(("N", 0.0))
        else:
            out.append(chord_of(chroma[:, m].mean(1)))
    return out, chroma.mean(1)


def analyse(y: np.ndarray, args) -> tuple[Grid, dict]:
    mono = y.mean(1)
    dur = len(mono) / SR
    say("tracking beats")
    t, tempo, strength = track(mono, args.expect_bpm, args.bpm)
    period = 60.0 / (args.bpm or tempo)
    a, P, idx, res, keep = fit_tempo(t, strength, period, fixed=args.bpm is not None)
    strong = keep & (strength >= 3)
    r = np.abs(res[strong] if strong.any() else res[keep]) * 1000
    # The fit's drift: the median residual in 30 s windows. A steady tempo
    # stays within a few ms; a track that speeds up or slows down bows.
    drift = []
    for w0 in np.arange(0, dur, 30.0):
        m = strong & (t >= w0) & (t < w0 + 30)
        if m.sum() >= 4:
            drift.append((float(w0), float(np.median(res[m]) * 1000)))
    fit = {"tracked_bpm": tempo, "beats": int(len(t)), "fitted": int(keep.sum()),
           "with_attack": int(strong.sum()), "median_abs_ms": float(np.median(r)),
           "p95_abs_ms": float(np.percentile(r, 95)), "max_abs_ms": float(r.max()), "drift_ms": drift}

    bpb = args.beats_per_bar
    say("finding the downbeat")
    feats = beat_features(mono, t)
    scores = phase_scores(feats, idx, bpb)
    if args.first_downbeat is not None:
        d0 = float(args.first_downbeat)
        phase = int(round((d0 - a) / P)) % bpb
    else:
        phase = int(np.argmax(scores))
        i = math.ceil((-0.005 - a) / P)
        i += (phase - i) % bpb
        d0 = max(0.0, a + P * i)
    bar = bpb * P
    if args.map == "detected":
        downbeats, used = [], 0
        k = 0
        while d0 + k * bar <= dur - 1e-3:
            g = d0 + k * bar
            j = int(np.argmin(np.abs(t - g)))
            if abs(t[j] - g) <= 0.040 and strength[j] >= 3 and (not downbeats or t[j] > downbeats[-1]):
                downbeats.append(float(t[j]))
                used += 1
            else:
                downbeats.append(g)
            k += 1
        fit["detected_downbeats"] = used
    else:
        downbeats = [d0 + k * bar for k in range(int((dur - 1e-3 - d0) // bar) + 1)]
    grid = Grid(P, bpb, d0, downbeats, scores, fit, t.tolist(), strength.tolist())

    say("guessing the key and the chords")
    chords, cmean = bar_chords(mono, downbeats)
    keys = key_of(cmean)
    report = {"duration": dur, "keys": keys[:3], "chords": chords}
    return grid, report


def semitones_to(keys: list, target: str) -> int:
    """The shift (-6..+5) that takes the detected key to the target's tonic,
    or for a major-key track to the target minor's relative major."""
    name, mode = target.split()
    tonic = NOTES.index(name)
    _, _, t, m = keys[0]
    goal = tonic if m == mode else (tonic + (3 if mode == "minor" else -3)) % 12
    return (goal - t + 6) % 12 - 6


def print_analysis(src: Path, rate: int, grid: Grid, rep: dict, target: str, semitones: int) -> None:
    f = grid.fit
    print(f"\nSource: {src} ({rate} Hz, {rep['duration']:.3f} s)")
    print(f"Tempo: tracked {f['tracked_bpm']:.3f} BPM; fitted {grid.bpm:.4f} BPM "
          f"(beat {grid.period * 1000:.3f} ms, bar {grid.beats_per_bar * grid.period:.5f} s) "
          f"-> stretch {BAR_S / (grid.beats_per_bar * grid.period):.5f}x to {TRACK_BPM:g} BPM")
    print(f"Fit: {f['fitted']} of {f['beats']} beats ({f['with_attack']} with a clear attack); "
          f"|residual| median {f['median_abs_ms']:.1f} ms, p95 {f['p95_abs_ms']:.1f} ms, max {f['max_abs_ms']:.1f} ms")
    if f["drift_ms"]:
        print("Drift (median residual per 30 s): " + "  ".join(f"{clock(w)[:-4]} {d:+.1f}" for w, d in f["drift_ms"]))
    if f["p95_abs_ms"] > 20:
        print("  warning: the tempo is not steady (p95 > 20 ms); try --map detected, or check --bpm")
    names = ["1", "2", "3", "4", "5", "6", "7"]
    print("Downbeat phase scores: " + "  ".join(f"beat {names[p]}={s:+.2f}" for p, s in enumerate(grid.phase_scores)))
    srt = sorted(grid.phase_scores, reverse=True)
    if len(srt) > 1 and srt[0] - srt[1] < 0.3:
        print("  warning: the downbeat is a close call; listen, and pass --first-downbeat if bar 1 is wrong")
    print(f"First downbeat: {grid.first_downbeat:.3f} s (source bar 0); "
          f"{len(grid.downbeats) - 1} whole bars to {grid.downbeats[-1]:.3f} s, then a {rep['duration'] - grid.downbeats[-1]:.3f} s tail")
    if "detected_downbeats" in f:
        print(f"Time map: detected downbeats for {f['detected_downbeats']} of {len(grid.downbeats)} bars, fitted for the rest")
    k = rep["keys"]
    print("Key: " + ", ".join(f"{n} ({r:.2f})" for r, n, _, _ in k) + f"; the reel's is {target}")
    sug = semitones_to(k, target)
    print(f"  shift to match: {sug:+d} semitones (--semitones {sug}, or --semitones auto)"
          + (f"; this run shifts {semitones:+d}" if semitones else ""))
    print("\nChords per source bar (a guess from chroma; N = too quiet to say):")
    ch = rep["chords"]
    for row in range(0, len(ch), 8):
        cells = "  ".join(f"{c:<4}" for c, _ in ch[row: row + 8])
        print(f"  bar {row:>3} {clock(grid.downbeats[row])[:-2]:>8}  {cells}")


# --- stretch -----------------------------------------------------------------


def stretch(y: np.ndarray, grid: Grid, semitones: int, work: Path) -> np.ndarray:
    """Rubber Band R3 through a time map: source downbeat k -> frame k * 96000.
    The source is trimmed to start on downbeat 0 first, so 0 -> 0 is where
    the file starts. Rubber Band does not resample, so the map's frames are
    48 kHz frames on both sides.

    The map leaves out the "0 0" line: given one, Rubber Band 4.0.0's R3
    engine plays the whole first segment at ratio 1 (bar 0 came out the
    source's 2.0219 s long, and every later bar line about 21 ms late, on a
    118.7 BPM click track), and without it bar 0 is stretched like the
    rest (every bar line within a millisecond)."""
    import soundfile as sf

    s0 = int(round(grid.downbeats[0] * SR))
    pairs = [(int(round((d - grid.downbeats[0]) * SR)), k * BAR_FRAMES)
             for k, d in enumerate(grid.downbeats) if k]
    # A track already on the grid (Suno renders on an exact 120 BPM clock,
    # so --bpm 120 and the first downbeat put every bar line on its frame)
    # needs no stretch: R3 at ratio 1 still resynthesises every sample, so
    # it is skipped and the trimmed track plays as it came.
    if not semitones and all(abs(a - b) <= 1 for a, b in pairs):
        say(f"every bar line is already on the grid: trimming {s0} frames, no stretch")
        return y[s0:].astype(np.float32)
    src = work / "trimmed.wav"
    sf.write(src, y[s0:], SR, subtype="FLOAT")
    n_src = len(y) - s0
    mp = work / "map.txt"
    lines = [f"{a} {b}" for a, b in pairs]
    mp.write_text("\n".join(lines) + "\n")
    K = len(grid.downbeats) - 1
    tail = n_src / SR - (grid.downbeats[-1] - grid.downbeats[0])
    duration = K * BAR_S + tail * BAR_S / (grid.beats_per_bar * grid.period)
    out = work / "stretched.wav"
    cmd = ["rubberband", "-q", "-3", "--centre-focus", "--ignore-clipping", "-M", str(mp), "-D", f"{duration:.6f}"]
    if semitones:
        cmd += ["-p", str(semitones)]
    say(f"stretching {K} bars with Rubber Band R3 ({' '.join(cmd[1:])})")
    run(cmd + [str(src), str(out)])
    z, rate = sf.read(out, dtype="float32", always_2d=True)
    if rate != SR:
        die(f"rubberband wrote {rate} Hz")
    return z


# --- arrange -----------------------------------------------------------------


@dataclass
class Segment:
    src_from: int
    src_to: int
    at: int
    gain_db: float = 0.0
    fade_in_ms: float | None = None
    fade_out_ms: float | None = None
    ring: bool = False

    @property
    def bars(self) -> int:
        return self.src_to - self.src_from

    @property
    def end(self) -> int:
        return self.at + self.bars


def load_spec(path: Path, src_bars: int, reel: Reel) -> tuple[dict, list[Segment]]:
    spec = tomllib.loads(path.read_text())
    segs = []
    for i, s in enumerate(spec.get("segment", [])):
        to = s["src_bar_to"]
        to = src_bars if to == "end" else int(to)
        seg = Segment(int(s["src_bar_from"]), to, int(s["reel_bar_at"]), float(s.get("gain_db", 0)),
                      s.get("fade_in_ms"), s.get("fade_out_ms"), bool(s.get("ring", False)))
        where = f"{path.name} segment {i + 1}"
        if not 0 <= seg.src_from < seg.src_to <= src_bars:
            die(f"{where}: source bars [{seg.src_from}, {seg.src_to}) are not within the track's 0..{src_bars}")
        if not 0 <= seg.at < reel.bars:
            die(f"{where}: reel_bar_at {seg.at} is not within the reel's 0..{reel.bars - 1}")
        if seg.end > reel.bars:
            die(f"{where}: ends at reel bar {seg.end}, past the reel's {reel.bars}; "
                f"end it at {reel.bars} and set ring = true to let it ring out")
        segs.append(seg)
    if not segs:
        die(f"{path}: no [[segment]]")
    last = max(segs, key=lambda s: s.end)
    if any(s.ring for s in segs if s is not last):
        die(f"{path}: only the segment that ends last may ring")
    return spec, segs


def ramp(n: int, up: bool) -> np.ndarray:
    """An equal-power fade: sin up, cos down, so a crossfade of uncorrelated
    material keeps its power through the join."""
    x = (np.arange(n) + 0.5) / max(n, 1)
    return np.sin(x * np.pi / 2) if up else np.cos(x * np.pi / 2)


def arrange(z: np.ndarray, segs: list[Segment], spec: dict, reel: Reel) -> tuple[np.ndarray, list[str]]:
    """The stretched bars laid on reel time. At a join where one segment ends
    on the bar line the next starts on, they crossfade over crossfade_ms
    ending ON the bar line: the outgoing one fades out over its own last
    milliseconds, the incoming one fades in over the source just before its
    first bar, so its downbeat lands at full level. fade_in_ms and
    fade_out_ms replace that with a longer fade inside the segment, and an
    edge with no neighbour gets a 3 ms declick. A segment that carries
    straight on from the one before (it starts on the source bar that one
    ends on, at the same gain, with no fade between) joins it with a plain
    cut, which is seamless: an equal-power crossfade of the same audio
    would bump it by 3 dB. Overlapping segments sum."""
    xf = int(round(float(spec.get("crossfade_ms", 30)) * SR / 1000))
    dc = int(round(0.003 * SR))
    out = np.zeros((reel.frames, 2), np.float64)
    notes = []
    # Where a ringing segment's audio runs out, in reel frames: past its last
    # bar it plays only as far as the source goes.
    rang_to = 0

    def carries_on(a: Segment, b: Segment) -> bool:
        """True when b plays on from where a stops, on the same bar line."""
        return (a is not b and a.end == b.at and a.src_to == b.src_from and a.gain_db == b.gain_db
                and not a.ring and a.fade_out_ms is None and b.fade_in_ms is None)

    for seg in segs:
        S, L, s = seg.at * BAR_FRAMES, seg.bars * BAR_FRAMES, seg.src_from * BAR_FRAMES
        E = S + L
        prev = any(o is not seg and o.end == seg.at for o in segs)
        nxt = any(o is not seg and o.at == seg.end for o in segs)
        # A plain cut on either side: no fade there, and nothing written past it.
        cut_in = any(carries_on(o, seg) for o in segs)
        cut_out = any(carries_on(seg, o) for o in segs)
        # The span this segment writes: [S - pre, E + post) of reel time,
        # from [s - pre, s + L + post) of the stretched source. The pre-roll
        # needs source before the segment's first bar and reel before its
        # first bar line: a segment at reel bar 0 has none, and declicks
        # inside instead.
        pre = 0
        if seg.fade_in_ms is None and not cut_in:
            pre = min(xf if prev else dc, s, S)
        if seg.ring:
            post = max(0, min(reel.frames - E, len(z) - (s + L)))
        elif seg.fade_out_ms is None and not nxt:
            post = min(dc, max(0, len(z) - (s + L)), reel.frames - E)
        else:
            post = 0
        if seg.ring:
            rang_to = E + post
        chunk = z[s - pre: s + L + post].astype(np.float64)
        g = np.ones(len(chunk))
        if seg.fade_in_ms is not None:
            n = min(L, int(round(seg.fade_in_ms * SR / 1000)))
            g[:n] *= ramp(n, True)
        elif pre:
            g[:pre] *= ramp(pre, True)
        elif not cut_in:
            # Bar 0 of the source has nothing before it: declick inside.
            g[:dc] *= ramp(dc, True)
        if seg.fade_out_ms is not None:
            n = min(L, int(round(seg.fade_out_ms * SR / 1000)))
            g[pre + L - n: pre + L] *= ramp(n, False)
            g[pre + L:] = 0
        elif not seg.ring and not cut_out:
            n = xf if nxt else post
            if n:
                g[len(g) - n:] *= ramp(n, False)
        chunk *= (g * 10 ** (seg.gain_db / 20))[:, None]
        out[S - pre: S - pre + len(chunk)] += chunk
        a, b = reel.at(seg.at * BAR_S), reel.at(seg.end * BAR_S - 1e-6)
        notes.append(f"  src bars {seg.src_from:>3}-{seg.src_to:<3} -> reel bars {seg.at:>3}-{seg.end:<3} "
                     f"{clock(seg.at * BAR_S)[:-2]:>8} - {clock(seg.end * BAR_S)[:-2]:<8} "
                     f"{a.id}..{b.id}" + (f"  {seg.gain_db:+g} dB" if seg.gain_db else "")
                     + ("  rings" if seg.ring else ""))
    # Coverage: bars nothing plays, and bars two segments play at once.
    count = np.zeros(reel.bars, int)
    for seg in segs:
        count[seg.at: seg.end] += 1
    gaps = [i for i in range(reel.bars) if count[i] == 0]
    over = [i for i in range(reel.bars) if count[i] > 1]
    ringing = [s for s in segs if s.ring]
    if ringing:
        # Bars after the ringing segment are covered only as far as its
        # audio reaches: a source shorter than the reel leaves the rest
        # silent, and that is a gap too.
        gaps = [i for i in gaps if i < ringing[0].end or i * BAR_FRAMES >= rang_to]
    if gaps:
        notes.append(f"  warning: silent reel bars {spans(gaps)}")
    if over:
        notes.append(f"  note: overlapping reel bars {spans(over)} (they sum)")
    return out, notes


def spans(bars: list[int]) -> str:
    out, a = [], None
    for i, b in enumerate(bars):
        if a is None:
            a = b
        if i + 1 == len(bars) or bars[i + 1] != b + 1:
            out.append(f"{a}" if a == b else f"{a}-{b}")
            a = None
    return ", ".join(out)


def tail_rule(x: np.ndarray, spec: dict) -> str | None:
    """The bed must be silent by the reel's last frame. If the last 20 ms
    still sound above silence_db, fade over tail_fade_ms ending there."""
    floor = float(spec.get("silence_db", -60))
    last = x[-int(0.020 * SR):]
    lvl = db(float(np.abs(last).max()))
    if lvl <= floor:
        return None
    n = int(round(float(spec.get("tail_fade_ms", 2000)) * SR / 1000))
    x[-n:] *= ramp(n, False)[:, None] ** 2
    return (f"warning: the bed still sounded at the reel's end ({lvl:.1f} dBFS in the last 20 ms); "
            f"faded it over the last {n / SR:g} s. Arrange an ending that rings out by {clock(len(x) / SR)}")


# --- verify ------------------------------------------------------------------


def verify(path: Path, reel: Reel, args, work: Path) -> bool:
    import soundfile as sf

    ok = True
    print(f"\nVerify: {path}")
    info = probe(path)
    print(f"  stream: {info['codec_name']}, {info['sample_rate']} Hz, {info['channels']} ch")
    if info["codec_name"] != "opus" or int(info["sample_rate"]) != SR or int(info["channels"]) != 2:
        print("  FAIL: the bed must be Opus, 48 kHz, stereo")
        ok = False
    dec = work / "verify.wav"
    run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(path), "-c:a", "pcm_f32le", str(dec)])
    x, rate = sf.read(dec, dtype="float32", always_2d=True)
    print(f"  decoded: {len(x)} frames = {len(x) / rate:.6f} s (the reel: {reel.frames} = {reel.seconds:g} s)")
    if rate != SR or len(x) != reel.frames:
        print("  FAIL: the decoded length is not the reel's, frame for frame")
        ok = False
    peak = db(float(np.abs(x).max()))
    tail = db(float(np.abs(x[-int(0.020 * SR):]).max()))
    lo = loudness(dec)
    print(f"  loudness: {lo['lufs']:.1f} LUFS integrated, LRA {lo['lra']:.1f} LU, "
          f"sample peak {peak:.2f} dBFS, true peak {lo['true_peak']:.2f} dBTP")
    print(f"  last 20 ms: {tail:.1f} dBFS")
    if tail > args.silence_db:
        print(f"  FAIL: not silent at the reel's end (> {args.silence_db:g} dBFS)")
        ok = False

    mono = x.mean(1).astype(np.float32)
    # The tracker's tempo is a report only: the grid check below reads each
    # bar line's own onset. A sparse bed, with too few beats for the tracker
    # (which exits), is checked by ear with --no-grid-check, so it skips it.
    if args.no_grid_check:
        print("  tempo: not tracked (--no-grid-check)")
    else:
        t, tempo, _ = track(mono, TRACK_BPM, None)
        print(f"  tempo: tracked {tempo:.3f} BPM over {len(t)} beats")
    # Each bar line's own onset: the strongest attack within 40 ms of the
    # grid line itself, if it is a clear one (a rise of CLEAR_DB). Starting
    # from the tracker's beat instead (a 23 ms frame, refined within 40 ms
    # of where it fell) caught a clap or a stab 50 ms after a downbeat that
    # was on its frame, in an unstretched copy of an exact 120 BPM track;
    # and a weaker rise is a pad's or an arpeggio's, which falls anywhere.
    # A fill or a pickup still lands near some bar lines, so the limits are
    # on the median and the 90th percentile, which a stretch that drifts or
    # a misread downbeat moves, rather than on the single worst bar.
    rise, _ = attack(mono)
    dev = []
    for k in range(reel.bars):
        c = int(round(k * BAR_S * 1000))
        lo, hi = max(0, c - 40), min(len(rise), c + 41)
        j = lo + int(np.argmax(rise[lo:hi])) if hi > lo else c
        dev.append(float(j - c) if hi > lo and rise[j] >= CLEAR_DB else None)
    have = np.array([d for d in dev if d is not None])
    print(f"\n  Bar lines vs the {BAR_S:g} s grid (ms; - = no clear onset within 40 ms):")
    for row in range(0, reel.bars, 10):
        cells = " ".join(f"{d:+5.1f}" if d is not None else "   - " for d in dev[row: row + 10])
        print(f"  {row:>4} {clock(row * BAR_S)[:-4]:>6}  {cells}")
    if len(have) < 8:
        print(f"  {'note' if args.no_grid_check else 'FAIL'}: only {len(have)} bar lines have a clear onset; "
              "too few to check the grid"
              + (" (--no-grid-check: check the bars by ear)" if args.no_grid_check
                 else " (a bed with no attacks: check the bars by ear, or pass --no-grid-check)"))
        ok = ok and args.no_grid_check
    else:
        med = float(np.median(np.abs(have)))
        p90 = float(np.percentile(np.abs(have), 90))
        worst = sorted(((abs(d), k, d) for k, d in enumerate(dev) if d is not None), reverse=True)[:5]
        print(f"  {len(have)} of {reel.bars} bar lines checked: |deviation| median {med:.2f} ms "
              f"(limit {args.max_median_ms:g}), 90th percentile {p90:.2f} ms (limit {args.max_p90_ms:g}); worst: "
              + ", ".join(f"bar {k} {d:+.1f}" for _, k, d in worst))
        if med > args.max_median_ms or p90 > args.max_p90_ms:
            print(f"  {'note' if args.no_grid_check else 'FAIL'}: the bar lines are off the grid")
            ok = ok and args.no_grid_check
    # The phase: on a 0.5 s beat grid, bar lines every fourth beat. If the
    # source's downbeat was misread, the accents fall on another beat.
    grid_t = np.arange(0, reel.seconds - 0.25, 0.5)
    feats = beat_features(mono, grid_t)
    sc = phase_scores(feats, np.arange(len(grid_t)), 4)
    best = int(np.argmax(sc))
    print("  downbeat phase on the output: " + "  ".join(f"beat {p + 1}={s:+.2f}" for p, s in enumerate(sc))
          + ("" if best == 0 else f"\n  warning: the accents read strongest on beat {best + 1}, not on the bar line; "
             "if bar 1 sounds late or early, rerun with --first-downbeat"))

    chords, _ = bar_chords(mono, [k * BAR_S for k in range(reel.bars + 1)])
    chart = []
    print("\n  Chords per reel section, as harmony.ts Changes (section beat at 60 BPM, chord):")
    for s in reel.sections:
        a, b = int(round(s.start / BAR_S)), int(round(s.end / BAR_S))
        changes: list[tuple[int, str]] = []
        for k in range(a, b):
            c = chords[k][0]
            if not changes or changes[-1][1] != c:
                changes.append((int(round(k * BAR_S - s.start)), c))
        chart.append({"section": s.id, "changes": changes})
        print(f"  {s.id:<10} " + " ".join(f"[{bt}, {c}]" for bt, c in changes))
    (work / "chords.json").write_text(json.dumps({"bars": [c for c, _ in chords], "sections": chart}, indent=2) + "\n")

    digest = sha256(path)
    print(f"\n  sha256: {digest}")
    print(f"  size: {path.stat().st_size} bytes")
    # Only the hash: gainDb and duck in BED are tuned in the render, and
    # pasting a whole object here would reset them.
    print("\n  For score/bed.ts, BED's sha256 line:\n")
    print(f'    sha256: "{digest}",')
    print(f"\n  {'PASS' if ok else 'FAIL'}")
    return ok


# --- commands ----------------------------------------------------------------


def workdir(args, src: Path | None) -> Path:
    w = Path(args.work) if args.work else Path(tempfile.gettempdir()) / "showreel-bed" / (src.stem if src else "verify")
    w.mkdir(parents=True, exist_ok=True)
    return w


def cmd_analyse(args, build: bool) -> int:
    reel = load_reel()
    src = Path(args.src).resolve()
    if not src.is_file():
        die(f"{src} is not a file")
    need("ffmpeg", "brew install ffmpeg / apt install ffmpeg")
    if build:
        need("rubberband", "brew install rubberband / apt install rubberband-cli")
    work = workdir(args, src)
    say(f"work directory {work}")
    y, rate = decode48(src, work / "source48.wav")
    grid, rep = analyse(y, args)
    if args.semitones == "auto":
        semitones = semitones_to(rep["keys"], args.key)
    else:
        semitones = int(args.semitones)
    print_analysis(src, rate, grid, rep, args.key, semitones)
    (work / "analysis.json").write_text(json.dumps({
        "source": str(src), "rate": rate, "duration": rep["duration"], "bpm": grid.bpm,
        "period": grid.period, "beats_per_bar": grid.beats_per_bar,
        "first_downbeat": grid.first_downbeat, "phase_scores": grid.phase_scores, "fit": grid.fit,
        "keys": [{"key": n, "r": r} for r, n, _, _ in rep["keys"]],
        "bars": [{"bar": k, "at": d, "chord": c, "score": s}
                 for k, (d, (c, s)) in enumerate(zip(grid.downbeats, rep["chords"]))],
        "downbeats": grid.downbeats, "beats": grid.beats, "semitones": semitones,
    }, indent=1) + "\n")
    if not build:
        print()
        print_sections(reel)
        return 0

    import soundfile as sf

    z = stretch(y, grid, semitones, work)
    src_bars = len(grid.downbeats) - 1
    spec_path = Path(args.spec).resolve()
    spec, segs = load_spec(spec_path, src_bars, reel)
    print(f"\nArrangement ({spec_path}, {src_bars} source bars):")
    x, notes = arrange(z, segs, spec, reel)
    print("\n".join(notes))
    warn = tail_rule(x, spec)
    if warn:
        print("  " + warn)
    sf.write(work / "arranged.wav", x.astype(np.float32), SR, subtype="FLOAT")

    lo = loudness(work / "arranged.wav")
    if lo["lufs"] == -math.inf:
        die("the arrangement is silent")
    peak = db(float(np.abs(x).max()))
    gain = args.lufs - lo["lufs"]
    limited = peak + gain > args.peak
    if limited:
        gain = args.peak - peak
    x *= 10 ** (gain / 20)
    sf.write(work / "bed.wav", x.astype(np.float32), SR, subtype="FLOAT")
    print(f"\nLoudness: {lo['lufs']:.1f} LUFS, sample peak {peak:.2f} dBFS before; gain {gain:+.2f} dB -> "
          f"{lo['lufs'] + gain:.1f} LUFS, peak {peak + gain:.2f} dBFS"
          + (f" (held to the {args.peak:g} dBFS peak; {args.lufs - lo['lufs'] - gain:.1f} LU under the target)"
             if limited else ""))

    out = Path(args.out).resolve() if args.out else work / "bed.opus"
    out.parent.mkdir(parents=True, exist_ok=True)
    say(f"encoding {out}")
    # bitexact: no encoder version in the tags and a fixed Ogg serial, so the
    # same bed.wav always encodes to the same bytes (and sha256).
    run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(work / "bed.wav"), "-map_metadata", "-1",
         "-c:a", "libopus", "-b:a", args.bitrate, "-vbr", "on", "-application", "audio",
         "-compression_level", "10", "-ar", str(SR), "-ac", "2",
         "-fflags", "+bitexact", "-flags:a", "+bitexact", str(out)])
    ok = verify(out, reel, args, work)
    if out != (ROOT / BED_FILE).resolve():
        print(f"\n  To use it: cp {out} {ROOT / BED_FILE}")
    print(f"  Listen: {work / 'bed.wav'} (pre-encode), {out}")
    return 0 if ok else 1


def cmd_verify(args) -> int:
    reel = load_reel()
    need("ffmpeg", "brew install ffmpeg / apt install ffmpeg")
    path = Path(args.file).resolve()
    return 0 if verify(path, reel, args, workdir(args, None)) else 1


def main() -> int:
    ap = argparse.ArgumentParser(prog="showreel-bed", description=__doc__.split("\n\n")[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("sections", help="print the reel's sections in bed bars")

    def common(p, src=True):
        if src:
            p.add_argument("src", help="the downloaded track (WAV, or anything ffmpeg reads)")
        p.add_argument("--work", help="where intermediate files go (default: $TMPDIR/showreel-bed/<name>)")
        p.add_argument("--max-median-ms", type=float, default=5.0, help="verify: median |bar line deviation| limit")
        p.add_argument("--max-p90-ms", type=float, default=15.0,
                       help="verify: 90th percentile |bar line deviation| limit")
        p.add_argument("--silence-db", type=float, default=-60.0, help="verify: the last 20 ms must peak below this")
        p.add_argument("--no-grid-check", action="store_true", help="verify: report the bar lines but do not fail on them")

    for name, helptext in (("analyse", "print the tempo, downbeat, key and chords"),
                           ("build", "analyse, stretch, arrange, normalise, encode and verify")):
        p = sub.add_parser(name, help=helptext)
        common(p)
        p.add_argument("--bpm", type=float, help="the track's tempo, overriding the fit")
        p.add_argument("--first-downbeat", type=float, help="seconds into the track of bar 1's downbeat")
        p.add_argument("--beats-per-bar", type=int, default=4, help="default 4")
        p.add_argument("--expect-bpm", type=float, default=TRACK_BPM,
                       help="fold the tracked tempo onto the octave nearest this (default 120)")
        p.add_argument("--map", choices=("fitted", "detected"), default="fitted",
                       help="time map from the constant-tempo fit (default) or each detected downbeat")
        p.add_argument("--semitones", default="0", help="pitch shift: an integer, or auto (to --key)")
        p.add_argument("--key", default="F minor", help="the reel's key, for the suggested shift (default F minor)")
        if name == "build":
            p.add_argument("--spec", default=str(DEFAULT_SPEC), help="the arrangement (default bed.toml here)")
            p.add_argument("--out", help=f"the Opus file (default <work>/bed.opus; the reel's is {BED_FILE})")
            p.add_argument("--lufs", type=float, default=-32.0,
                           help="integrated loudness target (default -32: the master chain lifts the bed "
                                "about 11 dB; \"mise screenreel\" at -32 put the reel at -17.0 LUFS "
                                "at BED.gainDb 0.7)")
            p.add_argument("--peak", type=float, default=-3.0, help="sample peak ceiling, dBFS (default -3)")
            p.add_argument("--bitrate", default="192k", help="Opus bitrate (default 192k)")
    p = sub.add_parser("verify", help="check an encoded bed against the grid")
    p.add_argument("file")
    common(p, src=False)

    args = ap.parse_args()
    if args.cmd == "sections":
        print_sections(load_reel())
        return 0
    if args.cmd == "verify":
        return cmd_verify(args)
    if args.semitones != "auto":
        try:
            int(args.semitones)
        except ValueError:
            die("--semitones takes an integer or auto", 2)
    return cmd_analyse(args, build=args.cmd == "build")


if __name__ == "__main__":
    sys.exit(main())
