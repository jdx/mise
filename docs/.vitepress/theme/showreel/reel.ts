// from jdx/hk@37937824 docs/.vitepress/theme/showreel/reel.ts
// The whole reel: every section's scene, composited (compose.ts). The video
// renderer (showreel-video.mjs) imports it, and the tests; the landing page
// only plays the rendered file.

import type { ReelFacts } from "./bible";
import { composeReel, type Reel, type ReelOptions } from "./compose";
import { scenes } from "./scenes";
import { sec } from "./timeline";

export type { Reel, ReelOptions } from "./compose";

/**
 * The video's poster frame: `switch` at beat 18 (2:10), a beat after its
 * event plan lands the second version (sections.json `apiVersion`), where
 * both projects hold their own versions on their folder cards.
 */
export const POSTER_TIME = sec("switch").beat(18);

export function createReel(
  facts: ReelFacts | null,
  options: ReelOptions = {},
): Reel {
  return composeReel(scenes, facts, options);
}

// Re-exported for the video renderer.
export { FONTS } from "./fonts";
export { resetTypeCache } from "./type";
