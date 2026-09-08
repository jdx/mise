# Social previews

The docs build generates a 1200×630 PNG per page from its title and mise's
existing logo, with a short subtitle. The homepage uses a product headline. Images are
rendered locally with resvg and the bundled OFL-licensed Space Grotesk font;
no rendering service or system font is required. Image URLs include a content
hash of the final PNG bytes so title, artwork, font, and renderer changes get a
new URL.

Every Markdown page must provide a non-empty string in frontmatter `description`.
Missing, blank, or non-string descriptions fail the docs build with the page path.
There is no automatic fallback to page prose or the site description.
CLI generation writes this field from command help, with a dedicated summary for
the CLI index; do not edit generated CLI pages by hand. VitePress's built-in 404
page uses its built-in description and remains marked noindex.

The required `description` supplies HTML, Open Graph, Twitter, and structured
metadata. Set optional `socialDescription` when the
image needs a shorter editorial subtitle, for example:

```yaml
description: Install tools from signed release manifests with mise.
socialDescription: Signed releases and verified downloads.
```

Subtitles are limited to approximately 100 characters and two measured lines.
Titles use at most three lines, shrinking and truncating when necessary. Both
have reserved space separate from the logo and footer. Empty page summaries
fail the build even when a `socialDescription` is supplied.

`docs:build` tests text wrapping and PNG rendering, then checks the built HTML
for matching Open Graph/Twitter metadata and the exact image expected from each
page title and subtitle. Checks also cover description consistency, canonical
URLs, image type and dimensions, and the error page's noindex directive.
Regression tests reject swapped images and empty alt text. Edit
`social-images.mjs` to adjust colors or layout.
