import { fileURLToPath } from "node:url";
import { defineReleasesData } from "@jdxcode/docs-releases/data";
import type { ReleasesData } from "@jdxcode/docs-releases/data";

// Options are under "docs-releases" in the root package.json. The root is given
// explicitly, so a build run from docs/ finds them too.
export default defineReleasesData({
  root: fileURLToPath(new URL("..", import.meta.url)),
});

declare const data: ReleasesData;
export { data };
