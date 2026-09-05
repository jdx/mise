# Social previews

The docs build generates a 1200×630 PNG per page from its title and mise's
existing logo. The homepage uses a short product description. Images are
rendered locally with resvg and the bundled OFL-licensed Space Grotesk font;
no rendering service or system font is required. Image URLs include a content
hash so title, artwork, and font changes get a new URL.

`docs:build` tests text wrapping and PNG rendering, then checks the built HTML
for matching Open Graph/Twitter metadata and emitted image files. Edit
`social-images.mjs` to adjust colors or layout.
