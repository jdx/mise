# The bed

`bed.opus` is the showreel's music: one instrumental track that plays under the synthesized sound effects from reel time 0 to 7:16.0, end card included. `bed.ts` pins it: its path, sha256, gain and duck depth. This file records what the bed is, where it came from, who owns it, and how a new one is fitted.

jdx decided on 2026-10-02 to replace the synthesized band with a recorded bed made in Suno, to keep every synthesized sound effect, and to remove the end card's sung line (STORYBOARD.md "Decisions and references").

## The track

`bed.opus` is "mise screenreel", an original instrumental jdx made in Suno (Suno id `981f87f2-45ed-479a-a879-eff90d0b3b37`, created 2026-10-02T13:07:01Z), arranged onto the reel by the fit tool from `docs/.vitepress/showreel-bed/bed.toml`.

- **The download:** a WAV, 5:59.72 (17,266,560 frames), 48 kHz, 16-bit stereo, 69,080,080 bytes. Its sha256 was not recorded before the file was moved, and it is not committed.
- **The tempo:** an exact 120 BPM, the first downbeat at 0.105 s (5,040 frames). Against an exact 120 BPM grid from that downbeat, its 532 beats with a clear attack sit within |residual| median 1 ms, p95 3 ms (the build's analysis). With `--bpm 120 --first-downbeat 0.105` every bar line is already on its frame, so the build trims the 5,040 frames before the first downbeat and does not stretch it: no Rubber Band, no pitch shift.
- **The form:** 179 whole bars of 2 s, in blocks: intro 0-15 (no drums), groove 16-43, break 44-47, groove 48-75, riser 76-79, breakdown 80-95 (no drums, no hats), groove 96-109, break 110-111, groove 112-135 (F with its third unclear: the A is in the bass), build 136-143, groove 144-155, break 156-159, the loudest groove 160-167, wind-down 168-171, and the outro 172-178, which rings out. Its key reads as F, a close call between F major and F minor (A and A♭ both sound); the build prints a chord chart per section, and the pitched stingers' charts (`harmony.ts`) follow it.

### The arrangement

`bed.toml` plays 218 reel bars from the track's 179, with some bars twice and a few cut. Every repeat or cut is on a section's bar line but two: `lock`'s repeat of the fourth groove at 5:36 (reel bar 168, six seconds into `lock`) and the jump to the loudest groove on `clone`'s chorus downbeat (6:38, reel bar 199). Every act opens on a drop or a change. Source bars are the track's, from its first downbeat:

| Reel bars | Reel time | Under                                             | Source bars                                                                                                        |
| --------- | --------- | ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| 0-6       | 0:00-0:14 | `open`                                            | intro 0-6                                                                                                          |
| 7-20      | 0:14-0:42 | `pitch`                                           | intro 2-15, so the drop lands on Act I's ticket                                                                    |
| 21-54     | 0:42-1:50 | Act I                                             | groove 16-26, then 25-47 from `registry` (two Gm bars twice), its break under `backends`' last four bars           |
| 55-84     | 1:50-2:50 | Act II                                            | groove 48-59, then 62-79 from `packslip` (two Fm bars cut), its riser ending on Act III's ticket                   |
| 85-104    | 2:50-3:30 | Act III                                           | breakdown 80-90, then 87-95 from `redact` (four bars twice)                                                        |
| 105-143   | 3:30-4:48 | Act IV                                            | 96-134 as they come: the drop on the ticket, the groove, the short break and the F major groove (its last bar cut) |
| 144-167   | 4:48-5:36 | Act V, Act VI's ticket, `lock`'s first three bars | 136-159 as they come: the build, the groove and its break                                                          |
| 168-174   | 5:36-5:50 | `lock`                                            | groove 144-150 again, fading out over 400 ms to its last bar line                                                  |
| 175-176   | 5:50-5:54 | `lock`'s last bar, Act VII's ticket               | none: the bed rests, so the whip and its riser have the stage                                                      |
| 177-191   | 5:54-6:24 | `bootstrap`, `breath`                             | breakdown 80-94 again; the section fader takes the breath down                                                     |
| 192-198   | 6:24-6:38 | `clone`                                           | build 137-143                                                                                                      |
| 199-217   | 6:38-7:16 | `clone`'s chorus, `morph`, `end`                  | 160-178: the loudest groove from the chorus downbeat, the wind-down under the morph, the outro under the end card  |

Where a segment carries straight on from the one before (reel bars 21, 55, 85 and 105: it starts on the source bar that one ends on), the join is a plain cut, which is seamless; an equal-power crossfade of the same audio would bump it by 3 dB. The other joins crossfade over 30 ms, ending on the bar line. The outro's tail is still just audible at 436.0 s (-50.5 dBFS in the last 20 ms before the level is set), so the build fades the last 2 s (`tail_fade_ms`) and warns; the file ends at -140 dBFS.

### The build

```sh
mise run docs:showreel-bed -- build mise-screenreel.wav --bpm 120 --first-downbeat 0.105 \
  --out docs/.vitepress/theme/showreel/score/bed.opus
```

Every other option at its default: `--spec bed.toml`, `--semitones 0`, `--lufs -32`, `--peak -3`, `--bitrate 192k`, `--map fitted`.

The result, as `verify` measures it (`mise run docs:showreel-bed -- verify docs/.vitepress/theme/showreel/score/bed.opus`):

- 11,505,402 bytes, sha256 `251d70c8dbee11bc82401bc4379ee3e510dab9734b2239c1b7304e534c01924a`
- 20,928,000 frames decoded, 436.000 s
- -32.0 LUFS integrated, LRA 8.7 LU, true peak -19.7 dBTP; the last 20 ms at -140.1 dBFS
- 111 of the 218 bar lines have a clear onset, |deviation| from the grid median 1 ms, 90th percentile 3 ms; the downbeat phase reads beat 1

### In the render

`BED.gainDb` is 0.7 and `BED.duck` 0.5. The sections' levels (`Part.level`) are all 1 but Act III's (the `env` ticket, `vars`, `redact`: 1.4, the breakdown lifted), `clone`'s (1.1, the climax) and the breath's (0.07, 23 dB under bootstrap's: a recorded bed does not go quiet on its own, so the fader takes the whole dip). The whole reel renders at -17.0 LUFS integrated, LRA 8.1 LU, true peak -2.8 dBTP (`aube run showreel:video --edition source --audio-only`, ffmpeg `ebur128`). The master's EQ and `MAKEUP` (`audio.ts`) are as they were voiced on the synthesized band.

## The file

What the page expects, and what the fit tool writes and checks:

- Ogg Opus, 48 kHz, stereo
- already on reel time: decoded sample 0, after Opus's pre-skip (which Chromium and ffmpeg both honour), is reel time 0
- 436.000 s (20,928,000 frames), the whole reel, with the bed's own ending under the end card (reel bars 211 to 218) and silence by 436.0 s
- 120 BPM 4/4, every bar line on the reel's grid: a bar is 2.000 s (96,000 frames), two of the reel's 60 BPM beats

How it plays: the renderer (`showreel-video.mjs`) reads the file, refuses it unless its sha256 is `BED.sha256`, hands its bytes to the page and decodes them there with `decodeAudioData`. The renderer refuses anything but two-channel Ogg Opus, which always decodes at 48 kHz (it checks the stream's header in node: `decodeAudioData` would resample any other rate without complaint), and a decoded bed shorter than the reel. `audio.ts` plays the bed on the music bus at `BED.gainDb`, starting on an exact frame. It ducks under the accents that ask for it (a line's seat, a stamp's thunk, the error stab, the pilot light, the rewind, the service bell, the slams and Act III's clicks; `Mix.duck`), at `BED.duck` of the depth each one asks for, and follows the sections' fader (`index.ts` `arc`). It then goes through the master chain and its limiter, mixed to about -17 LUFS integrated with the true peak under -2 dBTP before the AAC encode. Nothing is fetched. The render cache key (`xtasks/docs/showreel`) hashes `bed.opus`, so a new bed renders the reel again. It skips `.md` files, so editing this one does not.

## Provenance

Recorded in the commit that brought "mise screenreel" in, with `bed.opus` and `bed.ts`. What the track's file and the session did not record is marked so, not guessed.

| Field           | Value                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Made by         | jdx                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Suno plan       | Pro (paid)                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Suno model      | v6 (jdx, in the session that fitted it)                                                                                                                                                                                                                                                                                                                                                                                                             |
| Date generated  | 2026-10-02T13:07:01Z (the track's creation time on Suno)                                                                                                                                                                                                                                                                                                                                                                                            |
| Title and link  | "mise screenreel", Suno id `981f87f2-45ed-479a-a879-eff90d0b3b37`                                                                                                                                                                                                                                                                                                                                                                                   |
| Style prompt    | the session's deep-house prompt, which jdx chose: "instrumental, mellow deep house, 120 BPM, F minor, steady four-on-the-floor kick, warm Rhodes chords, round sub bass, soft plucked synth arpeggio, airy pads, sparse percussion, clean spacious mix, relaxed and friendly, long evolving arrangement with a drumless breakdown, a gradual build and one bigger final section, ends on a held chord"; any edit made to it in Suno is not recorded |
| Lyrics          | none: instrumental                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Other settings  | not recorded (the session proposed Custom mode, Instrumental on, and excluding vocals, choir, vocal chops, marimba, celesta, pizzicato strings, brass, organ, distorted guitar, EDM drop and dubstep; jdx did not confirm them)                                                                                                                                                                                                                     |
| Downloaded file | WAV, 5:59.72 (17,266,560 frames), 48 kHz, 16-bit stereo, 69,080,080 bytes; its sha256 was not recorded before the file was moved (the WAV is not committed)                                                                                                                                                                                                                                                                                         |
| Fit             | `bed.toml` as committed; `build --bpm 120 --first-downbeat 0.105`, every other option at its default ("The build" above)                                                                                                                                                                                                                                                                                                                            |
| Tools           | ffmpeg 9.0.1 with libopus 1.6.1; Rubber Band 4.0.0 on PATH but not run (no stretch); uv 0.12.21, which resolved Python 3.13.14, librosa 1.0.0, numpy 2.5.3, scipy 1.18.1, soundfile 0.14.0 and soxr 1.1.0 under `bed.py`'s `exclude-newer` pin                                                                                                                                                                                                      |
| Result          | `bed.opus` sha256 `251d70c8dbee11bc82401bc4379ee3e510dab9734b2239c1b7304e534c01924a`, as pinned in `bed.ts`; `BED.gainDb` 0.7, `BED.duck` 0.5                                                                                                                                                                                                                                                                                                       |
| Suno terms      | not recorded (ask jdx)                                                                                                                                                                                                                                                                                                                                                                                                                              |

With the same tool versions, the same WAV, spec and options give the same bytes (the fit tool's README). The WAV's hash is missing, so a rebuild from a fresh download can be checked only against the pinned hash of `bed.opus`: it matches if the download carries the same audio.

## Ownership and licence

The bed is an original instrumental that jdx generated on a paid (Pro) Suno plan of jdx's own. Suno's terms assign ownership of songs generated on a paid plan to the subscriber, so the audio is jdx's. It is not covered by the repository's MIT licence (`LICENSE`), and that licence grants no rights to it. It is committed only so the docs build can render the showreel. A copy of the reel's code needs its own bed.

## Fitting a new bed

`mise run docs:showreel-bed` (`xtasks/docs/showreel-bed`) turns a downloaded track into `bed.opus`. Its README, `docs/.vitepress/showreel-bed/README.md`, has the whole workflow: generate and download the WAV, analyse it, arrange its bars on the reel (a spec of its own: `bed.toml`'s bar numbers are "mise screenreel"'s), build, verify, listen in a draft render, and commit `bed.opus` and `bed.ts` together. The tool is not part of the render.

1. **Before anything, record the download:** its sha256 (`shasum -a 256`), length, rate and size, and the Suno model, style prompt and settings it was generated with, for the provenance table.
2. **Fit it.** A track on an exact 120 BPM clock, as Suno's appear to be and "mise screenreel" is, needs no stretch: read its first downbeat with `analyse --bpm 120` and build with `--bpm 120 --first-downbeat SECONDS`, and the build only trims it. Do not trust a plain `analyse` there: on "mise screenreel" its unpinned fit read 119.72 BPM, p95 residual 202 ms and a first downbeat at 1.434 s, and suggested `--map detected`; with `--bpm 120` it reads 0.105 s, p95 3 ms. For such a track the fix is `--bpm 120`, not `--map detected`. Otherwise the build stretches it onto the grid with Rubber Band. Either way, `verify` must pass.
3. **Pin it:** `BED.sha256` in `bed.ts`, the line the build prints. The renderer refuses any other file.
4. **Level it:** `BED.gainDb`, against the -17 LUFS integrated target, with the true peak under -2 dBTP. Render the whole reel's audio (`aube run showreel:video --edition source --audio-only <out.wav>`), measure it (`ffmpeg -nostats -i <out.wav> -af ebur128=peak=true -f null -`), and move `gainDb` by the difference. The fit tool's -32 LUFS default is staged against the master, whose makeup lifts the bed about 11 dB: "mise screenreel" at -32 LUFS needed 0.7 dB; a bed fitted at -20 LUFS put the reel at -11.6 with the limiter clamped throughout.
5. **Duck it:** `BED.duck`, by ear: deep enough that the effects speak, shallow enough that the bed does not pump (0.5 for "mise screenreel").
6. **Set the sections' fader** (`index.ts` `Part.level`, `arc`): 1 by default, since a produced track carries its own dynamics. Move a section only where the new arrangement needs it, and keep the breath near silent (the fader takes the whole dip there). Measure each section of the whole-reel render, not only the integrated figure.
7. **Listen through the master** (`audio.ts`: the EQ and `MAKEUP` voiced on the synthesized band), and change it only if the bed needs it.
8. **The stingers' chords:** the pitched stingers are struck on chord charts (`harmony.ts`, and the `CHANGES` of each part with a chart of its own: `clone.ts`, `depends.ts`, `skip.ts`, `switch.ts` and `use.ts`) that follow the bed's; the build and `verify` print the new bed's chart per section, to retune them against.
9. **Update this file:** "The track", the arrangement and the provenance table, in the commit that changes `bed.opus` and `bed.ts`.

## Landing-page films

The source bed above remains the soundtrack of `--edition source`. The tour
and overview use `music.opus`, the original supplied "mise screenreel.wav"
trimmed by 0.105 s to its first downbeat and attenuated by 17.1 dB (the original
measures -14.9 LUFS, giving a -32 LUFS music bus). It is encoded as 48 kHz
stereo Opus at 128 kbps and pinned by `film-music.ts`.

To regenerate that asset from the supplied original (no stretching):

```sh
ffmpeg -i 'mise screenreel.wav' \
  -af 'atrim=start=0.105,asetpts=PTS-STARTPTS,volume=-17.1dB' \
  -c:a libopus -b:a 128k -ar 48000 \
  docs/.vitepress/theme/showreel/score/music.opus
```

Each film is cut on the song's own bar lines (`film-music.ts` `musicCuts`),
never at a picture edit, and ends on the song's complete wind-down, outro and
ring-out, which dies out 0.385 s before the film does. The tour (4:58) plays
bars 11–79 straight through (the intro's tail under the open, the first groove
dropping at 0:10 as the open's card leaves and the switch begins, the break
under `mise use jq`'s install, the second groove, the riser ending in
depends), then 96–151 (the third and fourth grooves, the build under
bootstrap, the fifth groove to its last phrase), skipping only the drumless
breakdown and that phrase, then 156 to the end: the break under the clone
command typing, the loudest groove dropping at 4:18 on its install rows and
carrying the climax, the wind-down under the morph's lift, the outro's soft
kick on the toque's drop at 4:42, and the ring-out under the end card. The
overview (1:22) plays bars 11–39, then 168 to the end: the wind-down under
depends' lanes, the outro under the end card. Joins crossfade the outgoing
tail over 50 ms under the incoming downbeat. Arrangement and pin changes
belong in `film-music.ts`; the renderer checks the pin before decoding, and
`test/film-music.test.ts` holds each arrangement to the film's length, the
first groove's drop and the outro's place under the end card.

`film-audio.ts` renders unmastered source effects without any music, cuts those
with the picture, remaps their duck cues, and mixes them with the independently
arranged song through the shared delivery master. The original source fader,
especially the near-silent breath, is excluded from the films.
