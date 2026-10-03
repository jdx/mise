# The showreel's bed

The showreel's music is one instrumental track, the bed, which plays under the
sound effects from the first frame to the last. The page plays
`docs/.vitepress/theme/showreel/score/bed.opus` on its music bus, which ducks
under the sound effects that ask for it and follows each section's level, so the file has to be
arranged on reel time already:

- Ogg Opus, 48 kHz, stereo
- decoded sample 0 is reel time 0
- exactly 436.000 s (20,928,000 frames): the whole reel, end card included,
  with the bed's own ending
- 120 BPM 4/4, every bar line on the reel's grid: a bar is 2.000 s (96,000
  frames), two of the reel's 60 BPM beats

`score/bed.ts` pins the file by its sha256, so the bed changes only in a commit
that changes both.

`mise run docs:showreel-bed` (`xtasks/docs/showreel-bed`, which runs `bed.py`
here) turns a downloaded track into that file. It is not part of the render;
the render cache key covers `bed.opus` itself. The committed bed is "mise
screenreel", jdx's Suno track, arranged by `bed.toml`; `score/BED.md` records
how it was built.

## Needs

- ffmpeg with libopus
- the Rubber Band CLI: `brew install rubberband` on macOS, or
  `apt install rubberband-cli` on Debian and Ubuntu. `build` checks for it
  even when the track is already on the grid and it is not run
- uv, which runs `bed.py` with its dependencies (librosa, numpy, scipy,
  soundfile, soxr) from its inline metadata. If no uv is on PATH, the task
  runs one through `mise exec`

## Workflow

1. **Generate and download.** Ask Suno for an instrumental at 120 BPM in 4/4,
   F minor if you can get it (the sound effects' chord-aware stingers are in F
   minor), six or seven minutes long (the arrangement repeats or cuts bars to
   fill 7:16), calm, with an ending that rings out. Download the WAV, not the
   MP3, and record its sha256 before moving it (`score/BED.md`'s provenance
   table).

2. **Look at the reel and the track.**

   ```sh
   mise run docs:showreel-bed -- sections
   mise run docs:showreel-bed -- analyse ~/Downloads/bed.wav
   ```

   `sections` lists every section's start and length in bed bars. `analyse`
   prints the track's tempo (tracked, then fitted to one constant tempo, with
   the residuals and the drift every 30 s), where bar 0's downbeat is, the key
   and the semitone shift to F minor, and a chord guess for every source bar.
   If the downbeat is wrong (the phase scores are a close call, or bar 1 sounds
   off), pass `--first-downbeat SECONDS`. If the tempo read is wrong, pass
   `--bpm`. If the track drifts (the p95 residual is over 20 ms), try
   `--map detected`, which pins each bar to its own detected downbeat instead of
   the constant-tempo fit.

   Suno appears to render on an exact tempo clock: the beats of "mise
   screenreel" sit within a few milliseconds of an exact 120 BPM grid (median 1
   ms, p95 3 ms). The unpinned fit misreads such a clock: a plain `analyse` of
   "mise screenreel" read 119.72 BPM, a p95 residual of 202 ms and a first
   downbeat at 1.434 s, and warned to try `--map detected`. For a Suno track,
   run `analyse --bpm 120` instead (it read the first downbeat at 0.105 s, p95
   3 ms), and the fix for that warning is `--bpm 120`, not `--map detected`.
   Then pass `--bpm 120` and the first downbeat it found
   (`--first-downbeat 0.105` there) to `build`: every bar line is
   then on its frame already, and the build trims the track to its first
   downbeat instead of stretching it. A fitted tempo a ten-thousandth of a BPM
   off would put the last bar lines more than a frame out, and the whole track
   would be stretched for nothing.

3. **Arrange.** A spec maps source bars to reel bars. `bed.toml`, the default
   `--spec`, is the arrangement of one particular track, the one its comments
   name ("mise screenreel"): its bar numbers mean nothing for another. For a
   new track, write a spec of its own, starting from the generic block at the
   end of `bed.toml` (the track as it comes, from its first downbeat, ringing
   out at its end), and loop or cut bars, using `sections` and the chord list
   to put the changes and the ending where they belong (the end card is reel
   bars 211 to 218). The file's comments describe every field.

4. **Build.**

   ```sh
   mise run docs:showreel-bed -- build ~/Downloads/bed.wav --spec my-bed.toml --semitones auto
   ```

   This stretches the track onto the grid (Rubber Band R3, with a time map from
   source downbeat k to frame k × 96000, plus a pitch shift if asked for),
   unless every bar line is already within a frame of its place and no pitch
   shift is asked for: then it only trims the track to its first downbeat
   (R3 at ratio 1 would still resynthesise every sample). For a track on the
   grid:

   ```sh
   mise run docs:showreel-bed -- build ~/Downloads/bed.wav --bpm 120 --first-downbeat 0.105
   ```

   Then it arranges the track, sets one gain to `--lufs` with the sample peak at
   or below `--peak` (default -3 dBFS), and encodes Opus at 192 kb/s. The
   default, -32 LUFS integrated, is staged against the master chain in
   `audio.ts`, whose makeup lifts the bed about 11 dB: "mise screenreel" at -32
   LUFS and `BED.gainDb` 0.7 put the whole reel at -17.0 LUFS, the target, where
   a bed at -20 LUFS put it at -11.6 with the limiter clamped throughout. So
   `bed.wav` is quiet to listen to on its own; trim the level in the render with
   `BED.gainDb`, not here. Then it verifies the file it wrote. It decodes it,
   checks the length frame for frame, and tracks the beats again. It measures
   each bar line by its own onset: the strongest attack within 40 ms of the grid
   line, counted only if it is a clear one, a rise of at least 20 dB over 4 ms
   (a kick or a click; a pad's or an arpeggio's swell falls anywhere). A fill or
   a pickup still lands near some bar lines, so it fails on the median and the
   90th percentile of the deviations, not the worst bar: over 5 ms median or 15
   ms at the 90th percentile (`--max-median-ms`, `--max-p90-ms`), or fewer than
   8 bar lines with a clear onset. It checks that the bed is silent by 436.0 s,
   and prints a chord chart per section as `harmony.ts` changes. Every
   intermediate file stays in the work directory it prints: `stretched.wav`
   (only when it stretched), `arranged.wav`, `bed.wav`, `analysis.json`,
   `chords.json`.

5. **Listen.** Listen to `bed.wav` in the work directory, especially the joins
   and the ending. Then hear it under the sound effects in a draft render. The
   renderer checks the file against `BED.sha256`, so paste the hash the build
   printed into `score/bed.ts` first:

   ```sh
   cp <work>/bed.opus docs/.vitepress/theme/showreel/score/bed.opus
   aube run showreel:video --edition source --audio-only /tmp/reel.wav --from 400 --until 436
   ```

   Change the spec or the overrides and build again until it sits right.

6. **Commit once.** Commit `score/bed.opus` and `score/bed.ts`, with its
   `sha256` line, together. Do not commit the WAV. With the same tool versions,
   the same WAV, spec and options give the same bytes, so a rebuild reproduces
   the pinned hash; record the versions in `score/BED.md`'s provenance table.
   The Python packages are pinned by date (`exclude-newer` in `bed.py`'s
   inline metadata), but ffmpeg with libopus and Rubber Band come from PATH,
   and a different version of either can change the bytes (Rubber Band's
   only when the track was stretched). To check a
   committed file again, run:

   ```sh
   mise run docs:showreel-bed -- verify docs/.vitepress/theme/showreel/score/bed.opus
   ```

## Notes

- **Resampling.** Homebrew's ffmpeg has no soxr resampler, so ffmpeg only
  decodes. Resampling to 48 kHz uses soxr's Python binding at its very high
  quality setting.
- **The time map leaves out `0 0`.** With that line in the map, Rubber Band
  4.0.0's R3 engine plays the whole first bar at ratio 1. On a 118.7 BPM click
  track, every later bar line came out about 21 ms late. Without the line, every
  bar line came out within a millisecond.
- **Joins.** Where one segment ends on the bar line where the next starts, they
  crossfade with equal power over `crossfade_ms`, ending on the bar line. The
  incoming segment fades in over the source audio just before its first bar, so
  its downbeat lands at full level.
- **Tested on a synthetic track.** The input was a 300 s, 44.1 kHz click track
  at 118.7 BPM: first downbeat at 0.37 s, an accented click on each bar, a
  plain click on each beat, and a quiet Fm–D♭–E♭–C pad. Its 148 bars were
  looped to fill 218. The fit
  found 118.7000 BPM and a downbeat at 0.371 s. The verify pass measured every
  bar line within 3 ms of the grid (median 1 ms). Measured directly from the
  waveform, the clicks were within 2.3 ms.
