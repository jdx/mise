// A recording 2D context for the text tests: it draws nothing and hands
// every string the reel sets (through the kit's ink tap, kit/ink.ts, and
// every raw fillText besides) to a callback, frame by frame: what
// forbidden.test.ts and timing.test.ts read the reel's text from.

import type { ReelFacts } from "../bible";
import { composeReel } from "../compose";
import { type InkKind, setInkSink } from "../kit/ink";
import { scenes } from "../scenes";
import { DURATION } from "../timeline";

/** A 2D context that draws nothing, and remembers every string it was asked to fill. */
export function recordingContext(
  filled: (text: string) => void,
): CanvasRenderingContext2D {
  const noop = () => {};
  const gradient = { addColorStop: noop };
  const canvas = { width: 1920, height: 1080 };
  const target: Record<string | symbol, unknown> = {
    canvas,
    font: "10px sans-serif",
    globalAlpha: 1,
    measureText(text: string) {
      const px = Number(
        /(\d+(?:\.\d+)?)px/.exec(String(target.font))?.[1] ?? 10,
      );
      return { width: Array.from(text).length * px * 0.56 };
    },
    fillText(text: string) {
      filled(String(text));
    },
    createLinearGradient: () => gradient,
    createRadialGradient: () => gradient,
    createPattern: () => null,
    getTransform: () => ({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }),
    getImageData: (_x: number, _y: number, w: number, h: number) => ({
      data: new Uint8ClampedArray(w * h * 4),
    }),
    createImageData: (w: number, h: number) => ({
      data: new Uint8ClampedArray(w * h * 4),
    }),
  };
  return new Proxy(target, {
    get: (o, k) => (k in o ? o[k] : noop),
  }) as unknown as CanvasRenderingContext2D;
}

export interface Inked {
  text: string;
  kind: InkKind | "fillText";
}

/**
 * Render the whole reel at `fps` on a recording context, calling `frame`
 * with each frame's time and every string drawn on it.
 */
export function inkFrames(
  facts: ReelFacts | null,
  fps: number,
  frame: (t: number, inked: readonly Inked[]) => void,
): number {
  // The finishing passes make their sprites on a canvas of their own.
  const g = globalThis as { document?: unknown };
  const had = g.document;
  g.document = {
    createElement: () => ({
      width: 0,
      height: 0,
      getContext: () => recordingContext(() => {}),
    }),
  };
  let cur: Inked[] = [];
  const prev = setInkSink((text, kind) => cur.push({ text, kind }));
  const frames = Math.round(DURATION * fps);
  try {
    const reel = composeReel(scenes, facts);
    const ctx = recordingContext((text) =>
      cur.push({ text, kind: "fillText" }),
    );
    for (let i = 0; i < frames; i++) {
      cur = [];
      const t = i / fps;
      reel.render(ctx, t, 1920, 1080);
      frame(t, cur);
    }
  } finally {
    setInkSink(prev);
    g.document = had;
  }
  return frames;
}
