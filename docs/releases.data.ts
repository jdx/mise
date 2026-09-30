import { defineReleasesData } from "@jdxcode/docs-releases/data";
import type { ReleasesData } from "@jdxcode/docs-releases/data";

// Options are under "docs-releases" in the root package.json.
export default defineReleasesData();

declare const data: ReleasesData;
export { data };
