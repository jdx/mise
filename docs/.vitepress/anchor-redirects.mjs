// Sections that moved off a page that still exists, from `/page.html#old-id`
// to `/new-page.html#new-id`. Append-only: published links and released mise
// binaries point at the old ids. The browser loads this file only when a page
// opens with a hash that matches no element on it (see theme/index.ts).

/** @type {Record<string, string>} */
export const anchorRedirects = {};
