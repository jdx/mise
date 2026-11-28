use crate::config::SettingsExt;
use crate::config::config_file::trust_check;
use crate::dirs;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::path_env::PathEnv;
use crate::tera::{
    TeraEngine, contains_template_syntax, get_tera, render_config_str, tera_exec, tera1_exec,
};
use crate::toolset::Toolset;
use eyre::{Context, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Config, Settings};
pub(crate) use mise_util::env_value::EnvValue;

mod file;
mod module;
pub use module::skipped_env_modules;
mod path;
pub mod prompt;
mod source;
pub(crate) mod venv;

pub use mise_config::env::{
    AgeFormat, EnvDirective, EnvDirectiveOptions, PathPosition, Rendered, RequiredValue,
};

/// Show env directive paths the way the rest of mise does.
pub(crate) fn register_hooks() {
    mise_config::env::set_display_path(|path| display_path(path));
}

#[derive(Default, Clone)]
pub struct EnvResults {
    pub env: IndexMap<String, (String, PathBuf)>,
    pub vars: IndexMap<String, (String, PathBuf)>,
    pub env_remove: BTreeSet<String>,
    pub env_files: Vec<PathBuf>,
    pub env_paths: Vec<PathBuf>,
    /// `_.path` entries declared with `position = "append"`: they go after tool paths
    pub env_paths_append: Vec<PathBuf>,
    pub env_scripts: Vec<PathBuf>,
    pub redactions: Vec<String>,
    pub redaction_exclusions: BTreeSet<String>,
    /// Keys whose effective value comes from the caller's environment rather than from
    /// [`Self::env`]: `required` directives validate without assigning, and `default`
    /// directives yield to a value the caller already set. Redaction resolves these keys
    /// against the caller environment; only names are recorded, never values.
    pub caller_env_keys: BTreeSet<String>,
    /// Keys whose `default` directive rendered, as opposed to yielding to a value already
    /// present. Names only.
    pub rendered_defaults: BTreeSet<String>,
    pub tool_add_paths: Vec<PathBuf>,
    /// Files to watch for cache invalidation (from modules and _.source directives)
    pub watch_files: Vec<PathBuf>,
    /// True if any directive declared cacheable=false, is a dynamic module, or produced a
    /// decrypted (age, sops) value, or a redacted one unless `env_cache_redacted` is on,
    /// none of which may be written to the env cache
    pub has_uncacheable: bool,
}

pub(super) struct EnvDirectiveContext<'a> {
    config: &'a Arc<Config>,
    tera_ctx: &'a mut tera::Context,
    tera: &'a mut Option<TeraEngine>,
    results: &'a mut EnvResults,
    normalize_path: fn(&Path, PathBuf) -> PathBuf,
    source: &'a Path,
    exec_env: &'a EnvMap,
    config_root: &'a Path,
    /// The caller's resolved toolset, when the caller has one. `Toolset::env` does; plain
    /// `Config::env` does not, because it runs before any toolset exists. Directives that need to
    /// know *which* version of a tool is active — rather than just rendering `tools.*` templates —
    /// read it from here, since CLI overrides such as `--tool python@3.12` only ever reach a
    /// toolset and are never written back to `Config`.
    toolset: Option<&'a Toolset>,
}

impl EnvDirectiveContext<'_> {
    fn parse_template(&mut self, origin: &str, input: &str) -> eyre::Result<String> {
        self.results.parse_template(
            self.tera_ctx,
            self.tera,
            self.source,
            self.exec_env,
            origin,
            input,
        )
    }

    fn normalize_path(&self, path: PathBuf) -> PathBuf {
        (self.normalize_path)(self.config_root, path)
    }

    /// Renders a `_.file` or `_.source` path and splits it where the glob the entry writes
    /// begins. Only a `*` the entry itself writes makes a pattern, and a template or `$VAR` can
    /// only name the directory the pattern is matched in: `{{cwd}}/.env.*` and
    /// `env.d/$PROFILE/*.env` are globs, and what the substitution renders is taken as a
    /// directory name, glob characters included. A substitution in the path component that
    /// holds the first written glob character, or after it, as in `env.d/$PROFILE*.env`, is an
    /// error (see [`SubstitutionMarks::split_glob`]).
    ///
    /// The pattern is found in the rendered path, so the branch of a `{% if %}` that renders is
    /// the one whose glob counts: `{% if x %}a/*.env{% else %}b/*.env{% endif %}` is a glob
    /// either way, and `{% if x %}*.env{% else %}.env{% endif %}` names `.env` when `x` is false.
    ///
    /// The template renders first, with the substitutions marked, and `$VAR`s expand in the
    /// directory once it is split off, so a `$` the entry writes before a template still forms
    /// a variable with what the template renders, as without a glob: `${{ 'D' }}/*.env` reads
    /// the directory `$D` names.
    fn parse_path_template(&mut self, origin: &str, input: &str) -> eyre::Result<EnvPath> {
        let shell_expand = Settings::get().env_shell_expand;
        if !writes_a_star(input, shell_expand) {
            let path = self.parse_template(origin, input)?;
            return Ok(EnvPath::literal(path));
        }
        let marks = SubstitutionMarks::new(shell_expand);
        let rendered = self.render_template(&marks.wrap_substitutions(input))?;
        let glob = marks
            .split_glob(&rendered)
            .wrap_err_with(|| format!("in {origin} = {input:?}"))?;
        let expand =
            |text: String| EnvResults::shell_expand(self.tera_ctx, self.source, origin, text);
        Ok(match glob {
            Some((dir, pattern)) => {
                let (dir, pattern) = (expand(dir), expand(pattern));
                EnvPath {
                    path: format!("{dir}{pattern}").into(),
                    dir: Some((dir.into(), pattern)),
                }
            }
            // the branch that rendered writes no `*`, or a template dropped a marker or
            // rendered a NUL
            None => EnvPath::literal(expand(marks.strip(&rendered))),
        })
    }

    /// [`Self::parse_template`] without the `$VAR` expansion.
    fn render_template(&mut self, input: &str) -> eyre::Result<String> {
        self.results
            .render_template(self.tera_ctx, self.tera, self.source, self.exec_env, input)
    }

    /// The files an [`EnvPath`] names: the path itself, or the files its pattern matches.
    fn expand_path(&self, path: &EnvPath) -> Vec<PathBuf> {
        match &path.dir {
            None => vec![self.normalize_path(path.path.clone())],
            Some((dir, pattern)) => glob_files_in(&self.normalize_path(dir.clone()), pattern),
        }
    }
}

/// Whether [`EnvResults::parse_template`] can rewrite `input`: it holds a template, or a `$VAR`
/// while `env_shell_expand` is on. A `$` that starts no variable, as in `cost$.env` or `$$`, is
/// written text, and so is every `$` while `env_shell_expand` is off.
///
/// The path such a `_.file` or `_.source` entry names is not fixed by the config that declares
/// it. `{{cwd}}/.env` and `$PWD/.env` name a different file in every directory.
fn is_templated(input: &str) -> bool {
    is_templated_with(input, Settings::get().env_shell_expand)
}

/// [`is_templated`], with `env_shell_expand` passed in.
fn is_templated_with(input: &str, shell_expand: bool) -> bool {
    contains_template_syntax(input)
        || template_pieces(input, shell_expand)
            .iter()
            .any(|(piece, _)| *piece != Piece::Written)
}

/// Starts each marker [`SubstitutionMarks`] puts around a substitution. No path holds a NUL.
const PATTERN_MARK: char = '\0';

/// A rendered `_.file` or `_.source` path.
pub(super) struct EnvPath {
    /// The whole rendered path.
    path: PathBuf,
    /// For a glob, the directory it is taken in, as named, and the pattern below it.
    dir: Option<(PathBuf, String)>,
}

impl EnvPath {
    fn literal(path: String) -> Self {
        Self {
            path: path.into(),
            dir: None,
        }
    }
}

/// What a stretch of a `_.file` or `_.source` entry is to the path it renders.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Piece {
    /// Text the entry writes, copied to the path.
    Written,
    /// A `{{ }}` expression or a `$VAR`: replaced by data.
    Substituted,
    /// A `{% %}` or `{# #}` tag, which outputs nothing itself.
    Tag,
}

/// Splits `input` into the [`Piece`]s it is made of, in order. An opener with no closer makes
/// the rest of `input` one substitution.
///
/// A piece spans exactly what the renderer replaces, so that markers put around it in
/// [`SubstitutionMarks::wrap_substitutions`] never split a token:
///
/// - A template tag runs to its closer, past any in its string literals, and also covers the
///   whitespace its `-` trims: before `{{-`, `{%-` or `{#-`, and after `-}}`, `-%}` or `-#}`.
/// - The `$` syntax is read as [`shell_expand_env`] reads it: `$$` is a written `$`, `${…}` runs
///   to the brace that closes it, nested fallbacks and template tags included, and `$NAME` to the
///   end of the name.
///   A `$` before anything else is written text, and so is a `$` before a template tag, which
///   renders first. With `shell_expand` off (`env_shell_expand`), every `$` is written text.
/// - The body of a `{% raw %}` block renders as it is written, so it is written text and no
///   template tag opens in it, as the other template scans read it ([`crate::tera::raw_block`]).
///   Its `$` syntax is still read, since `$VAR`s expand after the render. The `{% raw %}` and
///   `{% endraw %}` around it are tags.
fn template_pieces(input: &str, shell_expand: bool) -> Vec<(Piece, std::ops::Range<usize>)> {
    let bytes = input.as_bytes();
    let mut pieces = Vec::new();
    let mut text_start = 0;
    let mut i = 0;
    // Where the body of the raw block being read ends, while in one.
    let mut raw_end: Option<usize> = None;
    while i < bytes.len() {
        if raw_end.is_some_and(|end| i >= end) {
            raw_end = None;
        }
        let found = match bytes[i] {
            b'{' if raw_end.is_none() && is_tag_open(bytes, i) => {
                let piece = if bytes[i + 1] == b'{' {
                    Piece::Substituted
                } else {
                    Piece::Tag
                };
                let close = tag_close(bytes[i + 1]);
                let mut end = match crate::tera::raw_block(&input[i..]) {
                    Some(raw) => {
                        raw_end = Some(i + raw.body.end);
                        i + raw.body.start
                    }
                    None => template_tag_end(input, i),
                };
                let tag = &input[i..end];
                let mut start = i;
                if bytes.get(i + 2) == Some(&b'-') {
                    start = text_start + input[text_start..i].trim_end().len();
                }
                if tag.len() >= 6 && tag.ends_with(close) && tag[..tag.len() - 2].ends_with('-') {
                    end = input.len() - input[end..].trim_start().len();
                }
                Some((piece, start, end))
            }
            b'$' if shell_expand => match bytes.get(i + 1) {
                Some(b'$') => {
                    i += 2;
                    continue;
                }
                Some(b'{') if opens_braced_expr(bytes, i + 1, raw_end) => {
                    let end =
                        braced_expr_end(input, i + 2, raw_end).map_or(input.len(), |end| end + 1);
                    Some((Piece::Substituted, i, end))
                }
                Some(&b) if is_var_start(b.into()) => {
                    let mut end = i + 1;
                    while bytes.get(end).is_some_and(|&b| is_var_char(b.into())) {
                        end += 1;
                    }
                    Some((Piece::Substituted, i, end))
                }
                _ => None,
            },
            _ => None,
        };
        match found {
            Some((piece, start, end)) => {
                if text_start < start {
                    pieces.push((Piece::Written, text_start..start));
                }
                pieces.push((piece, start..end));
                i = end;
                text_start = end;
            }
            None => i += 1,
        }
    }
    if text_start < input.len() {
        pieces.push((Piece::Written, text_start..input.len()));
    }
    pieces
}

/// Whether a template tag (`{{`, `{%` or `{#`) opens at `at`.
fn is_tag_open(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&b'{') && matches!(bytes.get(at + 1), Some(b'{' | b'%' | b'#'))
}

/// Whether the `{` at `at`, after a `$`, makes a `${`: it does in the body of a raw block, which
/// ends at `raw_end`, and elsewhere when it opens no template tag. A `$` that ends a raw body is
/// followed by its `{% endraw %}`, so it comes before a tag and is written text, as the `$` in
/// `cost$*` is once rendered.
fn opens_braced_expr(bytes: &[u8], at: usize, raw_end: Option<usize>) -> bool {
    match raw_end {
        Some(end) if at < end => true,
        _ => !is_tag_open(bytes, at),
    }
}

/// The closer of the tag whose opener's second byte is `kind`.
fn tag_close(kind: u8) -> &'static str {
    match kind {
        b'{' => "}}",
        b'%' => "%}",
        _ => "#}",
    }
}

/// The end of the tag that opens at `at`, after its closer, as the template engine reads it: a
/// closer inside a string literal does not end it. The end of `input` when it has no closer.
fn template_tag_end(input: &str, at: usize) -> usize {
    let close = tag_close(input.as_bytes()[at + 1]);
    crate::tera::template_tag_end(&input[at..], close).map_or(input.len(), |end| at + end)
}

/// Where the `}` that closes a `${` whose body starts at `start` is, as [`read_braced_expr`]
/// finds it once the template has rendered: a template tag in the body, such as the fallback in
/// `${P:-{{ env.DEFAULT }}}`, renders first, so its braces do not close the expression. The
/// body of a `{% raw %}` block renders as it is written, so its braces do. `raw_end` is where the
/// body of the raw block that `start` is in ends, when it is in one.
fn braced_expr_end(input: &str, start: usize, mut raw_end: Option<usize>) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut depth = 1;
    let mut command_depth = 0;
    let mut i = start;
    while i < bytes.len() {
        if raw_end.is_some_and(|end| i >= end) {
            raw_end = None;
        }
        if raw_end.is_none() && is_tag_open(bytes, i) {
            match crate::tera::raw_block(&input[i..]) {
                Some(raw) => {
                    raw_end = Some(i + raw.body.end);
                    i += raw.body.start;
                }
                None => i = template_tag_end(input, i),
            }
            continue;
        }
        let next = bytes.get(i + 1);
        match bytes[i] {
            b'(' if command_depth > 0 => command_depth += 1,
            b')' if command_depth > 0 => command_depth -= 1,
            _ if command_depth > 0 => {}
            b'$' if next == Some(&b'{') && opens_braced_expr(bytes, i + 1, raw_end) => {
                depth += 1;
                i += 1;
            }
            b'$' if next == Some(&b'(') => {
                command_depth = 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Whether the entry writes a `*`, which can make it a pattern. Template tags and `$VAR`s are
/// skipped: what they render is data.
fn writes_a_star(input: &str, shell_expand: bool) -> bool {
    template_pieces(input, shell_expand)
        .into_iter()
        .any(|(piece, range)| piece == Piece::Written && input[range].contains('*'))
}

/// Markers that keep track, through rendering, of the text each substitution in a pattern put
/// there. They hold a random number, so substituted data cannot forge them, and a NUL, which no
/// path holds.
struct SubstitutionMarks {
    open: String,
    close: String,
    /// Put between a written `*` and a `{% %}` or `{# #}` tag next to it, so that the `*`s on
    /// either side of a tag that renders nothing stay two runs.
    boundary: String,
    /// Whether a `$VAR` is a substitution (`env_shell_expand`).
    shell_expand: bool,
}

impl SubstitutionMarks {
    fn new(shell_expand: bool) -> Self {
        let nonce = ::rand::random::<u64>();
        Self {
            open: format!("{PATTERN_MARK}{nonce}<"),
            close: format!("{PATTERN_MARK}{nonce}>"),
            boundary: format!("{PATTERN_MARK}{nonce}|"),
            shell_expand,
        }
    }

    /// `pattern` with each substitution wrapped in the markers, and a boundary marker between
    /// each tag and a written `*` next to it.
    fn wrap_substitutions(&self, pattern: &str) -> String {
        let pieces = template_pieces(pattern, self.shell_expand);
        let mut out = String::with_capacity(pattern.len());
        for (i, (piece, range)) in pieces.iter().enumerate() {
            let text = &pattern[range.clone()];
            match piece {
                Piece::Substituted => {
                    out.push_str(&self.open);
                    out.push_str(text);
                    out.push_str(&self.close);
                }
                Piece::Written => out.push_str(text),
                Piece::Tag => {
                    if out.ends_with('*') {
                        out.push_str(&self.boundary);
                    }
                    out.push_str(text);
                    if let Some((Piece::Written, next)) = pieces.get(i + 1)
                        && pattern[next.clone()].starts_with('*')
                    {
                        out.push_str(&self.boundary);
                    }
                }
            }
        }
        out
    }

    /// `text` without boundary markers.
    fn without_boundaries(&self, text: &str) -> String {
        text.replace(&self.boundary, "")
    }

    /// The written and substituted (`true`) stretches of a rendered, marked pattern. `None` when
    /// the markers do not pair up or a NUL is left over.
    fn segments<'a>(&self, rendered: &'a str) -> Option<Vec<(bool, &'a str)>> {
        let mut segments = vec![];
        let mut rest = rendered;
        while let Some(open) = rest.find(&self.open) {
            let written = &rest[..open];
            let after = &rest[open + self.open.len()..];
            let close = after.find(&self.close)?;
            let data = &after[..close];
            if self.without_boundaries(written).contains(PATTERN_MARK)
                || data.contains(PATTERN_MARK)
            {
                return None;
            }
            segments.push((false, written));
            segments.push((true, data));
            rest = &after[close + self.close.len()..];
        }
        if self.without_boundaries(rest).contains(PATTERN_MARK) {
            return None;
        }
        segments.push((false, rest));
        Some(segments)
    }

    /// The directory a rendered, marked path is globbed in, as a literal path, and the glob
    /// below it, when the text the entry writes holds a `*`.
    ///
    /// The glob starts at the path component that holds the first glob character the entry
    /// writes (`*`, `?`, `[` or `{`, or a `\` escape outside Windows), and the directory is
    /// everything ahead of it. Only a separator the entry writes ends a component, so where
    /// the glob starts never depends on what a substitution renders. A substitution can name
    /// the directory, and what it renders there is a name, glob characters and separators
    /// included; a substitution in the glob, even one that renders nothing, is an error, so
    /// the glob holds only text the entry writes.
    ///
    /// `None` when the written text holds no `*`, or the markers do not pair up or a NUL is
    /// left over.
    fn split_glob(&self, rendered: &str) -> eyre::Result<Option<(String, String)>> {
        let Some(segments) = self.segments(rendered) else {
            return Ok(None);
        };
        if !segments.iter().any(|(data, t)| !data && t.contains('*')) {
            return Ok(None);
        }
        let glob_chars: &[char] = if cfg!(windows) {
            &['*', '?', '[', '{']
        } else {
            &['*', '?', '[', '{', '\\']
        };
        let is_sep = |c: char| c == '/' || (cfg!(windows) && c == '\\');
        // A Windows prefix, as the `?` in `\\?\C:\`, holds no glob.
        let prefix = match Path::new(segments[0].1).components().next() {
            Some(std::path::Component::Prefix(prefix)) => prefix.as_os_str().len(),
            _ => 0,
        };
        // The segment, always a written one, and the offset in it where the glob starts: just
        // past the last written separator ahead of the first written glob character.
        let mut start = (0, prefix);
        for (i, (data, text)) in segments.iter().enumerate() {
            if *data {
                continue;
            }
            let skip = if i == 0 { prefix } else { 0 };
            let glob_char = text[skip..].find(glob_chars).map(|at| skip + at);
            if let Some(at) = text[skip..glob_char.unwrap_or(text.len())].rfind(is_sep) {
                start = (i, skip + at + 1);
            }
            if glob_char.is_some() {
                break;
            }
        }
        let (i, at) = start;
        let dir: String = segments[..i]
            .iter()
            .map(|(_, t)| *t)
            .chain([&segments[i].1[..at]])
            .collect();
        let dir = self.without_boundaries(&dir);
        let pattern: String = [&segments[i].1[at..]]
            .into_iter()
            .chain(segments[i + 1..].iter().map(|(_, t)| *t))
            .collect();
        // A `$VAR` that tags joined, as `${% if x %}P{% endif %}` does, is a substitution too.
        if segments[i..].iter().any(|(data, _)| *data)
            || (self.shell_expand && has_shell_substitution(&pattern))
        {
            let start = if dir.is_empty() {
                "at the start of the path".to_string()
            } else {
                format!("after {dir:?}")
            };
            eyre::bail!(
                "a template or $VAR is part of the glob, which starts {start}. A substitution \
                 can name the directory a glob is matched in, but not be part of the glob: move \
                 it ahead of the last `/` before the first wildcard, as in \
                 `env.d/{{{{ env.PROFILE }}}}/*.env`, or write the glob out in full"
            );
        }
        Ok(Some((dir, self.join_star_runs_across_tags(&pattern))))
    }

    /// `pattern` without its boundary markers, and with each run of `*`s that a tag rendering
    /// nothing joined, as in `*{% if x %}a{% endif %}*`, made what [`join_star_runs`] makes it:
    /// the `**`s written in it, or one `*` when there are none. So joining never forms a `**`
    /// that descends.
    fn join_star_runs_across_tags(&self, pattern: &str) -> String {
        #[derive(Default)]
        struct Run {
            /// The `*`s of the run since the last boundary.
            current: usize,
            /// All the `*`s of the run.
            stars: usize,
            /// The longest written `**` (or longer run) in the run. Two that a tag joins are
            /// one, so `**{% if x %}/dev/{% endif %}**` with `x` false is `**`, not `****`,
            /// which the walk would not read as recursive.
            written: usize,
        }
        impl Run {
            fn cross(&mut self) {
                if self.current > 1 {
                    self.written = self.written.max(self.current);
                }
                self.current = 0;
            }
            fn flush(&mut self, out: &mut String) {
                self.cross();
                let n = if self.written > 0 {
                    self.written
                } else {
                    self.stars.min(1)
                };
                out.extend(std::iter::repeat_n('*', n));
                *self = Self::default();
            }
        }
        let mut out = String::with_capacity(pattern.len());
        let mut run = Run::default();
        let mut escaping = false;
        let mut rest = pattern;
        while let Some(c) = rest.chars().next() {
            if let Some(after) = rest.strip_prefix(&self.boundary) {
                run.cross();
                rest = after;
                continue;
            }
            rest = &rest[c.len_utf8()..];
            if c == '*' && !escaping {
                run.current += 1;
                run.stars += 1;
            } else {
                run.flush(&mut out);
                out.push(c);
            }
            escaping = !escaping && c == '\\' && !cfg!(windows);
        }
        run.flush(&mut out);
        out
    }

    /// `rendered` without the markers or a NUL.
    fn strip(&self, rendered: &str) -> String {
        rendered
            .replace(&self.open, "")
            .replace(&self.close, "")
            .replace(&self.boundary, "")
            .replace(PATTERN_MARK, "")
    }

    /// `text` without any markers, whatever their number.
    fn strip_any(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(PATTERN_MARK) {
            out.push_str(&rest[..at]);
            let after = rest[at + 1..].trim_start_matches(|c: char| c.is_ascii_digit());
            rest = after.strip_prefix(['<', '>', '|']).unwrap_or(after);
        }
        out.push_str(rest);
        out
    }
}

/// The files `pattern` matches below `dir`, in name order. As in a shell, `*` stays within one
/// directory and only `**` descends, so `.env.*` does not reach into a repository cloned below
/// the config. `dir` is taken as it is named. The alternatives of a `{a,b}` group are matched
/// together, so their files are in name order too, not in the order the group lists them.
///
/// Only a `**` the pattern writes descends. Expanding a group can put a `*` next to another, as
/// `*{*,dev}` does, and each run of `*`s it forms is one `*`.
fn glob_files_in(dir: &Path, pattern: &str) -> Vec<PathBuf> {
    use crate::task::task_source_checker::{expand_glob_braces, expand_trailing_globstar};
    let shielded = mark_written_star_runs(pattern);
    let alternatives = expand_glob_braces(&shielded)
        .unwrap_or_else(|_| vec![shielded.clone()])
        .into_iter()
        .map(|alternative| expand_trailing_globstar(&join_star_runs(&alternative)));
    let mut files = Vec::new();
    for alternative in alternatives {
        let Ok(walker) = crate::task::task_source_checker::glob_walk_in(dir, &alternative) else {
            continue;
        };
        files.extend(
            walker
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_file())
                .map(|entry| entry.into_path()),
        );
    }
    files.sort();
    files.dedup();
    files
}

/// Calls `each` with every character of a glob and whether it is an unescaped `*` wildcard. A
/// `\` escapes the next character outside Windows.
fn scan_stars(glob: &str, mut each: impl FnMut(char, bool)) {
    let mut escaping = false;
    for c in glob.chars() {
        each(c, c == '*' && !escaping);
        escaping = !escaping && c == '\\' && !cfg!(windows);
    }
}

/// `pattern` with each `*` of a run of two or more written `*`s replaced by [`PATTERN_MARK`],
/// which no glob [`SubstitutionMarks::split_glob`] returns holds, so that [`join_star_runs`]
/// can tell them from the runs brace expansion forms.
fn mark_written_star_runs(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut run = 0;
    let flush = |out: &mut String, run: &mut usize| {
        let star = if *run > 1 { PATTERN_MARK } else { '*' };
        out.extend(std::iter::repeat_n(star, *run));
        *run = 0;
    };
    scan_stars(pattern, |c, star| {
        if star {
            run += 1;
        } else {
            flush(&mut out, &mut run);
            out.push(c);
        }
    });
    flush(&mut out, &mut run);
    out
}

/// A brace-expanded alternative of a [`mark_written_star_runs`] pattern, with each run of `*`s
/// that holds no written `**` made one `*`, and each that does made the run the pattern wrote.
fn join_star_runs(alternative: &str) -> String {
    let mut out = String::with_capacity(alternative.len());
    let (mut stars, mut written) = (0, 0);
    let flush = |out: &mut String, stars: &mut usize, written: &mut usize| {
        let n = if *written > 0 {
            *written
        } else {
            (*stars).min(1)
        };
        out.extend(std::iter::repeat_n('*', n));
        (*stars, *written) = (0, 0);
    };
    scan_stars(alternative, |c, star| {
        if c == PATTERN_MARK {
            written += 1;
        } else if star {
            stars += 1;
        } else {
            flush(&mut out, &mut stars, &mut written);
            out.push(c);
        }
    });
    flush(&mut out, &mut stars, &mut written);
    out
}

#[derive(Debug, Clone, Default)]
pub(crate) enum ToolsFilter {
    ToolsOnly,
    #[default]
    NonToolsOnly,
    Both,
    /// `tools = true` directives, but only plain `Val` (`KEY = value`) entries.
    /// Env *modules* (PythonVenv/Module/Source/File/Path) are skipped because they
    /// may reference tools outside a partial (dependency) toolset and would error.
    /// Used by `dependency_env` so a dependent tool's install sees `tools = true`
    /// value vars like `CLOUDSDK_PYTHON = "{{ tools.python.path }}/..."`. (#10282)
    ToolsOnlyVals,
}

static OCI_ENV_SATISFIES_REQUIRED: AtomicBool = AtomicBool::new(false);

/// Let `[oci.env]` satisfy `required` env vars for the rest of this process.
///
/// `mise oci` bakes `[oci.env]` into the image, so a `required` var it defines
/// has a value in the image and needn't exist on the build host. Set before the
/// config loads: `[env]` is validated as part of loading it.
pub fn oci_env_satisfies_required() {
    OCI_ENV_SATISFIES_REQUIRED.store(true, Ordering::Relaxed);
}

/// Whether [`oci_env_satisfies_required`] was called. Env resolved this way
/// isn't valid for a command that enforces `required`, so it must not be cached.
pub(crate) fn is_oci_env_satisfying_required() -> bool {
    OCI_ENV_SATISFIES_REQUIRED.load(Ordering::Relaxed)
}

/// Keys the project's `[oci.env]` sections define. Global and system configs
/// are left out: `mise oci` doesn't bake their `[oci]` unless asked to.
fn oci_env_keys(config: &Config) -> BTreeSet<String> {
    if !is_oci_env_satisfying_required() {
        return BTreeSet::new();
    }
    config
        .config_files
        .values()
        .filter(|cf| cf.project_root().is_some())
        .filter_map(|cf| cf.oci_config())
        .flat_map(|oci| oci.env.into_keys())
        .collect()
}

pub(crate) struct EnvResolveOptions {
    pub vars: bool,
    pub tools: ToolsFilter,
    pub warn_on_missing_required: bool,
}

impl EnvResults {
    fn track_redaction_override(&mut self, key: &str, redact: Option<bool>) {
        match redact {
            Some(false) => {
                self.redaction_exclusions.insert(key.to_string());
            }
            Some(true) | None => {
                self.redaction_exclusions.remove(key);
            }
        }
    }

    /// Value map for [`crate::config::Config::add_redactions_excluding`]: the values this
    /// resolution assigned, plus caller-supplied values for [`Self::caller_env_keys`], which
    /// are absent from [`Self::env`] because those directives never assign.
    pub(crate) fn redactable_env(&self, caller_env: &EnvMap) -> EnvMap {
        let mut out: EnvMap = self
            .env
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect();
        for k in &self.caller_env_keys {
            if let Some(v) = caller_env.get(k) {
                out.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        out
    }

    pub(crate) async fn resolve(
        config: &Arc<Config>,
        ctx: tera::Context,
        initial: &EnvMap,
        input: Vec<(EnvDirective, PathBuf)>,
        resolve_opts: EnvResolveOptions,
    ) -> eyre::Result<Self> {
        Self::resolve_with_toolset(config, ctx, initial, input, resolve_opts, None).await
    }

    /// [`Self::resolve`] for callers that already have a resolved toolset. See
    /// [`EnvDirectiveContext::toolset`] for why a directive would want it.
    pub(crate) async fn resolve_with_toolset(
        config: &Arc<Config>,
        mut ctx: tera::Context,
        initial: &EnvMap,
        input: Vec<(EnvDirective, PathBuf)>,
        resolve_opts: EnvResolveOptions,
        toolset: Option<&Toolset>,
    ) -> eyre::Result<Self> {
        // trace!("resolve: input: {:#?}", &input);
        let mut env = initial
            .iter()
            .map(|(k, v)| (k.clone(), (v.clone(), None)))
            .collect::<IndexMap<_, _>>();
        let mut r = Self::default();
        let normalize_path = |config_root: &Path, p: PathBuf| {
            let p = p.strip_prefix("./").unwrap_or(&p);
            match p.strip_prefix("~/") {
                Ok(p) => dirs::HOME.join(p),
                _ if p.is_relative() => config_root.join(p),
                _ => p.to_path_buf(),
            }
        };
        let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut append_paths: Vec<(PathBuf, PathBuf)> = Vec::new();
        let safe_mode = Settings::safe_mode();
        // In safe mode, environment directives from project (non-global) config are
        // ignored — they would otherwise be applied to the host environment and to
        // subprocesses mise spawns during resolution (e.g. `go list`, vfox hooks),
        // an indirect code-execution vector (PATH, LD_PRELOAD, DYLD_INSERT_LIBRARIES,
        // NODE_OPTIONS, ...) that safe mode is meant to close. Global/system config
        // is operator-owned and still applies, mirroring the trust model. `_.source`
        // runs a shell script (code execution rather than env injection), so it is
        // ignored regardless of source, including operator-owned global config.
        let dropped_in_safe_mode = |directive: &EnvDirective, source: &Path| {
            safe_mode && dropped_in_safe_mode(directive, source)
        };
        // Choose the last venv among directives that survive the safe-mode filter,
        // so a dropped project venv can't cause an operator's global venv to be
        // skipped as "not the last one".
        let last_python_venv = input.iter().rev().find_map(|(d, source)| match d {
            EnvDirective::PythonVenv { .. } if !dropped_in_safe_mode(d, source) => Some(d),
            _ => None,
        });
        let filtered_input = input
            .iter()
            .fold(Vec::new(), |mut acc, (directive, source)| {
                if dropped_in_safe_mode(directive, source) {
                    return acc;
                }
                // Filter directives based on tools setting
                let should_include = match &resolve_opts.tools {
                    ToolsFilter::ToolsOnly => directive.options().tools,
                    ToolsFilter::NonToolsOnly => !directive.options().tools,
                    ToolsFilter::Both => true,
                    ToolsFilter::ToolsOnlyVals => {
                        directive.options().tools && matches!(directive, EnvDirective::Val(..))
                    }
                };

                if !should_include {
                    return acc;
                }

                if let Some(d) = &last_python_venv
                    && matches!(directive, EnvDirective::PythonVenv { .. })
                    && **d != *directive
                {
                    // skip venv directives if it's not the last one
                    return acc;
                }
                acc.push((directive.clone(), source.clone()));
                acc
            });

        // Save filtered_input for validation after processing
        let filtered_input_for_validation = filtered_input.clone();
        let required_env = if resolve_opts.vars {
            &*env::PRISTINE_ENV
        } else {
            initial
        };

        if resolve_opts.vars {
            // A saved value is an ordinary var, below every config value and visible to
            // templates from the start. A name the process environment sets is skipped,
            // because the environment ranks higher.
            for (key, value) in prompt::saved_all() {
                if !value.is_empty() && env::PRISTINE_ENV.get(&key).is_none_or(|v| v.is_empty()) {
                    r.vars.insert(key, (value, prompt::answers_path()));
                }
            }
        }

        for (directive, source) in filtered_input {
            let mut tera = None;
            // trace!(
            //     "resolve: directive: {:?}, source: {:?}",
            //     &directive,
            //     &source
            // );
            let config_root = crate::config::config_file::config_root::config_root(&source);
            ctx.insert("cwd", &dirs::CWD.as_ref().map(|p| p.to_string_lossy()));
            ctx.insert("config_root", &config_root.to_string_lossy());
            ctx.insert(
                "config_source",
                &crate::config::config_file::config_root::config_source(&source),
            );
            let env_vars = env
                .iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect::<EnvMap>();
            ctx.insert("env", &env_vars);

            let context_vars = Self::context_vars(&ctx);

            let mut vars = context_vars.clone();
            vars.extend(r.vars.iter().map(|(k, (v, _))| (k.clone(), v.clone())));

            ctx.insert("vars", &vars);
            let redact = directive.options().redact;
            if !resolve_opts.vars && directive.options().prompt.is_some() {
                eyre::bail!("`prompt` is only supported in [vars], not [env]");
            }
            if resolve_opts.vars
                && directive.options().prompt.is_some()
                && !matches!(
                    directive,
                    EnvDirective::Default(..) | EnvDirective::Required(..)
                )
            {
                eyre::bail!("`prompt` needs a `default` or `required` on the same var");
            }
            // trace!("resolve: ctx.get('env'): {:#?}", &ctx.get("env"));
            match directive {
                EnvDirective::Val(k, v, opts) => {
                    // `[vars]` are template variables rather than environment variables, so the
                    // PATH rule does not apply to them.
                    let k = if resolve_opts.vars {
                        k
                    } else {
                        crate::env::normalize_path_key(k)
                    };
                    r.track_redaction_override(&k, redact);
                    // A value another task handed down is data: its templates were rendered
                    // where it was written, and rendering it again would evaluate the text that
                    // pass substituted.
                    let v = match opts.rendered {
                        Rendered::No => {
                            r.parse_template(&ctx, &mut tera, &source, &env_vars, &k, &v)?
                        }
                        Rendered::Templates => Self::shell_expand(&ctx, &source, &k, v),
                        Rendered::Final => v,
                    };

                    if resolve_opts.vars {
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        r.vars.insert(k, (v, source.clone()));
                    } else {
                        r.env_remove.remove(&k);
                        // trace!("resolve: inserting {:?}={:?} from {:?}", &k, &v, &source);
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        env.insert(k, (v, Some(source.clone())));
                    }
                }
                EnvDirective::Default(k, v, opts) => {
                    if resolve_opts.vars && opts.prompt.is_some() {
                        prompt::note_declared(&k);
                    }
                    // Same fold as `Val` above, and for the same reason.
                    let k = if resolve_opts.vars {
                        k
                    } else {
                        crate::env::normalize_path_key(k)
                    };
                    if resolve_opts.vars {
                        if r.vars.get(&k).is_some_and(|(v, _)| !v.is_empty()) {
                            // The value stays with the file (or the saved answer) that set it.
                            if redact.unwrap_or(false) {
                                r.redactions.push(k.clone());
                            }
                            continue;
                        }
                        if let Some(v) = env::PRISTINE_ENV.get(&k).filter(|v| !v.is_empty()) {
                            if redact.unwrap_or(false) {
                                r.redactions.push(k.clone());
                            }
                            r.vars.insert(k, (v.clone(), source.clone()));
                            continue;
                        }
                    } else if env.get(&k).is_some_and(|(v, _)| !v.is_empty()) {
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        r.caller_env_keys.insert(k.clone());
                        continue;
                    }

                    // A saved answer wins over the default, so render the default only
                    // when there is none.
                    if resolve_opts.vars
                        && let Some(a) = prompt::saved(&k)
                    {
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        r.track_redaction_override(&k, redact);
                        r.vars.insert(k, (a, prompt::answers_path()));
                        continue;
                    }

                    r.track_redaction_override(&k, redact);
                    r.rendered_defaults.insert(k.clone());
                    let v = r.parse_template(&ctx, &mut tera, &source, &env_vars, &k, &v)?;

                    if resolve_opts.vars {
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        let answer = match &opts.prompt {
                            Some(p) => prompt::answer(&k, p, Some(&v))?,
                            None => None,
                        };
                        match answer {
                            Some(a) => r.vars.insert(k, (a, prompt::answers_path())),
                            None => r.vars.insert(k, (v, source.clone())),
                        };
                    } else {
                        r.env_remove.remove(&k);
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        env.insert(k, (v, Some(source.clone())));
                    }
                }
                EnvDirective::Rm(k, _opts) => {
                    // The same fold as `Val`, so that `_.unset` naming another spelling of PATH
                    // reaches the key `Val` stored it under rather than missing silently.
                    let k = if resolve_opts.vars {
                        k
                    } else {
                        crate::env::normalize_path_key(k)
                    };
                    env.shift_remove(&k);
                    r.redaction_exclusions.remove(&k);
                    // The key is gone from the environment, so its caller value is no longer
                    // reachable by the task. Leaving it here would let `redactable_env` resolve
                    // it anyway and register a pattern that rewrites unrelated output.
                    r.caller_env_keys.remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Required(k, opts) => {
                    if resolve_opts.vars && opts.prompt.is_some() {
                        prompt::note_declared(&k);
                    }
                    // Required directives only validate; they never assign. Record the key so
                    // redaction can resolve it against the caller environment.
                    r.caller_env_keys.insert(k.clone());
                    // A directive that assigns nothing expresses no redaction opinion unless
                    // `redact` is set explicitly, so an exclusion from an earlier assignment
                    // survives a later bare `required = true`.
                    if redact.is_some() {
                        r.track_redaction_override(&k, redact);
                    }
                    if redact == Some(true) {
                        r.redactions.push(k.clone());
                    }
                    // Var required directives also surface process environment values so
                    // `{{vars.KEY}}` can render.
                    if resolve_opts.vars
                        && !vars.contains_key(&k)
                        && let Some(v) = required_env.get(&k)
                    {
                        r.vars.insert(k, (v.clone(), source.clone()));
                    } else if resolve_opts.vars
                        && !vars.contains_key(&k)
                        && !r.vars.contains_key(&k)
                        && let Some(a) = match &opts.prompt {
                            Some(p) => prompt::answer(&k, p, None)?,
                            None => prompt::saved(&k),
                        }
                    {
                        r.vars.insert(k, (a, prompt::answers_path()));
                    }
                }
                EnvDirective::Age {
                    key: ref k,
                    ref options,
                    ..
                } => {
                    r.track_redaction_override(k, options.redact);
                    // Decrypt age-encrypted value
                    let res = crate::agecrypt::decrypt_age_directive(&directive).await;
                    let decrypted_v = match res {
                        Ok(decrypted_v) => {
                            // Parse as template after decryption
                            r.parse_template(&ctx, &mut tera, &source, &env_vars, k, &decrypted_v)?
                        }
                        Err(e) if Settings::get().age.strict => {
                            return Err(e)
                                .wrap_err(eyre!("[experimental] Failed to decrypt {}", k));
                        }
                        Err(e) => {
                            debug!(
                                "[experimental] Age decryption failed for {} but continuing in non-strict mode: {}",
                                k, e
                            );
                            // continue to the next directive
                            continue;
                        }
                    };

                    // Decrypted, so never env-cached, even when `redact = false`.
                    r.has_uncacheable = true;

                    if resolve_opts.vars {
                        match options.redact {
                            Some(false) => {}
                            Some(true) | None => r.redactions.push(k.clone()),
                        }
                        r.vars.insert(k.clone(), (decrypted_v, source.clone()));
                    } else {
                        r.env_remove.remove(k);
                        // Handle redaction for age-encrypted values
                        // We're already in the EnvDirective::Age match arm, so we know this is an Age directive

                        // For age-encrypted values, we default to redacting for security
                        // With nullable redact, we can now distinguish between:
                        // - None: not specified (default for age is to redact for security)
                        // - Some(true): explicitly redact
                        // - Some(false): explicitly don't redact
                        debug!("Age directive {}: redact = {:?}", k, options.redact);
                        match options.redact {
                            Some(false) => {
                                // User explicitly set redact = false - don't redact
                                debug!(
                                    "Age directive {}: NOT redacting (explicit redact = false)",
                                    k
                                );
                            }
                            Some(true) | None => {
                                // Either explicitly redact or use age default (redact for security)
                                debug!(
                                    "Age directive {}: redacting (redact = {:?})",
                                    k, options.redact
                                );
                                r.redactions.push(k.clone());
                            }
                        }
                        env.insert(k.clone(), (decrypted_v, Some(source.clone())));
                    }
                }
                EnvDirective::Path(input_str, opts) => {
                    let mut directive_ctx = EnvDirectiveContext {
                        config,
                        tera_ctx: &mut ctx,
                        tera: &mut tera,
                        results: &mut r,
                        normalize_path,
                        source: &source,
                        exec_env: &env_vars,
                        config_root: &config_root,
                        toolset,
                    };
                    let path = Self::path(&mut directive_ctx, input_str).await?;
                    match opts.position {
                        PathPosition::Prepend => paths.push((path.clone(), source.clone())),
                        PathPosition::Append => append_paths.push((path.clone(), source.clone())),
                    }
                    // Don't modify PATH in env - just add to env_paths
                    // This allows consumers to control PATH ordering
                }
                directive @ (EnvDirective::File(..) | EnvDirective::FileGroup(..)) => {
                    let (inputs, opts) = match directive {
                        EnvDirective::File(input, opts) => (vec![input], opts),
                        EnvDirective::FileGroup(inputs, opts) => (inputs, opts),
                        _ => unreachable!(),
                    };
                    let mut directive_ctx = EnvDirectiveContext {
                        config,
                        tera_ctx: &mut ctx,
                        tera: &mut tera,
                        results: &mut r,
                        normalize_path,
                        source: &source,
                        exec_env: &env_vars,
                        config_root: &config_root,
                        toolset,
                    };
                    let files =
                        Self::file(&mut directive_ctx, inputs, opts.expand, opts.find_up).await?;
                    for (f, new_env) in files {
                        r.env_files.push(f.clone());
                        for (k, v) in new_env {
                            r.track_redaction_override(&k, redact);
                            if resolve_opts.vars {
                                if redact.unwrap_or(false) {
                                    r.redactions.push(k.clone());
                                }
                                r.vars.insert(k, (v, f.clone()));
                            } else {
                                if redact.unwrap_or(false) {
                                    r.redactions.push(k.clone());
                                }
                                env.insert(k, (v, Some(f.clone())));
                            }
                        }
                    }
                }
                EnvDirective::Source(input, _opts) => {
                    let mut directive_ctx = EnvDirectiveContext {
                        config,
                        tera_ctx: &mut ctx,
                        tera: &mut tera,
                        results: &mut r,
                        normalize_path,
                        source: &source,
                        exec_env: &env_vars,
                        config_root: &config_root,
                        toolset,
                    };
                    let files = Self::source(&mut directive_ctx, &mut paths, input)?;
                    for (f, new_env) in files {
                        r.env_scripts.push(f.clone());
                        for (k, v) in new_env {
                            r.track_redaction_override(&k, redact);
                            if resolve_opts.vars {
                                if redact.unwrap_or(false) {
                                    r.redactions.push(k.clone());
                                }
                                r.vars.insert(k, (v, f.clone()));
                            } else {
                                if redact.unwrap_or(false) {
                                    r.redactions.push(k.clone());
                                }
                                env.insert(k, (v, Some(f.clone())));
                            }
                        }
                    }
                }
                EnvDirective::PythonVenv {
                    path,
                    create,
                    python,
                    uv_create_args,
                    python_create_args,
                    options: _opts,
                } => {
                    let mut directive_ctx = EnvDirectiveContext {
                        config,
                        tera_ctx: &mut ctx,
                        tera: &mut tera,
                        results: &mut r,
                        normalize_path,
                        source: &source,
                        exec_env: &env_vars,
                        config_root: &config_root,
                        toolset,
                    };
                    Self::venv(
                        &mut directive_ctx,
                        &mut env,
                        path,
                        create,
                        venv::PythonVenvOptions {
                            python,
                            // filled in by `venv()` from the caller's toolset
                            active_python: None,
                            uv_create_args,
                            python_create_args,
                            require_uv: false,
                        },
                    )
                    .await?;
                }
                EnvDirective::Module(name, value, _opts) => {
                    let mut env_map: IndexMap<String, String> = env
                        .iter()
                        .map(|(k, (v, _))| (k.clone(), v.clone()))
                        .collect();
                    // Incorporate _.path entries accumulated so far into PATH
                    // so that cmd.exec in the plugin can find tools on PATH.
                    if !paths.is_empty() || !append_paths.is_empty() {
                        let existing_path =
                            env_map.get(&*env::PATH_KEY).cloned().unwrap_or_default();
                        let mut path_env = PathEnv::from_path_str(&existing_path);
                        let resolve = |(p, path_source): &(PathBuf, PathBuf)| {
                            let config_root =
                                crate::config::config_file::config_root::config_root(path_source);
                            env::split_paths(p)
                                .map(|s| normalize_path(&config_root, s))
                                .collect_vec()
                        };
                        for entry in paths.iter().flat_map(resolve) {
                            path_env.add(entry);
                        }
                        // A `tools = true` module already has the tool paths in PATH, and
                        // appended entries belong behind them, not ahead.
                        let tool_paths = match toolset {
                            Some(ts) => ts.list_paths(config).await,
                            None => vec![],
                        };
                        let tools_in_path =
                            env::split_paths(&existing_path).any(|e| tool_paths.contains(&e));
                        let mut appended = vec![];
                        for entry in append_paths.iter().flat_map(resolve) {
                            if tools_in_path {
                                appended.push(entry);
                            } else {
                                path_env.add(entry);
                            }
                        }
                        let mut entries = env::split_paths(&path_env.to_string()).collect_vec();
                        if let Some(i) = entries.iter().rposition(|e| tool_paths.contains(e)) {
                            entries.splice(i + 1..i + 1, appended);
                        }
                        env_map.insert(
                            env::PATH_KEY.to_string(),
                            env::join_paths(entries)?.to_string_lossy().to_string(),
                        );
                    }
                    if log::log_enabled!(log::Level::Trace) {
                        if let Some(path) = env_map.get(&*env::PATH_KEY) {
                            trace!("module {name}: PATH={path}");
                        } else {
                            trace!("module {name}: no PATH in env_map");
                        }
                    }
                    let env_before: IndexMap<String, (String, PathBuf)> = r.env.clone();
                    Self::module(&mut r, config, source, name, &value, redact, env_map).await?;
                    // Merge entries that this module call added or changed into
                    // the local `env` so they are visible in the Tera context
                    // for subsequent directives.  Keys unchanged in `r.env`
                    // (same value before and after this call) are skipped, which
                    // preserves any Val/File/Source override in `env` applied
                    // after a prior module emitted the same value.  When a
                    // module emits a *different* value the merge writes it
                    // through — "later directive wins", consistent with all
                    // other directive pairs.
                    for (k, (v, src)) in &r.env {
                        let added_or_changed = match env_before.get(k) {
                            Some((old_v, _)) => old_v != v,
                            None => true,
                        };
                        if added_or_changed {
                            env.insert(k.clone(), (v.clone(), Some(src.clone())));
                        }
                    }
                }
            };
        }
        let env_vars = env
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>();
        ctx.insert("env", &env_vars);
        for (k, (v, source)) in env {
            if let Some(source) = source {
                r.env.insert(k, (v, source));
            }
        }
        // trace!("resolve: paths: {:#?}", &paths);
        // trace!("resolve: ctx.env: {:#?}", &ctx.get("env"));
        Self::prepend_paths(&mut r.env_paths, &paths, normalize_path);
        Self::prepend_paths(&mut r.env_paths_append, &append_paths, normalize_path);

        let context_vars_for_validation = Self::context_vars(&ctx);
        let oci_env_keys = if resolve_opts.vars {
            BTreeSet::new()
        } else {
            oci_env_keys(config)
        };

        // Validate required variables
        Self::validate_required_vars(
            &filtered_input_for_validation,
            required_env,
            &r,
            &context_vars_for_validation,
            &oci_env_keys,
            resolve_opts.warn_on_missing_required
                || (resolve_opts.vars && prompt::tolerates_missing()),
            resolve_opts.vars,
        )?;

        // A redacted value is a secret by the user's or the directive's own account
        // (age values are redacted by default), and the env cache would write it to disk.
        // `env_cache_redacted` opts in; decrypted values were marked above regardless.
        // A redacted var can change without the config file changing, and a `default` or
        // `required` depends on the caller's environment, which the cache key does not see.
        let caller_dependent = r
            .redactions
            .iter()
            .any(|k| r.caller_env_keys.contains(k) || r.rendered_defaults.contains(k));
        if !r.redactions.is_empty()
            && (!Settings::get().env_cache_redacted || resolve_opts.vars || caller_dependent)
        {
            r.has_uncacheable = true;
        }

        Ok(r)
    }

    fn validate_required_vars(
        input: &[(EnvDirective, PathBuf)],
        initial: &EnvMap,
        env_results: &EnvResults,
        context_vars: &EnvMap,
        oci_env_keys: &BTreeSet<String>,
        warn_mode: bool,
        vars_mode: bool,
    ) -> eyre::Result<()> {
        let mut required_vars = Vec::new();

        // Collect all required variables with their options
        for (directive, source) in input {
            match directive {
                EnvDirective::Val(key, _, options) if options.required.is_required() => {
                    required_vars.push((key.clone(), source.clone(), options.required.clone()));
                }
                EnvDirective::Default(key, _, options) if options.required.is_required() => {
                    required_vars.push((key.clone(), source.clone(), options.required.clone()));
                }
                EnvDirective::Required(key, options) => {
                    required_vars.push((key.clone(), source.clone(), options.required.clone()));
                }
                _ => {}
            }
        }

        // Check if required variables are defined
        for (var_name, declaring_source, required_value) in required_vars {
            // Looked up under the folded name, reported under the one the config wrote. On
            // Windows `Path = { required = true }` asks after the variable that is spelled
            // `PATH` in the environment, and `Val`/`Default` store it folded, so a lookup on
            // the literal name reads a variable that is set as missing.
            let lookup = if vars_mode {
                var_name.clone()
            } else {
                env::normalize_path_key(var_name.clone())
            };

            // Variable must be defined either:
            // 1. In the initial environment (before mise runs), OR
            // 2. In a config file processed later than the one declaring it as required, OR
            // 3. In `[oci.env]`, when `mise oci` is building the image
            let is_predefined = initial.contains_key(&lookup);

            let resolved_values = if vars_mode {
                &env_results.vars
            } else {
                &env_results.env
            };
            let is_defined_later = if let Some((_, var_source)) = resolved_values.get(&lookup) {
                // Check if the variable comes from a different config file
                var_source != &declaring_source
            } else {
                false
            };
            let is_defined_in_context =
                vars_mode && context_vars.get(&lookup).is_some_and(|v| !v.is_empty());

            let is_defined_by_oci_env = oci_env_keys.contains(&lookup);

            if !is_predefined
                && !is_defined_later
                && !is_defined_in_context
                && !is_defined_by_oci_env
            {
                let variable_kind = if vars_mode {
                    "variable"
                } else {
                    "environment variable"
                };
                let base_message = format!(
                    "Required {} '{}' is not defined. It must be set before mise runs or in a later config file. (Required in: {})",
                    variable_kind,
                    var_name,
                    display_path(declaring_source)
                );

                let message = if let Some(help) = required_value.help_text() {
                    format!("{}\nHelp: {}", base_message, help)
                } else {
                    base_message
                };

                if warn_mode {
                    warn!("{}", message);
                } else {
                    return Err(eyre!("{}", message));
                }
            }
        }

        Ok(())
    }

    fn context_vars(ctx: &tera::Context) -> EnvMap {
        if let Some(existing_vars) = ctx.get("vars").and_then(|v| v.as_map()) {
            existing_vars
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string())))
                .collect()
        } else {
            EnvMap::new()
        }
    }

    fn parse_template(
        &self,
        ctx: &tera::Context,
        tera: &mut Option<TeraEngine>,
        path: &Path,
        exec_env: &EnvMap,
        origin: &str,
        input: &str,
    ) -> eyre::Result<String> {
        // Step 1: Tera template expansion
        let output = self.render_template(ctx, tera, path, exec_env, input)?;
        // Step 2: Shell-style $VAR expansion
        Ok(Self::shell_expand(ctx, path, origin, output))
    }

    /// The Tera step of [`Self::parse_template`].
    fn render_template(
        &self,
        ctx: &tera::Context,
        tera: &mut Option<TeraEngine>,
        path: &Path,
        exec_env: &EnvMap,
        input: &str,
    ) -> eyre::Result<String> {
        let mut output = input.to_string();
        if contains_template_syntax(input) {
            trust_check(path)?;
            let tera = tera.get_or_insert_with(|| {
                let mut tera = get_tera(path.parent());
                // Re-bind exec() to the accumulated env vars — but never in safe
                // mode, where get_tera has installed an erroring stub that this
                // registration must not override.
                if !Settings::get().safe {
                    if let TeraEngine::V2(tera) = &mut tera {
                        tera.register_function(
                            "exec",
                            tera_exec(path.parent().map(|d| d.to_path_buf()), exec_env.clone()),
                        );
                    } else if let TeraEngine::V1(tera) = &mut tera {
                        tera.register_function(
                            "exec",
                            tera1_exec(path.parent().map(|d| d.to_path_buf()), exec_env.clone()),
                        );
                    }
                }
                tera
            });
            output = render_config_str(tera, input, ctx, path).wrap_err_with(|| {
                // a glob's substitutions are marked; show the template as written
                let input = SubstitutionMarks::strip_any(input);
                eyre!("failed to parse template: '{input}'")
            })?;
        }
        Ok(output)
    }

    /// The `$VAR` step of [`Self::parse_template`], for a value whose templates are rendered.
    fn shell_expand(ctx: &tera::Context, path: &Path, origin: &str, mut output: String) -> String {
        if output.contains('$') && Settings::get().env_shell_expand {
            let env_vars: BTreeMap<String, String> = ctx
                .get("env")
                .and_then(|v| serde::Deserialize::deserialize(v.clone()).ok())
                .unwrap_or_default();
            let mut missing_vars = Vec::new();
            output = shell_expand_env(&output, &env_vars, &mut missing_vars);
            warn_unexpanded_vars(missing_vars, origin, path);
        }
        output
    }

    /// Puts each config file's `paths` ahead of `target`, so the config file that sorts last
    /// ends up first. A file's own entries keep the order they were written in.
    fn prepend_paths(
        target: &mut Vec<PathBuf>,
        paths: &[(PathBuf, PathBuf)],
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
    ) {
        for (source, paths) in &paths.iter().chunk_by(|(_, source)| source) {
            // Use the computed config_root (project root for nested configs) for path resolution
            // to be consistent with other env directives like _.source and _.file
            let config_root = crate::config::config_file::config_root::config_root(source);
            let paths = paths.map(|(p, _)| p).collect_vec();
            let mut paths = paths
                .iter()
                .rev()
                .flat_map(|path| env::split_paths(path))
                .map(|s| normalize_path(&config_root, s))
                .collect::<Vec<_>>();
            // target is already reversed and paths should prepend target
            paths.reverse();
            paths.extend(std::mem::take(target));
            *target = paths;
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.env.is_empty()
            && self.vars.is_empty()
            && self.env_remove.is_empty()
            && self.env_files.is_empty()
            && self.env_paths.is_empty()
            && self.env_paths_append.is_empty()
            && self.env_scripts.is_empty()
            && self.tool_add_paths.is_empty()
    }
}

/// The warning for a `$VAR` that `shell_expand_env` could not expand.
///
/// Built as a string rather than warned in place so it can be tested without
/// `warn_once!`'s process-global dedup set, which would make any test asserting
/// on emitted output depend on whatever ran before it.
///
/// Naming the missing variable alone was not actionable. The reporter in
/// discussion #8232 could see *which* variable was missing but not which value
/// referenced it, and `mise env` cannot show that: a missing variable is by
/// definition absent from the environment. `origin` is the referencing `[env]`
/// key where one exists, and the directive otherwise.
fn unexpanded_var_warning(var: &str, origin: &str, path: &Path) -> String {
    format!(
        "env var '{var}' is not defined and will be left unexpanded, \
         referenced by `{origin}` in {}. \
         Use ${{{var}:-}} to default to an empty string and suppress this warning.",
        display_path(path)
    )
}

/// Warn once for each variable left unexpanded, naming what referenced it.
pub(crate) fn warn_unexpanded_vars(missing: Vec<String>, origin: &str, path: &Path) {
    for var in missing {
        warn_once!("{}", unexpanded_var_warning(&var, origin, path));
    }
}

pub(crate) fn shell_expand_env(
    input: &str,
    env_vars: &BTreeMap<String, String>,
    missing_vars: &mut Vec<String>,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some((_, '$')) => {
                chars.next();
                output.push('$');
            }
            Some((_, '{')) => {
                chars.next();
                if let Some((end, expr)) = read_braced_expr(input, idx + 2) {
                    output.push_str(&expand_braced_expr(
                        expr,
                        &input[idx..=end],
                        env_vars,
                        missing_vars,
                    ));
                    while chars.peek().is_some_and(|(i, _)| *i <= end) {
                        chars.next();
                    }
                } else {
                    output.push_str(&input[idx..]);
                    break;
                }
            }
            Some((_, next)) if is_var_start(next) => {
                let start = chars.peek().map(|(i, _)| *i).unwrap_or(idx + 1);
                let mut end = start;
                while let Some((i, next)) = chars.peek().copied() {
                    if is_var_char(next) {
                        end = i + next.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let var = &input[start..end];
                if let Some(value) = env_vars.get(var) {
                    output.push_str(value);
                } else {
                    missing_vars.push(var.to_string());
                    output.push_str(&input[idx..end]);
                }
            }
            _ => output.push('$'),
        }
    }

    output
}

fn read_braced_expr(input: &str, start: usize) -> Option<(usize, &str)> {
    let mut depth = 1;
    let mut command_depth = 0;
    let mut chars = input[start..].char_indices().peekable();

    while let Some((offset, ch)) = chars.next() {
        let idx = start + offset;
        if command_depth > 0 {
            match ch {
                '(' => command_depth += 1,
                ')' => command_depth -= 1,
                _ => {}
            }
            continue;
        }

        match ch {
            '$' if chars.peek().is_some_and(|(_, next)| *next == '{') => {
                chars.next();
                depth += 1;
            }
            '$' if chars.peek().is_some_and(|(_, next)| *next == '(') => {
                chars.next();
                command_depth = 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((idx, &input[start..idx]));
                }
            }
            _ => {}
        }
    }

    None
}

fn expand_braced_expr(
    expr: &str,
    original: &str,
    env_vars: &BTreeMap<String, String>,
    missing_vars: &mut Vec<String>,
) -> String {
    let Some(var_end) = expr
        .char_indices()
        .take_while(|(_, ch)| is_var_char(*ch))
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
    else {
        return original.to_string();
    };

    let var = &expr[..var_end];
    if !var.chars().next().is_some_and(is_var_start) {
        return original.to_string();
    }

    match expr.get(var_end..) {
        Some("") => match env_vars.get(var) {
            Some(value) => value.to_string(),
            None => {
                missing_vars.push(var.to_string());
                original.to_string()
            }
        },
        Some(rest) if rest.starts_with(":-") => {
            let default = &rest[2..];
            match env_vars.get(var) {
                Some(value) if !value.is_empty() => value.to_string(),
                _ => shell_expand_env(default, env_vars, missing_vars),
            }
        }
        Some(rest) if rest.starts_with('-') => {
            let default = &rest[1..];
            match env_vars.get(var) {
                Some(value) => value.to_string(),
                None => shell_expand_env(default, env_vars, missing_vars),
            }
        }
        _ => original.to_string(),
    }
}

/// Whether rendered `text` holds a `$VAR` or `${…}` that [`shell_expand_env`] expands. A `$$`
/// is a written `$`, and so is a `${` with no closing brace.
fn has_shell_substitution(text: &str) -> bool {
    let mut chars = text.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c != '$' {
            continue;
        }
        match chars.peek() {
            Some((_, '$')) => {
                chars.next();
            }
            Some((_, '{')) if read_braced_expr(text, idx + 2).is_some() => return true,
            Some(&(_, next)) if is_var_start(next) => return true,
            _ => {}
        }
    }
    false
}

fn is_var_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_var_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

impl Debug for EnvResults {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("EnvResults");
        if !self.env.is_empty() {
            ds.field("env", &self.env.keys().collect::<Vec<_>>());
        }
        if !self.vars.is_empty() {
            ds.field("vars", &self.vars.keys().collect::<Vec<_>>());
        }
        if !self.env_remove.is_empty() {
            ds.field("env_remove", &self.env_remove);
        }
        if !self.env_paths.is_empty() {
            ds.field("env_paths", &self.env_paths);
        }
        if !self.env_paths_append.is_empty() {
            ds.field("env_paths_append", &self.env_paths_append);
        }
        if !self.env_scripts.is_empty() {
            ds.field("env_scripts", &self.env_scripts);
        }
        if !self.tool_add_paths.is_empty() {
            ds.field("tool_add_paths", &self.tool_add_paths);
        }
        ds.finish()
    }
}

/// Whether safe mode ignores `directive` from `source` (see `resolve_with_toolset`): any
/// directive from project config, and `_.source` from any config.
pub(super) fn dropped_in_safe_mode(directive: &EnvDirective, source: &Path) -> bool {
    !crate::config::is_global_config(source) || matches!(directive, EnvDirective::Source(..))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::env_diff::EnvMap;
    use crate::tera::BASE_CONTEXT;

    /// Naming the missing variable was never the hard part — finding what
    /// referenced it was. See discussion #8232.
    #[test]
    fn test_unexpanded_var_warning_names_the_reference() {
        let path = Path::new("/p/mise.toml");
        let msg = unexpanded_var_warning("DB_HOST", "DATABASE_URL", path);
        assert!(msg.contains("'DB_HOST'"), "{msg}");
        assert!(msg.contains("`DATABASE_URL`"), "{msg}");
        assert!(msg.contains(&display_path(path)), "{msg}");
        // The existing escape hatch has to survive the rewording.
        assert!(msg.contains("${DB_HOST:-}"), "{msg}");
    }

    /// A `_.file` or `_.source` glob reads as it does in a shell. `.env.*` names the files next
    /// to the config, not those of a repository cloned somewhere below it.
    #[test]
    fn test_glob_files_in_stays_in_its_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for file in [
            ".env.local",
            ".env.test",
            ".env.dir/inner",
            "cloned/.env.example",
            "cloned/docs/.env.example",
            "env.d/a.env",
            "env.d/nested/b.env",
            "#dev-local.env",
            "!x.env",
        ] {
            let file = root.join(file);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "").unwrap();
        }
        let glob = |pattern: &str| glob_files_in(root, pattern);
        let files = |names: &[&str]| names.iter().map(|name| root.join(name)).collect_vec();

        assert_eq!(glob(".env.*"), files(&[".env.local", ".env.test"]));
        assert_eq!(glob("env.d/*.env"), files(&["env.d/a.env"]));
        assert_eq!(glob("*/.env.*"), files(&["cloned/.env.example"]));
        // the alternatives of a group load in name order, each file once
        assert_eq!(
            glob("{env.d,cloned,env.d}/*"),
            files(&["cloned/.env.example", "env.d/a.env"])
        );
        // `**` asks for the tree
        assert_eq!(
            glob("**/.env.example"),
            files(&["cloned/.env.example", "cloned/docs/.env.example"])
        );
        // a `*` a group puts next to another does not make a `**`: only one the pattern
        // writes descends, inside a group too
        for pattern in [
            "*{*,x}/.env.example",
            "{*,x}*/.env.example",
            "*{,*}/.env.example",
        ] {
            assert_eq!(glob(pattern), files(&["cloned/.env.example"]), "{pattern}");
        }
        for pattern in [
            "{**,x}/.env.example",
            "**{*,x}/.env.example",
            "{x,cloned/**}",
        ] {
            assert_eq!(
                glob(pattern),
                files(&["cloned/.env.example", "cloned/docs/.env.example"]),
                "{pattern}"
            );
        }
        // a `*` escaped next to a written one is a character of a name, and the `*` stays one
        if cfg!(unix) {
            assert_eq!(glob("\\**/.env.example"), files(&[]));
        }
        // a leading `#` or `!` is part of a name, not gitignore syntax
        assert_eq!(glob("#dev*.env"), files(&["#dev-local.env"]));
        assert_eq!(glob("!x*"), files(&["!x.env"]));
        // a trailing separator selects directories, which hold no file to load
        assert_eq!(glob(".env.*/"), files(&[]));
    }

    /// A global `_.file` or `_.source` entry is exempt from paranoid env-file trust only when
    /// it names a fixed path. These follow the working directory or the environment.
    #[test]
    fn test_is_templated() {
        for input in [
            "{{cwd}}/.env",
            "{{ env.PWD }}/.env",
            "{% if true %}.env{% endif %}",
            "$PWD/.env",
            "${PWD}/env.sh",
        ] {
            assert!(is_templated_with(input, true), "{input}");
        }
        // with `env_shell_expand` off mise reads a `$` as written text, so the path is fixed
        for input in ["$PWD/.env", "${PWD}/env.sh", "/opt/$HOME/.env"] {
            assert!(!is_templated_with(input, false), "{input}");
        }
        // but a template still renders
        for input in ["{{cwd}}/.env", "{% if true %}$PWD{% endif %}/.env"] {
            assert!(is_templated_with(input, false), "{input}");
        }
        for input in [
            ".env",
            "~/.env",
            "/etc/mise/env.sh",
            "../shared/.env.json",
            "/opt/mise/cost$.env",
            "/opt/mise/a$$b.env",
        ] {
            assert!(!is_templated_with(input, true), "{input}");
        }
    }

    /// A directory is taken as it is named, even when its name holds a `*`. Windows does not
    /// allow such a name.
    #[test]
    #[cfg(unix)]
    fn test_glob_files_in_takes_the_directory_literally() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for file in ["src*/.env.a", "srcfoo/.env.b"] {
            let file = root.join(file);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "").unwrap();
        }
        assert_eq!(
            glob_files_in(&root.join("src*"), ".env.*"),
            vec![root.join("src*/.env.a")]
        );
    }

    /// Only a wildcard the entry writes makes a pattern. What a template or `$VAR` renders is
    /// part of a name.
    #[test]
    fn test_writes_a_star() {
        for input in [
            ".env",
            "{{cwd}}/.env",
            "{{ env.X | default(value='*') }}",
            "${X:-*}/.env",
            "{# * #}.env",
            "{{ '}}*' }}.env",
            "${X:-{{ '*' }}}.env",
            "{% if '*' %}.env{% endif %}",
            // `?` alone is not a glob, as before
            "a?.env",
        ] {
            assert!(!writes_a_star(input, true), "{input}");
        }
        for input in [
            ".env.*",
            "{{cwd}}/.env.*",
            "$PWD/env.d/*.env",
            "conf/[ab]/*.env",
            "conf/*/{{name}}/x.env",
            "{% if x %}a/*.env{% else %}b/*.env{% endif %}",
        ] {
            assert!(writes_a_star(input, true), "{input}");
        }
        // with `env_shell_expand` off a `$` is written text, and so is what follows it
        assert!(writes_a_star("${X:-*}/.env", false));
    }

    /// What `parse_path_template` makes of `input` when it writes a glob: the directory and the
    /// glob below it. `$VAR`s expand from `env` in `ctx` when `shell_expand` is on.
    fn split_path(
        engine: &mut TeraEngine,
        input: &str,
        ctx: &tera::Context,
        shell_expand: bool,
    ) -> eyre::Result<Option<(String, String)>> {
        let env: BTreeMap<String, String> = ctx
            .get("env")
            .and_then(|v| serde::Deserialize::deserialize(v.clone()).ok())
            .unwrap_or_default();
        let marks = SubstitutionMarks::new(shell_expand);
        let rendered =
            mise_util::tera::render_str_with_env(engine, &marks.wrap_substitutions(input), ctx)
                .unwrap_or_else(|err| panic!("{input}: {err}"));
        let expand = |text: String| {
            if shell_expand {
                shell_expand_env(&text, &env, &mut vec![])
            } else {
                text
            }
        };
        Ok(marks
            .split_glob(&rendered)?
            .map(|(dir, glob)| (expand(dir), expand(glob))))
    }

    fn glob_context() -> tera::Context {
        let env = BTreeMap::from([
            ("P".to_string(), "dev".to_string()),
            ("D".to_string(), "/abs/d*r".to_string()),
            ("X".to_string(), "/a*".to_string()),
        ]);
        let mut ctx = tera::Context::new();
        ctx.insert("env", &env);
        ctx
    }

    /// A template or `$VAR` names the directory a glob is matched in, and what it renders is
    /// taken there as a name. The directory ends at the last separator the entry writes ahead
    /// of the first glob character it writes, and the template and `$` syntax render there as
    /// they do in a path with no glob.
    #[test]
    fn test_substitutions_name_the_directory() {
        let ctx = glob_context();
        let owned = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        for mut engine in [
            TeraEngine::V2(Box::default()),
            TeraEngine::V1(Box::default()),
        ] {
            let mut split = |input: &str| {
                split_path(&mut engine, input, &ctx, true)
                    .unwrap_or_else(|err| panic!("{input}: {err}"))
            };
            for (input, dir, glob) in [
                ("{{ env.X }}/env.d/*.env", "/a*/env.d/", "*.env"),
                ("a/b{{ env.X }}c/*.env", "a/b/a*c/", "*.env"),
                ("conf/[ab]/*.env", "conf/", "[ab]/*.env"),
                ("$D/*.env", "/abs/d*r/", "*.env"),
                ("${P}/*.env", "dev/", "*.env"),
                ("$$P/*.env", "$P/", "*.env"),
                ("$$*.env", "", "$*.env"),
                ("cost$*.env", "", "cost$*.env"),
                ("${A:-${P}}/*", "dev/", "*"),
                ("${A:-${B:-x}}-$P/*", "x-dev/", "*"),
                ("${A:-{{ env.P }}}/*.env", "dev/", "*.env"),
                // a `$` written before a template forms a variable with what it renders
                ("${{ 'P' }}/*.env", "dev/", "*.env"),
                ("${{ 'D' }}/*.env", "/abs/d*r/", "*.env"),
                (".env. {{- env.P }}/*", ".env.dev/", "*"),
                ("{{ env.P -}}  /.e*", "dev/", ".e*"),
                ("a {%- if true %}{{ env.P }}{% endif -%} /*", "adev/", "*"),
                // a tag in the glob renders no text
                ("a/{# c #}*.env", "a/", "*.env"),
                ("a/{% if true %}*{% endif %}.env", "a/", "*.env"),
                // a separator a substitution renders is part of the directory
                ("{{ env.X ~ '/b' }}/*.env", "/a*/b/", "*.env"),
            ] {
                assert_eq!(split(input), owned(dir, glob), "{input}");
            }
            // a written `*` the render drops leaves no glob
            assert_eq!(split("{% if false %}*{% endif %}.env"), None);
            assert_eq!(split("{{ env.X }}.env"), None);
        }
        // a written glob escape starts the glob outside Windows
        if cfg!(unix) {
            let mut engine = TeraEngine::V2(Box::default());
            assert_eq!(
                split_path(&mut engine, "{{ env.P }}/\\[1]*.env", &ctx, true).unwrap(),
                owned("dev/", "\\[1]*.env")
            );
        }
    }

    /// A substitution in the glob, or after it, is an error, whatever it renders: where the
    /// glob starts depends only on what the entry writes.
    #[test]
    fn test_a_substitution_in_the_glob_is_an_error() {
        let ctx = glob_context();
        let mut engine = TeraEngine::V2(Box::default());
        for (input, starts) in [
            ("env.d/{{ env.P }}*.env", "after \"env.d/\""),
            ("env.d/$P*.env", "after \"env.d/\""),
            ("env.d/${P:-}*.env", "after \"env.d/\""),
            ("env.d/${{ 'P' }}*.env", "after \"env.d/\""),
            (
                "env.d/{{ env.A | default(value='') }}*.env",
                "after \"env.d/\"",
            ),
            ("{{ env.X }}*.env", "at the start of the path"),
            ("{{ env.X ~ '/' }}*.env", "at the start of the path"),
            ("*/{{ env.P }}.env", "at the start of the path"),
            ("{{ env.P }}/*/$P.env", "after \"dev/\""),
            ("env/[{{ env.P }}]*.env", "after \"env/\""),
        ] {
            let err = split_path(&mut engine, input, &ctx, true)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(&format!("is part of the glob, which starts {starts}")),
                "{input}: {err}"
            );
        }
    }

    /// With `env_shell_expand` off, a `$` is written text in the glob too.
    #[test]
    fn test_dollars_are_written_text_without_shell_expansion() {
        let ctx = glob_context();
        let mut engine = TeraEngine::V2(Box::default());
        let owned = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        for (input, dir, glob) in [
            ("env.d/$P*.env", "env.d/", "$P*.env"),
            ("$D/*.env", "$D/", "*.env"),
            ("env/[${P}]*.env", "env/", "[${P}]*.env"),
        ] {
            assert_eq!(
                split_path(&mut engine, input, &ctx, false).unwrap(),
                owned(dir, glob),
                "{input}"
            );
        }
    }

    /// Data cannot forge a marker: a substitution that renders one leaves no glob.
    #[test]
    fn test_substituted_markers_leave_no_glob() {
        let marks = SubstitutionMarks::new(true);
        let forged = format!("{}*", marks.close);
        let rendered = marks
            .wrap_substitutions("{{ x }}/*.env")
            .replace("{{ x }}", &forged);
        assert_eq!(marks.split_glob(&rendered).unwrap(), None);
        // a template that drops a substitution's closing marker leaves no glob either
        assert_eq!(marks.split_glob(&format!("{}*", marks.open)).unwrap(), None);
        // and an error shows the template without its markers
        assert_eq!(
            SubstitutionMarks::strip_any(&marks.wrap_substitutions("a/{{ x }}/$Y*")),
            "a/{{ x }}/$Y*"
        );
    }

    /// A tag that renders nothing between two written `*`s does not join them into a `**`.
    #[test]
    fn test_tags_do_not_join_stars() {
        let mut engine = TeraEngine::V2(Box::default());
        let mut split = |input: &str, x: bool| {
            let mut ctx = tera::Context::new();
            ctx.insert("x", &x);
            split_path(&mut engine, input, &ctx, true).unwrap()
        };
        let owned = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        let input = "env/*{% if x %}dev{% endif %}*/.env";
        assert_eq!(split(input, true), owned("env/", "*dev*/.env"));
        assert_eq!(split(input, false), owned("env/", "*/.env"));
        assert_eq!(split("env/*{# c #}*/.env", true), owned("env/", "*/.env"));
        assert_eq!(
            split("env/* {%- if x %}{% endif -%} */.env", false),
            owned("env/", "*/.env")
        );
        // a `**` the entry writes still descends, and so does one a tag sits next to
        assert_eq!(split("env/**/.env", true), owned("env/", "**/.env"));
        assert_eq!(split("env/**{# c #}/.env", true), owned("env/", "**/.env"));
        assert_eq!(split("env/**{# c #}*/.env", true), owned("env/", "**/.env"));
        // two written `**`s that a tag which renders nothing joins are one `**`, not `****`
        let input = "env/**{% if x %}/dev/{% endif %}**/.env";
        assert_eq!(split(input, true), owned("env/", "**/dev/**/.env"));
        assert_eq!(split(input, false), owned("env/", "**/.env"));
        // an escaped `*` is a name, not part of a run
        #[cfg(unix)]
        assert_eq!(
            split("env/\\*{# c #}*.env", true),
            owned("env/", "\\**.env")
        );
        // and no boundary marker is left in the directory or a literal path
        assert_eq!(
            split("{% if x %}a{% endif %}/*{# c #}.env", true),
            owned("a/", "*.env")
        );
        let marks = SubstitutionMarks::new(true);
        let wrapped = marks.wrap_substitutions("a*{# c #}");
        assert_eq!(marks.strip(&wrapped), "a*{# c #}");
        assert_eq!(SubstitutionMarks::strip_any(&wrapped), "a*{# c #}");
    }

    /// The body of a `{% raw %}` block is text the entry writes, as the other template scans
    /// read it: no tag opens in it, and a `*` in it makes a pattern. Its `$VAR`s still expand.
    #[test]
    fn test_raw_block_body_is_written() {
        use Piece::*;
        let pieces = |input: &str| template_pieces(input, true);
        assert_eq!(
            pieces("{% raw %}{{label{% endraw %}/*.env"),
            vec![
                (Tag, 0..9),
                (Written, 9..16),
                (Tag, 16..28),
                (Written, 28..34)
            ]
        );
        assert!(writes_a_star("{% raw %}{{label{% endraw %}/*.env", true));
        assert!(writes_a_star("{% raw %}{{ '*' }}{% endraw %}.env", true));
        assert!(!writes_a_star(
            "{% raw %}{{label{% endraw %}{{ '*' }}",
            true
        ));
        // a `$VAR` in the body expands after the render, and a `${` ends at the first brace
        // that closes it, since a tag does not open in the body
        assert_eq!(
            pieces("{% raw %}$HOME/{{x{% endraw %}"),
            vec![
                (Tag, 0..9),
                (Substituted, 9..14),
                (Written, 14..18),
                (Tag, 18..30)
            ]
        );
        assert_eq!(
            pieces("{% raw %}${A:-{{b}}{% endraw %}"),
            vec![
                (Tag, 0..9),
                (Substituted, 9..18),
                (Written, 18..19),
                (Tag, 19..31)
            ]
        );
        assert_eq!(
            template_pieces("{% raw %}$HOME/{{x{% endraw %}", false),
            vec![(Tag, 0..9), (Written, 9..18), (Tag, 18..30)]
        );
        // a `$` that ends the body comes before the `{% endraw %}` tag, not a `{`: once rendered
        // it is followed by what comes after the tag, and `cost$*` expands to itself
        assert_eq!(
            pieces("env/{% raw %}cost${% endraw %}*.env"),
            vec![
                (Written, 0..4),
                (Tag, 4..13),
                (Written, 13..18),
                (Tag, 18..30),
                (Written, 30..35),
            ]
        );
        assert!(writes_a_star("env/{% raw %}cost${% endraw %}*.env", true));
        assert_eq!(
            split_path(
                &mut TeraEngine::V2(Box::default()),
                "env/{% raw %}cost${% endraw %}*.env",
                &glob_context(),
                true
            )
            .unwrap(),
            Some(("env/".to_string(), "cost$*.env".to_string()))
        );
        // and in a `${` fallback it does not open another one: `${A:-$}` closes at the `}`
        assert_eq!(
            pieces("{% raw %}${A:-${% endraw %}}*.env"),
            vec![(Tag, 0..9), (Substituted, 9..28), (Written, 28..33)]
        );
        assert!(writes_a_star("{% raw %}${A:-${% endraw %}}*.env", true));
        // a raw block in a `${` fallback is text too
        assert_eq!(
            pieces("${A:-{% raw %}}{% endraw %}}"),
            vec![(Substituted, 0..15), (Tag, 15..27), (Written, 27..28)]
        );
        // the `-` of either tag trims the whitespace next to it
        assert_eq!(
            pieces("a {%- raw -%} b {%- endraw -%} c"),
            vec![
                (Written, 0..1),
                (Tag, 1..14),
                (Written, 14..15),
                (Tag, 15..31),
                (Written, 31..32),
            ]
        );
        // an unclosed block makes the rest of the entry its body
        assert_eq!(
            pieces("{% raw %}{{a/*.env"),
            vec![(Tag, 0..9), (Written, 9..18)]
        );
        assert!(writes_a_star("{% raw %}{{a/*.env", true));
        // and once rendered, a `{` the body writes is a glob character like any written one
        let mut engine = TeraEngine::V2(Box::default());
        assert_eq!(
            split_path(
                &mut engine,
                "d/{% raw %}{{label{% endraw %}/*.env",
                &glob_context(),
                true
            )
            .unwrap(),
            Some(("d/".to_string(), "{{label/*.env".to_string()))
        );
        assert_eq!(
            split_path(
                &mut engine,
                "{% raw %}$P{% endraw %}/*.env",
                &glob_context(),
                true
            )
            .unwrap(),
            Some(("dev/".to_string(), "*.env".to_string()))
        );
    }

    /// A `$VAR` that tags join is a substitution once rendered: in the glob it is an error, and
    /// in the directory it expands. A `$$` stays a written `$`.
    #[test]
    fn test_a_variable_that_tags_join_in_the_glob_is_an_error() {
        let mut engine = TeraEngine::V2(Box::default());
        let ctx = glob_context();
        let owned = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        for input in [
            "profiles/${% if true %}P{% endif %}.e*",
            "profiles/${% if true %}{P}{% endif %}.e*",
        ] {
            let err = split_path(&mut engine, input, &ctx, true).unwrap_err();
            assert!(
                err.to_string().contains("part of the glob"),
                "{input}: {err}"
            );
            // with shell expansion off, the `$` is a name
            let off = split_path(&mut engine, input, &ctx, false).unwrap();
            assert!(off.is_some_and(|(dir, _)| dir == "profiles/"), "{input}");
        }
        assert_eq!(
            split_path(&mut engine, "p/$${% if true %}P{% endif %}.e*", &ctx, true).unwrap(),
            owned("p/", "$P.e*")
        );
        assert_eq!(
            split_path(&mut engine, "p/${% if true %}{P{% endif %}*", &ctx, true).unwrap(),
            owned("p/", "${P*")
        );
        assert_eq!(
            split_path(&mut engine, "${% if true %}D{% endif %}/*.env", &ctx, true).unwrap(),
            owned("/abs/d*r/", "*.env")
        );
    }

    /// The `?` of a Windows verbatim prefix is no wildcard.
    #[cfg(windows)]
    #[test]
    fn test_a_windows_prefix_holds_no_glob() {
        let mut engine = TeraEngine::V2(Box::default());
        let ctx = glob_context();
        assert_eq!(
            split_path(&mut engine, r"\\?\C:\repo\*.env", &ctx, false).unwrap(),
            Some((r"\\?\C:\repo\".to_string(), "*.env".to_string()))
        );
    }

    /// The pattern is found in what renders, so each branch of a conditional can hold its own.
    #[test]
    fn test_the_rendered_branch_holds_the_pattern() {
        let mut engine = TeraEngine::V2(Box::default());
        let mut split = |input: &str, x: bool| {
            let mut ctx = tera::Context::new();
            ctx.insert("x", &x);
            ctx.insert("name", "n*");
            split_path(&mut engine, input, &ctx, true).unwrap()
        };
        let owned = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        let input = "{% if x %}a/*.env{% else %}b/{{ name }}/*.env{% endif %}";
        assert_eq!(split(input, true), owned("a/", "*.env"));
        assert_eq!(split(input, false), owned("b/n*/", "*.env"));
        let input = "{% if x %}*.env{% else %}.env{% endif %}";
        assert_eq!(split(input, true), owned("", "*.env"));
        assert_eq!(split(input, false), None);
        let input = "{% if x %}a/{% else %}b/{% endif %}*.env";
        assert_eq!(split(input, true), owned("a/", "*.env"));
        assert_eq!(split(input, false), owned("b/", "*.env"));
    }

    /// A glob matches a symlink to a file, as a path that names the link does.
    #[test]
    #[cfg(unix)]
    fn test_glob_files_in_matches_symlinked_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("env")).unwrap();
        std::fs::write(root.join("target.env"), "").unwrap();
        std::fs::write(root.join("env/.env.a"), "").unwrap();
        std::os::unix::fs::symlink(root.join("target.env"), root.join("env/.env.b")).unwrap();
        std::os::unix::fs::symlink(root.join("missing.env"), root.join("env/.env.c")).unwrap();
        assert_eq!(
            glob_files_in(&root.join("env"), ".env.*"),
            vec![root.join("env/.env.a"), root.join("env/.env.b")]
        );
    }

    /// `ToolsFilter::ToolsOnlyVals` must select only `tools = true` `Val`
    /// directives — excluding `tools = false` vars and `tools = true` *modules*
    /// (here a `Path`). This is what `dependency_env` relies on to surface vars
    /// like `CLOUDSDK_PYTHON = "{{ tools.python.path }}/..."` during a dependent
    /// tool's install without running env modules on a partial toolset. (#10282)
    #[tokio::test]
    async fn test_tools_only_vals_filter() {
        let env = EnvMap::new();
        let config = Config::get().await.unwrap();
        let tools = EnvDirectiveOptions {
            tools: true,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &env,
            vec![
                // tools = true Val -> included
                (
                    EnvDirective::Val("TOOLS_VAL".into(), "yes".into(), tools.clone()),
                    PathBuf::from("/config"),
                ),
                // tools = false Val -> excluded
                (
                    EnvDirective::Val("PLAIN_VAL".into(), "no".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
                // tools = true module (Path) -> excluded
                (
                    EnvDirective::Path("/should/not/appear".into(), tools.clone()),
                    PathBuf::from("/config"),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::ToolsOnlyVals,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();
        let keys: Vec<String> = results.env.keys().cloned().collect();
        assert_eq!(keys, vec!["TOOLS_VAL".to_string()]);
        assert!(results.env_paths.is_empty());
    }

    async fn resolve_for_cache_test(directives: Vec<EnvDirective>) -> EnvResults {
        let config = Config::get().await.unwrap();
        EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &EnvMap::new(),
            directives
                .into_iter()
                .map(|d| (d, PathBuf::from("/config")))
                .collect(),
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_rendered_value_is_not_rendered_again() {
        // what a parent task's `{{ cwd }}` renders to in a directory named like a template
        let text = "/repo/{{ exec(command='echo ran') }}/$$HOST";
        let results = resolve_for_cache_test(vec![
            EnvDirective::Val("HOST".into(), "h".into(), Default::default()),
            EnvDirective::rendered("FINAL".into(), text.into(), Rendered::Final),
            EnvDirective::rendered("ENTRY".into(), text.into(), Rendered::Templates),
        ])
        .await;
        assert_eq!(results.env["FINAL"].0, text);
        // the env of a `depends` or `run` entry still gets its one `$VAR` pass
        assert_eq!(
            results.env["ENTRY"].0,
            "/repo/{{ exec(command='echo ran') }}/$HOST"
        );
    }

    #[tokio::test]
    async fn test_plain_value_stays_cacheable() {
        let results = resolve_for_cache_test(vec![EnvDirective::Val(
            "PLAIN".into(),
            "value".into(),
            Default::default(),
        )])
        .await;
        assert!(!results.has_uncacheable);
    }

    #[tokio::test]
    async fn test_redacted_value_is_uncacheable() {
        let options = EnvDirectiveOptions {
            redact: Some(true),
            ..Default::default()
        };
        let results = resolve_for_cache_test(vec![EnvDirective::Val(
            "SECRET".into(),
            "value".into(),
            options,
        )])
        .await;
        assert!(results.has_uncacheable);
    }

    #[tokio::test]
    async fn test_skipped_default_does_not_override_redaction() {
        let env = EnvMap::from_iter([("SECRET_TOKEN".to_string(), "secret".to_string())]);
        let config = Config::get().await.unwrap();
        let options = EnvDirectiveOptions {
            redact: Some(false),
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &env,
            vec![(
                EnvDirective::Default("SECRET_TOKEN".into(), "fallback".into(), options),
                PathBuf::from("/config"),
            )],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();

        assert!(!results.redaction_exclusions.contains("SECRET_TOKEN"));
    }

    #[tokio::test]
    async fn test_reassignment_and_remove_clear_redaction_exclusion() {
        let config = Config::get().await.unwrap();
        let excluded = EnvDirectiveOptions {
            redact: Some(false),
            ..Default::default()
        };
        let initial = EnvMap::new();
        let resolve = |directives| {
            EnvResults::resolve(
                &config,
                BASE_CONTEXT.clone(),
                &initial,
                directives,
                EnvResolveOptions {
                    vars: false,
                    tools: ToolsFilter::Both,
                    warn_on_missing_required: false,
                },
            )
        };

        let reassigned = resolve(vec![
            (
                EnvDirective::Val("TOKEN".into(), "first".into(), excluded.clone()),
                PathBuf::from("/global"),
            ),
            (
                EnvDirective::Val("TOKEN".into(), "second".into(), Default::default()),
                PathBuf::from("/local"),
            ),
        ])
        .await
        .unwrap();
        assert!(!reassigned.redaction_exclusions.contains("TOKEN"));

        let removed = resolve(vec![
            (
                EnvDirective::Val("TOKEN".into(), "first".into(), excluded),
                PathBuf::from("/global"),
            ),
            (
                EnvDirective::Rm("TOKEN".into(), Default::default()),
                PathBuf::from("/local"),
            ),
        ])
        .await
        .unwrap();
        assert!(!removed.redaction_exclusions.contains("TOKEN"));
    }

    /// A `required` directive validates that the caller set a key but must never assign it
    /// into `env` — otherwise the value would leak into anything that reads `EnvResults::env`
    /// directly. Redaction instead resolves the value from the caller environment via
    /// `caller_env_keys` and `redactable_env`.
    #[tokio::test]
    async fn test_required_records_caller_key_without_assigning() {
        let initial = EnvMap::from_iter([("ASC_KEY_ID".to_string(), "caller_key".to_string())]);
        let config = Config::get().await.unwrap();
        let options = EnvDirectiveOptions {
            redact: Some(true),
            required: RequiredValue::True,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &initial,
            vec![(
                EnvDirective::Required("ASC_KEY_ID".into(), options),
                PathBuf::from("/config"),
            )],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();

        assert!(!results.env.contains_key("ASC_KEY_ID"));
        assert!(results.caller_env_keys.contains("ASC_KEY_ID"));
        assert!(results.redactions.contains(&"ASC_KEY_ID".to_string()));
        assert_eq!(
            results.redactable_env(&initial).get("ASC_KEY_ID"),
            Some(&"caller_key".to_string())
        );
    }

    /// A key removed by `_.rm` is unreachable by the task, so its caller value must stop being
    /// resolvable for redaction. Otherwise `redactable_env` would keep supplying it and register
    /// a pattern for a variable the config explicitly removed — over-redaction that rewrites
    /// unrelated output when the value is short.
    #[tokio::test]
    async fn test_rm_clears_a_recorded_caller_key() {
        let initial = EnvMap::from_iter([
            ("ASC_KEY_ID".to_string(), "caller_key".to_string()),
            ("DEF_TOKEN".to_string(), "caller_default".to_string()),
        ]);
        let config = Config::get().await.unwrap();
        let redacted = EnvDirectiveOptions {
            redact: Some(true),
            required: RequiredValue::True,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &initial,
            vec![
                (
                    EnvDirective::Required("ASC_KEY_ID".into(), redacted.clone()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Default(
                        "DEF_TOKEN".into(),
                        "fallback".into(),
                        EnvDirectiveOptions {
                            redact: Some(true),
                            ..Default::default()
                        },
                    ),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Rm("ASC_KEY_ID".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Rm("DEF_TOKEN".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();

        assert!(!results.caller_env_keys.contains("ASC_KEY_ID"));
        assert!(!results.caller_env_keys.contains("DEF_TOKEN"));
        let redactable = results.redactable_env(&initial);
        assert!(!redactable.contains_key("ASC_KEY_ID"));
        assert!(!redactable.contains_key("DEF_TOKEN"));
    }

    /// `required = true, redact = false` on a directive that assigns nothing must still record
    /// an exclusion, or a global `redactions` pattern would redact the caller's value anyway.
    #[tokio::test]
    async fn test_required_redact_false_excludes_key() {
        let initial = EnvMap::from_iter([("ASC_KEY_ID".to_string(), "caller_key".to_string())]);
        let config = Config::get().await.unwrap();
        let options = EnvDirectiveOptions {
            redact: Some(false),
            required: RequiredValue::True,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &initial,
            vec![(
                EnvDirective::Required("ASC_KEY_ID".into(), options),
                PathBuf::from("/config"),
            )],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();

        assert!(results.redaction_exclusions.contains("ASC_KEY_ID"));
        assert!(!results.redactions.contains(&"ASC_KEY_ID".to_string()));
    }

    /// A bare `required = true` with no `redact` of its own assigns nothing, so it expresses no
    /// redaction opinion and must not clear an exclusion an earlier directive set. Contrast
    /// `test_reassignment_and_remove_clear_redaction_exclusion`, where a later *assignment*
    /// (`Val`) does clear it.
    #[tokio::test]
    async fn test_bare_required_preserves_earlier_redaction_exclusion() {
        let config = Config::get().await.unwrap();
        let initial = EnvMap::new();
        let excluded = EnvDirectiveOptions {
            redact: Some(false),
            ..Default::default()
        };
        let required = EnvDirectiveOptions {
            required: RequiredValue::True,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &initial,
            vec![
                (
                    EnvDirective::Val("TOKEN".into(), "value".into(), excluded),
                    PathBuf::from("/global"),
                ),
                (
                    EnvDirective::Required("TOKEN".into(), required),
                    PathBuf::from("/local"),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();

        assert!(results.redaction_exclusions.contains("TOKEN"));
    }

    /// `redactable_env` must prefer an assigned value over a caller-supplied one for a key
    /// present in both `env` and `caller_env_keys`, and must skip caller keys the caller never
    /// actually set (no `ABSENT` in the caller environment).
    #[test]
    fn test_redactable_env_prefers_assigned_value() {
        let mut results = EnvResults {
            caller_env_keys: BTreeSet::from([
                "K".to_string(),
                "ONLY_CALLER".to_string(),
                "ABSENT".to_string(),
            ]),
            ..Default::default()
        };
        results.env.insert(
            "K".to_string(),
            ("assigned".to_string(), PathBuf::from("/config")),
        );

        let caller_env = EnvMap::from_iter([
            ("K".to_string(), "caller".to_string()),
            ("ONLY_CALLER".to_string(), "caller_only".to_string()),
        ]);

        let redactable = results.redactable_env(&caller_env);
        assert_eq!(redactable.get("K"), Some(&"assigned".to_string()));
        assert_eq!(
            redactable.get("ONLY_CALLER"),
            Some(&"caller_only".to_string())
        );
        assert!(!redactable.contains_key("ABSENT"));
    }

    /// `Default`'s caller-wins branch assigns nothing (the directive's own value is unused), so
    /// it must record `caller_env_keys` the same way `Required` does, or the caller's value
    /// would escape redaction.
    #[tokio::test]
    async fn test_default_records_caller_key_when_caller_wins() {
        let initial = EnvMap::from_iter([("DEF_TOKEN".to_string(), "caller_default".to_string())]);
        let config = Config::get().await.unwrap();
        let options = EnvDirectiveOptions {
            redact: Some(true),
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &initial,
            vec![(
                EnvDirective::Default("DEF_TOKEN".into(), "fallback".into(), options),
                PathBuf::from("/config"),
            )],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();

        assert!(!results.env.contains_key("DEF_TOKEN"));
        assert!(results.caller_env_keys.contains("DEF_TOKEN"));
        assert_eq!(
            results.redactable_env(&initial).get("DEF_TOKEN"),
            Some(&"caller_default".to_string())
        );
    }
}
