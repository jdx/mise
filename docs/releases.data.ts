import { defineReleasesData } from "@jdx/docs-releases/data";
import type { ReleasesData } from "@jdx/docs-releases/data";

// Options are under "docs-releases" in the root package.json.
export default defineReleasesData();

declare const data: ReleasesData;
export { data };
