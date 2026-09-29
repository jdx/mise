// Adapted from jdx/hk@37937824 docs/.vitepress/showreel.data.ts.
//
// The landing-page showreel, as the page and the head tags need it: the
// rendered files' versioned URLs, the reel's length, and its chapters with
// the captions they show, for the screen-reader list under the player. It is
// null when this build has no render, and then the page is exactly what it
// is without one: no player, no og:video, and "Watch the demo" goes to
// /demo. Only the capture set and the timeline are read for the chapters
// (or the chapters kept with the render, SHOWREEL_CHAPTERS); nothing that
// draws is imported, so it is safe in the config and on the server.
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CHAPTERS, DURATION } from "./theme/showreel/timeline.ts";

const configDir = dirname(fileURLToPath(import.meta.url));
const videoPath = resolve(configDir, "../public/showreel.mp4");
const video120Path = resolve(configDir, "../public/showreel-120.mp4");
const posterPath = resolve(configDir, "../public/showreel-poster.jpg");
const chaptersPath = resolve(configDir, "../public/showreel-chapters.vtt");
const boardPath = resolve(configDir, "theme/showreel/sections.json");

export interface ShowreelFiles {
  /** Site-relative URL of the 60 fps MP4. */
  src: string;
  /**
   * The same reel at 120 fps, with its average bitrate in bits per second,
   * or null when this render has none.
   */
  video120: { src: string; bitrate: number } | null;
  /** Site-relative URL of its poster frame. */
  poster: string;
  /** Site-relative URL of its chapters track, versioned like the files. */
  track: string;
}

export interface ShowreelChapter {
  id: string;
  label: string;
  /** Where the chapter starts, in seconds (the chapters track's cue). */
  start: number;
  /** Its captions, as the reel shows them; empty when it has none. */
  text: string;
}

export interface ShowreelData extends ShowreelFiles {
  /** The 60 fps file's length in seconds. */
  seconds: number;
  chapters: ShowreelChapter[];
}

// Versioned so browsers and link previews that cached an earlier render fetch
// the new one.
const version = (file: Buffer) =>
  createHash("sha256").update(file).digest("hex").slice(0, 12);

/** The first box of this type between start and end, as [body, end]. */
function findBox(mp4: Buffer, type: string, start = 0, end = mp4.length) {
  for (let at = start; at + 8 <= end;) {
    const size = mp4.readUInt32BE(at);
    if (size < 8) return null;
    if (mp4.toString("latin1", at + 4, at + 8) === type)
      return [at + 8, at + size];
    at += size;
  }
  return null;
}

/** An MP4's length in seconds, from its movie header. */
function mp4Seconds(mp4: Buffer): number | null {
  const moov = findBox(mp4, "moov");
  const mvhd = moov && findBox(mp4, "mvhd", moov[0], moov[1]);
  if (!mvhd) return null;
  // Version 1 widens the times to 64 bits.
  const at = mvhd[0];
  const [timescale, duration] =
    mp4[at] === 1
      ? [mp4.readUInt32BE(at + 20), Number(mp4.readBigUInt64BE(at + 24))]
      : [mp4.readUInt32BE(at + 12), mp4.readUInt32BE(at + 16)];
  return timescale && duration ? duration / timescale : null;
}

/**
 * The showreel rendered by `mise run docs:showreel`, or null when this build
 * has no render. The landing page plays the 120 fps file where it decodes
 * smoothly and the 60 fps file elsewhere; the homepage offers the 60 fps
 * file as og:video.
 */
export function showreelFiles(): ShowreelFiles | null {
  // The player needs its chapters track as well as the video and poster.
  if (![videoPath, posterPath, chaptersPath].every((p) => existsSync(p)))
    return null;
  const video = readFileSync(videoPath);
  const video120 = existsSync(video120Path) ? readFileSync(video120Path) : null;
  const seconds = video120 && mp4Seconds(video120);
  return {
    src: `/showreel.mp4?v=${version(video)}`,
    video120:
      video120 && seconds
        ? {
            src: `/showreel-120.mp4?v=${version(video120)}`,
            bitrate: Math.round((video120.length * 8) / seconds),
          }
        : null,
    // Versioned by its own bytes: a change to the poster frame alone must
    // reach browsers too.
    poster: `/showreel-poster.jpg?v=${version(readFileSync(posterPath))}`,
    // Versioned too, so a browser that cached an earlier render's track
    // never seeks a new render with its chapter times.
    track: `/showreel-chapters.vtt?v=${version(readFileSync(chaptersPath))}`,
  };
}

interface BoardSection {
  id: string;
  act: string;
  captions: { lines: string[] }[];
}

/**
 * What the captions' version tokens are filled from (captures.ts tokens()),
 * and which machine 2 recorded the new-machine act, read from the capture
 * set the render was made from, found as theme/showreel/load.ts finds it:
 * SHOWREEL_CAPTURES, else a capture run in the rig's out/, else the
 * committed reference set. The docs deploy points SHOWREEL_CAPTURES at the
 * copy it keeps with each render. Null without a capture set.
 */
function captureFacts(): {
  tokens: Record<string, string>;
  variant: string;
} | null {
  const out = [
    process.env.SHOWREEL_CAPTURES,
    resolve(configDir, "showreel-capture/out"),
    resolve(configDir, "theme/showreel/test/captures"),
  ].find((dir) => dir && existsSync(join(dir, "versions.json")));
  if (!out) return null;
  const versionsFile = join(out, "versions.json");
  const { node } = JSON.parse(readFileSync(versionsFile, "utf8"));
  const run = join(
    out,
    "runs",
    process.env.SHOWREEL_CAPTURE_RUN ?? "a",
    "run-machine2.json",
  );
  const machine2 = existsSync(run) ? JSON.parse(readFileSync(run, "utf8")) : {};
  return {
    tokens: {
      "node.lts_major": String(node.lts_major),
      "node.lts_version": String(node.lts_version),
      "node.other_major": String(node.other_major),
      "node.other_version": String(node.other_version),
    },
    variant: machine2.variant ?? machine2.machine2 ?? "systemd",
  };
}

/**
 * Each chapter's captions, as the reel shows them, without their code marks
 * (the end card's chapter has none). A caption is left out rather than
 * quoted with a gap or in the wrong words: one whose version tokens the
 * build cannot fill, and bootstrap's second caption unless machine 2 ran
 * systemd (the reel rewords it for the plain fallback, storyboard.ts).
 */
function chapters(): ShowreelChapter[] {
  const board: BoardSection[] = JSON.parse(readFileSync(boardPath, "utf8"));
  const facts = captureFacts();
  const quote = (s: BoardSection) =>
    s.captions.flatMap((c, k) => {
      if (s.id === "bootstrap" && k === 1 && facts?.variant !== "systemd")
        return [];
      let gap = false;
      const text = c.lines
        .join(" ")
        .replace(/\{([a-z_.]+)\}/g, (_, name: string) => {
          const value = facts?.tokens[name];
          if (value === undefined) gap = true;
          return value ?? "";
        })
        .replaceAll("`", "");
      return gap ? [] : [text];
    });
  return CHAPTERS.map((chapter) => {
    const sections = chapter.sections.map((id) => {
      const s = board.find((b) => b.id === id);
      if (!s) throw new Error(`sections.json has no section "${id}"`);
      return s;
    });
    const captions = sections.flatMap(quote);
    return {
      id: chapter.id,
      label: chapter.label,
      start: chapter.start,
      text: captions.join(" "),
    };
  });
}

/**
 * The chapters kept with the render this build serves (the docs deploy sets
 * SHOWREEL_CHAPTERS to the copy `mise run docs:showreel` keeps beside it),
 * so a restored render is never paired with a newer storyboard's chapters;
 * null when there is none.
 */
function keptChapters(): ShowreelChapter[] | null {
  const file = process.env.SHOWREEL_CHAPTERS;
  if (!file || !existsSync(file)) return null;
  const kept: unknown = JSON.parse(readFileSync(file, "utf8"));
  const ok =
    Array.isArray(kept) &&
    kept.every(
      (c) =>
        typeof c?.id === "string" &&
        typeof c?.label === "string" &&
        typeof c?.start === "number" &&
        typeof c?.text === "string",
    );
  return ok ? (kept as ShowreelChapter[]) : null;
}

export function showreelData(): ShowreelData | null {
  const files = showreelFiles();
  if (!files) return null;
  return {
    ...files,
    seconds: mp4Seconds(readFileSync(videoPath)) ?? DURATION,
    chapters: keptChapters() ?? chapters(),
  };
}

export declare const data: ShowreelData | null;

export default {
  // Pick up a render, a new capture set or a storyboard edit while the dev
  // server is running.
  watch: [
    videoPath,
    video120Path,
    posterPath,
    boardPath,
    resolve(configDir, "showreel-capture/out/versions.json"),
  ],
  load: showreelData,
};
