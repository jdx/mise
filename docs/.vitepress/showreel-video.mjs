// from jdx/hk@37937824 docs/.vitepress/showreel-video.mjs
// Renders the showreel to docs/public/showreel-120.mp4 and showreel.mp4 (the
// same reel at 120 and 60 fps), with its poster frame in
// docs/public/showreel-poster.jpg. The landing page plays the 120 fps file
// where the browser can decode it smoothly and the 60 fps file elsewhere,
// and offers the 60 fps file as og:video. Every frame of the reel
// is a pure function of time and of the capture set, which it loads from
// docs/.vitepress/showreel-capture/out (theme/showreel/load.ts; a missing
// take draws a labelled box). The score is rendered offline, and the end
// card's recording (theme/showreel/score/song.ts) is cut from
// docs/.vitepress/theme/showreel/score/mise-en-place.mp3 and mixed over it with ffmpeg. None of the
// outputs is committed (docs/.gitignore).
//
// Needs ffmpeg on PATH and a Chromium: SHOWREEL_CHROMIUM (or CHROME_PATH),
// else Playwright's headless shell (`aube exec playwright-core install
// chromium-headless-shell`); see showreel-chromium.mjs.
//
// Drafts, for watching or hearing a change without touching the published
// files:
//
//   aube run showreel:video --out <file.mp4> [--fps 30|60|120] [--from <s>] [--until <s>] [--section <id>] [--burn-in]
//   aube run showreel:video --audio-only <file.wav> [--from <s>] [--until <s>] [--section <id>]
//
//   --out <file.mp4>         one video, at --fps (default 60), to this file only
//   --fps 30|60|120          the draft's frame rate; 30 for animatics
//   --from <seconds>         the first frame, reel time (default 0); with
//                            --until it picks a frame range for a quick
//                            preview. Both snap to the draft's frame grid.
//   --until <seconds>        stop before this reel time (default: the end)
//   --section <id>           the range of one section, as --from and --until
//   --burn-in                burn the timecode, act, section and beat into
//                            the corner of every frame
//   --audio-only <file.wav>  only the soundtrack (the score and the end
//                            card's recording), as 48 kHz 16-bit stereo PCM
//
// With no arguments it makes the full render. Every other flag needs --out
// or --audio-only, which must name a file outside docs/public, so only a
// full render ever replaces the files the site deploys.

import { once } from "node:events";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";
import { chromiumPath } from "./showreel-chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
// The site deploys everything in public/.
const PUBLIC = resolve(here, "../public");
const poster = join(PUBLIC, "showreel-poster.jpg");
// Written outside public/ and renamed over the outputs only once ffmpeg
// succeeds, so a failed or interrupted render never leaves a partial file
// for the build to publish. The cache directory is on the same filesystem
// (so the rename is atomic) and is never deployed.
const staging = resolve(here, "cache/showreel");
const posterPartial = join(staging, "showreel-poster.jpg");
const WIDTH = 1920;
const HEIGHT = 1080;
// One pass renders every frame at 120 fps. Every 60 fps frame is also a
// 120 fps frame (i/60 == 2i/120), so the 60 fps file takes every other one.
// 1080p120 needs H.264 level 5.1; the 60 fps file, which link previews play,
// stays at 4.2. A 30 fps draft needs only 4.0.
const VIDEOS = [
  { name: "showreel-120.mp4", fps: 120, level: "5.1" },
  { name: "showreel.mp4", fps: 60, level: "4.2" },
];
const LEVELS = { 30: "4.0", 60: "4.2", 120: "5.1" };
// Frames do not depend on each other, so pages render them side by side. On
// a 4-vCPU runner x264 is the floor: two pages render about 12% faster than
// one, and a third adds nothing. Bigger machines get up to four.
const PAGES = Math.max(1, Math.min(4, availableParallelism() >> 1));
// Frames each page has queued, so one page hands back a frame while it draws
// the next.
const DEPTH = 2;
// Rendered before the reel starts and trimmed, so the score's compressor
// lookahead can place the first sounds exactly.
const PRE_ROLL = 0.2;
const SAMPLE_RATE = 48000;

function fail(message) {
  console.error(`showreel-video: ${message}`);
  process.exit(2);
}

function parseArgs(argv) {
  const o = { burnIn: false };
  const value = (i, flag) => {
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) fail(`${flag} needs a value`);
    return v;
  };
  const seconds = (v, flag) => {
    const n = Number(v);
    if (v.trim() === "" || !Number.isFinite(n) || n < 0)
      fail(`${flag} must be a number of seconds, not "${v}"`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case "--out":
        o.out = resolve(value(i++, a));
        break;
      case "--audio-only":
        o.audioOnly = resolve(value(i++, a));
        break;
      case "--fps":
        o.fps = Number(value(i++, a));
        break;
      case "--from":
        o.from = seconds(value(i++, a), a);
        break;
      case "--until":
        o.until = seconds(value(i++, a), a);
        break;
      case "--section":
        o.section = value(i++, a);
        break;
      case "--burn-in":
        o.burnIn = true;
        break;
      case "-h":
      case "--help": {
        const head = readFileSync(fileURLToPath(import.meta.url), "utf8").split(
          "\n\nimport",
        )[0];
        console.log(head.replace(/^\/\/ ?/gm, ""));
        process.exit(0);
      }
      default:
        fail(`unknown argument "${a}" (see --help)`);
    }
  }
  if (o.out && o.audioOnly) fail("pass one of --out and --audio-only");
  const drafting =
    o.fps !== undefined ||
    o.from !== undefined ||
    o.until !== undefined ||
    o.section !== undefined ||
    o.burnIn;
  if (!o.out && !o.audioOnly && drafting) {
    fail(
      "--fps, --from, --until, --section and --burn-in make a draft, which needs --out <file.mp4> or --audio-only <file.wav>; only a full render writes docs/public",
    );
  }
  if (o.audioOnly && (o.fps !== undefined || o.burnIn))
    fail(
      "--fps and --burn-in are for video; --audio-only renders only the score",
    );
  if (o.fps !== undefined && !(o.fps in LEVELS))
    fail(`--fps must be 30, 60 or 120`);
  if (
    o.section !== undefined &&
    (o.from !== undefined || o.until !== undefined)
  )
    fail("pass --section or --from/--until, not both");
  if (o.from !== undefined && o.until !== undefined && !(o.until > o.from))
    fail("--until must come after --from");
  if (o.out && !/\.mp4$/i.test(o.out))
    fail("--out names the draft's .mp4 file");
  if (o.audioOnly && !/\.wav$/i.test(o.audioOnly))
    fail("--audio-only names the score's .wav file");
  const target = o.out ?? o.audioOnly;
  if (target) {
    // Outside means up and out of public/ (or on another drive), not merely a
    // name that starts with two dots, such as public/..draft.mp4.
    const rel = relative(PUBLIC, target);
    const outside =
      rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel);
    if (!outside)
      fail(
        `a draft is never written to ${PUBLIC}, which the site deploys; run without arguments for the full render`,
      );
  }
  return o;
}

const opts = parseArgs(process.argv.slice(2));
const draft = Boolean(opts.out || opts.audioOnly);

/** A small ESM bundle of `contents`, imported in node. */
async function nodeModule(contents) {
  const bundled = await build({
    stdin: { contents, resolveDir: here, loader: "ts" },
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node22",
    write: false,
    logLevel: "error",
  });
  return import(
    `data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].text).toString("base64")}`
  );
}

// The timeline, the font list, the capture set's loader and the end card's
// recording, read in node from the same sources the page draws with.
const { FONTS, SECTIONS, sec, loadFacts, SONG } =
  await nodeModule(`export { FONTS } from "./theme/showreel/fonts.ts";
export { SECTIONS, sec } from "./theme/showreel/timeline.ts";
export { loadFacts } from "./theme/showreel/load.ts";
export { SONG } from "./theme/showreel/score/song.ts";`);
const REPO = resolve(here, "../..");
// The capture set every terminal line and version number comes from. A
// missing take draws a labelled "CAPTURE MISSING" box.
const { facts, report } = loadFacts(REPO);
for (const line of report) console.log(`showreel-video: ${line}`);
if (
  opts.section !== undefined &&
  !SECTIONS.some((s) => s.id === opts.section)
) {
  fail(
    `no section "${opts.section}"; sections are ${SECTIONS.map((s) => s.id).join(", ")}`,
  );
}

const bundle = await build({
  stdin: {
    contents: `export { createReel, POSTER_TIME, resetTypeCache } from "./theme/showreel/reel.ts";
export { playScore } from "./theme/showreel/audio.ts";`,
    resolveDir: here,
    loader: "ts",
  },
  bundle: true,
  format: "iife",
  globalName: "Showreel",
  target: "es2022",
  write: false,
  logLevel: "error",
});

/**
 * The end card's recording (score/song.ts), mixed over the score at mux
 * time: each segment cut from the MP3's own decode (t = 0 is its first
 * decoded sample), faded at both ends, set to the fixed gain and placed at
 * its reel time, then the whole mix trimmed to the range rendered. Writes
 * `out`, 48 kHz 16-bit stereo, the same length as `score`.
 */
function spliceRecording(score, out, from, duration) {
  const mp3 = join(REPO, SONG.file);
  const sha = createHash("sha256").update(readFileSync(mp3)).digest("hex");
  if (sha !== SONG.sha256)
    throw new Error(
      `${SONG.file} is not the recording the end card was measured on (sha256 ${sha}, want ${SONG.sha256})`,
    );
  const S = (t) => Math.round(t * SAMPLE_RATE);
  const n = SONG.segments.length;
  const parts = [
    `[1:a]aresample=${SAMPLE_RATE},asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:channel_layouts=stereo,asplit=${n}${SONG.segments.map((_, i) => `[r${i}]`).join("")}`,
  ];
  SONG.segments.forEach((seg, i) => {
    const len = seg.to - seg.from;
    parts.push(
      `[r${i}]atrim=start_sample=${S(seg.from)}:end_sample=${S(seg.to)},asetpts=PTS-STARTPTS,` +
        `afade=t=in:st=0:d=${seg.fadeIn},afade=t=out:st=${(len - seg.fadeOut).toFixed(4)}:d=${seg.fadeOut},` +
        `volume=${SONG.gainDb}dB,adelay=delays=${S(seg.at)}S:all=1[s${i}]`,
    );
  });
  parts.push(
    `[0:a]aformat=sample_fmts=fltp:channel_layouts=stereo,adelay=delays=${S(from)}S:all=1[sc]`,
    `[sc]${SONG.segments.map((_, i) => `[s${i}]`).join("")}amix=inputs=${n + 1}:normalize=0:duration=longest,` +
      `atrim=start_sample=${S(from)}:end_sample=${S(from) + S(duration)},asetpts=PTS-STARTPTS[out]`,
  );
  const r = spawnSync(
    "ffmpeg",
    [
      "-y",
      "-loglevel",
      "error",
      "-i",
      score,
      "-i",
      mp3,
      "-filter_complex",
      parts.join(";"),
      "-map",
      "[out]",
      "-ar",
      String(SAMPLE_RATE),
      "-c:a",
      "pcm_s16le",
      out,
    ],
    { stdio: ["ignore", "inherit", "inherit"] },
  );
  if (r.status !== 0)
    throw new Error(`ffmpeg could not splice the recording (exit ${r.status})`);
}

/** 16-bit stereo PCM, base64 from the page, as a WAV file. */
function wavFile(pcm) {
  const samples = Buffer.from(pcm, "base64");
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + samples.length, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(2, 22);
  header.writeUInt32LE(SAMPLE_RATE, 24);
  header.writeUInt32LE(SAMPLE_RATE * 4, 28);
  header.writeUInt16LE(4, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(samples.length, 40);
  return Buffer.concat([header, samples]);
}

const fps = opts.fps ?? 60;
const FPS = opts.out ? fps : 120;
const encoders = opts.audioOnly
  ? []
  : opts.out
    ? [
        {
          name: basename(opts.out),
          fps,
          level: LEVELS[fps],
          out: opts.out,
          // Beside the draft, so the rename cannot cross filesystems.
          partial: join(dirname(opts.out), `.${basename(opts.out)}.partial`),
        },
      ]
    : VIDEOS.map((video) => ({
        ...video,
        out: join(PUBLIC, video.name),
        partial: join(staging, video.name),
      }));

if (!draft) mkdirSync(staging, { recursive: true });
if (opts.out) mkdirSync(dirname(opts.out), { recursive: true });
const started = performance.now();
const executablePath = chromiumPath();
const browser = await chromium.launch({ executablePath });
const work = mkdtempSync(join(tmpdir(), "showreel-"));
try {
  let pageError = null;
  const fonts = FONTS.map((font) => ({
    ...font,
    bytes: readFileSync(resolve(here, "fonts", font.file)).toString("base64"),
  }));
  const pages = await Promise.all(
    Array.from({ length: opts.audioOnly ? 1 : PAGES }, async () => {
      const page = await browser.newPage();
      page.on("pageerror", (err) => {
        pageError ??= err;
      });
      await page.setContent(
        `<canvas id="reel" width="${WIDTH}" height="${HEIGHT}"></canvas>`,
      );
      await page.addScriptTag({ content: bundle.outputFiles[0].text });
      await page.evaluate(
        async ({ fonts, burnIn, facts }) => {
          for (const font of fonts) {
            const bytes = Uint8Array.from(atob(font.bytes), (c) =>
              c.charCodeAt(0),
            );
            const face = new FontFace(font.family, bytes, {
              weight: font.weight,
              style: font.style,
            });
            document.fonts.add(await face.load());
          }
          // layout() caches measurements; none may survive from a fallback font.
          Showreel.resetTypeCache();
          const canvas = document.getElementById("reel");
          // The capture set (theme/showreel/load.ts): the takes' screens,
          // files and versions, as JSON.
          window.facts = facts;
          window.reel = Showreel.createReel(window.facts, { burnIn });
          window.ctx = canvas.getContext("2d", { alpha: false });
        },
        { fonts, burnIn: opts.burnIn, facts },
      );
      return page;
    }),
  );
  const [page] = pages;
  // The reel's own length (timeline.ts), so video and score follow its timing.
  const full = await page.evaluate(() => window.reel.duration);
  const span = opts.section ? sec(opts.section) : null;
  // The range, snapped to the frame grid so every frame is one a full render draws too.
  const snap = (t) => Math.round(t * FPS) / FPS;
  const first = Math.round(
    snap(Math.min(span?.start ?? opts.from ?? 0, full)) * FPS,
  );
  const last = Math.round(
    snap(Math.min(span?.end ?? opts.until ?? full, full)) * FPS,
  );
  // Thrown, not fail(): process.exit would skip the finally that closes the browser.
  if (!(last > first))
    throw new Error(
      `no frames between ${first / FPS} and ${last / FPS} s of the ${full} s reel`,
    );
  const from = first / FPS;
  const duration = (last - first) / FPS;
  console.log(
    `Rendering ${from === 0 && duration === full ? "the whole reel" : `${from}–${from + duration} s of the reel`} (${full} s) at ${opts.audioOnly ? "48 kHz" : `${FPS} fps`} in ${executablePath ?? "Playwright's Chromium"}`,
  );

  // The score, as 16-bit stereo PCM, from the range's first frame: it plays
  // there exactly what a full render plays (test/score.test.ts).
  const pcm = await page.evaluate(
    async ({ from, duration, preRoll, rate }) => {
      const ac = new OfflineAudioContext(
        2,
        Math.ceil(rate * (duration + preRoll)),
        rate,
      );
      Showreel.playScore(ac, ac.destination, from, preRoll, window.facts);
      const buffer = await ac.startRendering();
      const skip = Math.round(preRoll * rate);
      const frames = Math.round(duration * rate);
      const left = buffer.getChannelData(0);
      const right = buffer.getChannelData(1);
      const data = new Int16Array(frames * 2);
      for (let i = 0; i < frames; i++) {
        data[i * 2] = Math.max(-1, Math.min(1, left[i + skip])) * 32767;
        data[i * 2 + 1] = Math.max(-1, Math.min(1, right[i + skip])) * 32767;
      }
      let binary = "";
      const bytes = new Uint8Array(data.buffer);
      for (let i = 0; i < bytes.length; i += 0x8000) {
        binary += String.fromCharCode.apply(
          null,
          bytes.subarray(i, i + 0x8000),
        );
      }
      return btoa(binary);
    },
    { from, duration, preRoll: PRE_ROLL, rate: SAMPLE_RATE },
  );
  if (pageError) throw pageError;
  if (opts.audioOnly) {
    mkdirSync(dirname(opts.audioOnly), { recursive: true });
    const scoreOnly = join(work, "score.wav");
    writeFileSync(scoreOnly, wavFile(pcm));
    spliceRecording(scoreOnly, opts.audioOnly, from, duration);
    const elapsed = (performance.now() - started) / 1000;
    console.log(`Rendered ${opts.audioOnly} in ${elapsed.toFixed(1)} s`);
  } else {
    const scoreOnly = join(work, "score.wav");
    writeFileSync(scoreOnly, wavFile(pcm));
    const wav = join(work, "mix.wav");
    spliceRecording(scoreOnly, wav, from, duration);

    // 1080p H.264 High with AAC and the index up front: sharp on the landing
    // page and, at 60 fps, playable by every link preview that plays video.
    // Frames arrive as JPEG, which is BT.601 YCbCr; the tag makes players
    // decode it with the same matrix.
    for (const encoder of encoders) {
      const ffmpeg = spawn(
        "ffmpeg",
        [
          "-y",
          "-loglevel",
          "error",
          "-f",
          "image2pipe",
          "-framerate",
          String(encoder.fps),
          "-c:v",
          "mjpeg",
          "-i",
          "pipe:0",
          "-i",
          wav,
          "-c:v",
          "libx264",
          "-preset",
          "slow",
          // CRF 22 tuned for animation (flat fills, hard edges): at hk's CRF 23
          // x264 left faint ghosts of departed text in the reel's flat dark
          // areas (redact's empty column, packslip's [tools] band). On
          // redact's 12 beats at 60 fps this is 3 % larger than CRF 23 and
          // cleaner than CRF 21 with aq-mode 3, which was 41 % larger.
          "-crf",
          "22",
          "-tune",
          "animation",
          "-profile:v",
          "high",
          "-level:v",
          encoder.level,
          "-pix_fmt",
          "yuv420p",
          "-colorspace",
          "bt470bg",
          "-c:a",
          "aac",
          "-b:a",
          "160k",
          "-movflags",
          "+faststart",
          "-shortest",
          // Named by the flag, since a draft's partial file has no .mp4 extension.
          "-f",
          "mp4",
          encoder.partial,
        ],
        { stdio: ["pipe", "inherit", "inherit"] },
      );
      encoder.ffmpeg = ffmpeg;
      // Fail here, inside the try, if ffmpeg cannot start at all (not on PATH).
      await once(ffmpeg, "spawn");
      encoder.exited = once(ffmpeg, "close");
      // If ffmpeg dies mid-stream, the next frame rethrows its broken pipe; keep
      // that error and the pending close from escaping the try as unhandled.
      encoder.exited.catch(() => {});
      encoder.pipeError = null;
      ffmpeg.stdin.on("error", (err) => {
        encoder.pipeError ??= err;
      });
    }

    // JPEG at 0.95 rather than PNG: the grain makes every PNG about 2.5 MB, and
    // encoding and moving one takes about three times as long. Pages finish
    // out of order, so frames are held until the ones before them are written.
    const total = last - first;
    const capture = (i) =>
      pages[i % pages.length].evaluate(
        ({ t, w, h }) => {
          window.reel.render(window.ctx, t, w, h);
          return document.getElementById("reel").toDataURL("image/jpeg", 0.95);
        },
        { t: (first + i) / FPS, w: WIDTH, h: HEIGHT },
      );
    const pending = new Map();
    let queued = 0;
    for (let i = 0; i < total; i++) {
      for (; queued < Math.min(total, i + pages.length * DEPTH); queued++) {
        const frame = capture(queued);
        // Awaited in order below; until then a failure must not go unhandled.
        frame.catch(() => {});
        pending.set(queued, frame);
      }
      const jpeg = await pending.get(i);
      pending.delete(i);
      const frame = Buffer.from(jpeg.slice(jpeg.indexOf(",") + 1), "base64");
      const drained = [];
      for (const encoder of encoders) {
        if ((first + i) % (FPS / encoder.fps)) continue;
        if (encoder.pipeError) throw encoder.pipeError;
        if (!encoder.ffmpeg.stdin.write(frame))
          drained.push(once(encoder.ffmpeg.stdin, "drain"));
      }
      await Promise.all(drained);
      if ((i + 1) % (FPS * 10) === 0) {
        const elapsed = (performance.now() - started) / 1000;
        console.log(
          `Rendered ${i + 1} of ${total} frames in ${elapsed.toFixed(0)} s`,
        );
      }
    }
    for (const encoder of encoders) encoder.ffmpeg.stdin.end();
    for (const encoder of encoders) {
      const [code] = await encoder.exited;
      if (code !== 0)
        throw new Error(`ffmpeg exited with ${code} for ${encoder.name}`);
    }
    if (pageError) throw pageError;

    if (draft) {
      for (const encoder of encoders) renameSync(encoder.partial, encoder.out);
      const elapsed = (performance.now() - started) / 1000;
      console.log(
        `Rendered ${total} frames to ${opts.out} in ${elapsed.toFixed(0)} s`,
      );
    } else {
      // The poster the player shows until someone presses play.
      const jpeg = await page.evaluate(
        ({ w, h }) => {
          window.reel.render(window.ctx, Showreel.POSTER_TIME, w, h);
          return document.getElementById("reel").toDataURL("image/jpeg", 0.9);
        },
        { w: WIDTH, h: HEIGHT },
      );
      writeFileSync(
        posterPartial,
        Buffer.from(jpeg.slice(jpeg.indexOf(",") + 1), "base64"),
      );
      if (pageError) throw pageError;
      for (const encoder of encoders) renameSync(encoder.partial, encoder.out);
      renameSync(posterPartial, poster);
      const elapsed = (performance.now() - started) / 1000;
      console.log(
        `Rendered ${encoders.map((encoder) => encoder.out).join(", ")} and ${poster} in ${elapsed.toFixed(0)} s`,
      );
    }
  }
} finally {
  // An encoder still running here was cut off by an error; its output is
  // discarded, so stop it without letting it finish the file.
  for (const encoder of encoders) encoder.ffmpeg?.kill("SIGKILL");
  await browser.close();
  rmSync(work, { recursive: true, force: true });
  for (const encoder of encoders) rmSync(encoder.partial, { force: true });
  if (!draft) rmSync(posterPartial, { force: true });
}
