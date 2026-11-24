use crate::env;
use aho_corasick::AhoCorasick;
use indexmap::IndexSet;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

/// Process-wide redaction patterns, registered by config loading.
///
/// Seeded with the secrets inherited from a parent mise, so the first log
/// record of the process is already redacted.
pub static GLOBAL_REDACTOR: LazyLock<Mutex<Redactor>> = LazyLock::new(|| {
    Mutex::new(Redactor::new(inherited_secret_patterns(
        &env::INHERITED_SECRET_KEYS,
        &|k| std::env::var(k).ok(),
    )))
});

/// Every form of `value` that should be redacted: the value itself; for a
/// multi-line value each trimmed line of at least 4 bytes that contains an
/// ASCII alphanumeric (output is redacted line by line); its JSON-escaped and
/// Rust debug forms (without quotes) when they differ from the value; and the
/// shell-quoted spelling of each of those when it holds a `'`.
pub fn secret_patterns(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    };
    push(value.to_string());
    if value.contains('\n') {
        for line in value.lines().map(str::trim) {
            if line.len() >= 4 && line.bytes().any(|b| b.is_ascii_alphanumeric()) {
                push(line.to_string());
            }
        }
    }
    if let Ok(json) = serde_json::to_string(value)
        && let Some(inner) = json.strip_prefix('"').and_then(|j| j.strip_suffix('"'))
        && inner != value
    {
        push(inner.to_string());
    }
    // Rust's `{:?}` spelling (task argument trace logging): U+0001 is `\u{1}`.
    let debug = format!("{value:?}");
    if let Some(inner) = debug.strip_prefix('"').and_then(|d| d.strip_suffix('"')) {
        push(inner.to_string());
    }
    // The POSIX shell spelling inside single quotes (`{{ x | quote }}`, a
    // displayed command line), where `'` is written `'\''`.
    for form in out.clone() {
        let quoted = form.replace('\'', r"'\''");
        if !out.contains(&quoted) {
            out.push(quoted);
        }
    }
    out
}

pub(crate) fn inherited_secret_patterns(
    keys: &BTreeSet<String>,
    get: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    keys.iter()
        .filter_map(|k| get(k))
        .flat_map(|v| secret_patterns(&v))
        .collect()
}

/// Redact registered secrets without needing a loaded `Config`.
///
/// Some of the places a secret can surface have no `Config` to hand:
/// `Display for CmdLineRunner` is rendered into `eyre` errors that are printed
/// long after any config went out of scope.
pub fn redact_global(input: &str) -> String {
    GLOBAL_REDACTOR.lock().unwrap().redact(input)
}

/// Replace the userinfo of every `scheme://` URL in `text` with `[redacted]`.
///
/// `https://user:token@host/path` becomes `https://[redacted]@host/path`. The
/// [`Redactor`] only hides values that were registered with it, and a URL that
/// carries its own credentials (a config value, a CLI argument) never is.
///
/// In free text whitespace ends a URL, since `https://host:8080 me@x` cannot be
/// told apart from userinfo holding a space. A parsed [`url::Url`]
/// percent-encodes spaces, so its serialization never has one. For a single
/// command-line argument, which may hold a raw space, use
/// [`redact_url_userinfo_in_arg`].
pub fn redact_url_userinfo(text: &str) -> Cow<'_, str> {
    redact_userinfo(text, true)
}

/// [`redact_url_userinfo`] for one command-line argument.
///
/// URL parsers accept a raw space in userinfo (`https://user:my pass@host`), and
/// inside one argument it is part of the URL, so only `/`, `?` or `#` ends the
/// authority. Redact each argument before joining them for display.
pub fn redact_url_userinfo_in_arg(arg: &str) -> Cow<'_, str> {
    redact_userinfo(arg, false)
}

/// Remove the credentials of every `scheme://` URL in `text`.
///
/// Unlike [`redact_url_userinfo`], which marks where a credential was for a
/// reader, this is for values that are stored, compared or used as names
/// (lockfiles, install manifests, directory names), where `[redacted]` would be
/// recorded as if it were part of the URL. `https://user:token@host/path`
/// becomes `https://host/path`. The user of an `ssh` URL is an account name
/// rather than a secret, so `ssh://git@host/repo` is kept as it is and only a
/// password is removed from it. Everything else is kept byte for byte, so the
/// text need not parse as a URL.
///
/// Whitespace ends a URL, as in [`redact_url_userinfo`]; for a single argument
/// or name that may hold a raw space use [`strip_url_credentials_in_arg`].
pub fn strip_url_credentials(text: &str) -> Cow<'_, str> {
    rewrite_userinfo(text, true, Userinfo::Strip)
}

/// [`strip_url_credentials`] for a stored value. A value that is itself a URL
/// (`source = "https://user:my pass@host/"`) is read as one argument, where
/// whitespace can be part of the userinfo; anything else, an argument list such
/// as `npm_args` for one, is free text.
pub fn strip_url_credentials_in_value(value: &str) -> Cow<'_, str> {
    let scheme = value
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .unwrap_or_default();
    let is_url = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if is_url {
        strip_url_credentials_in_arg(value)
    } else {
        strip_url_credentials(value)
    }
}

/// [`strip_url_credentials`] for one command-line argument or tool name, where
/// only `/`, `?` or `#` ends the authority.
pub fn strip_url_credentials_in_arg(arg: &str) -> Cow<'_, str> {
    rewrite_userinfo(arg, false, Userinfo::Strip)
}

/// Whether a URL query parameter named `name` (decoded) carries a credential,
/// judged by its name in any case and naming style (`access_token`,
/// `access-token`, `accessToken`, `ACCESSTOKEN`): `auth`, `authorization`,
/// `key`, `sig`, `jwt`, `pwd`, `X-Amz-Credential`, or one ending in `token`,
/// `secret`, `password`, `signature` or `_key` (`apiKey`, `x-api-key`).
pub fn is_credential_query_param(name: &str) -> bool {
    use heck::ToSnakeCase;
    const NAMES: &[&str] = &[
        "auth",
        "authorization",
        "bearer",
        "jwt",
        "apikey",
        "key",
        "sig",
        "pass",
        "passwd",
        "pwd",
        "credential",
        "credentials",
        "x_amz_credential",
        "x_goog_credential",
    ];
    const SUFFIXES: &[&str] = &["token", "secret", "password", "signature", "_key"];
    let judge =
        |name: &str| NAMES.contains(&name) || SUFFIXES.iter().any(|suffix| name.ends_with(suffix));
    let name = name.replace(TAB_OR_NEWLINE, "");
    // `APIKey` splits as `api_key`, `APIkey` only once lowercased.
    judge(&name.to_snake_case()) || judge(&name.to_lowercase().to_snake_case())
}

/// What a URL parser drops wherever it is in a URL, a parameter name included.
const TAB_OR_NEWLINE: [char; 3] = ['\t', '\n', '\r'];

/// The query of each URL in free text (an argument list, a message), where
/// whitespace or a quote ends a URL.
static URL_QUERY: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(://[^\s?#'"]*)\?([^\s#'"]*)"#).unwrap());

/// The query of a text that starts with a URL, or with a name that holds one
/// (`cargo:https://...`). There raw whitespace and quotes are part of the URL:
/// a URL parser percent-encodes them in a path or query, and the request is
/// sent. The query runs from the first `?` to the `#`. Anchored at the start,
/// so the URLs inside an argument list are read as free text only.
static WHOLE_URL_QUERY: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(\s*[A-Za-z][A-Za-z0-9+._:-]*://[^?#]*)\?([^#]*)").unwrap()
});

/// The query of one command-line argument, which is one URL wherever its
/// `://` is (`--index-url=https://...`).
static ARG_URL_QUERY: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(://[^?#]*)\?([^#]*)").unwrap());

/// The decoded name of the query parameter `pair` (`name=value`).
fn query_param_name(pair: &str) -> String {
    let pair = pair.replace(TAB_OR_NEWLINE, "");
    url::form_urlencoded::parse(pair.as_bytes())
        .next()
        .map(|(name, _)| name.into_owned())
        .unwrap_or_default()
}

/// The decoded names of the query parameters of every URL in `text`.
///
/// `text` is read both as a value that is one URL and as free text: a
/// parameter after raw whitespace or a quote (`https://host/my tool?token=x`)
/// is found by the first reading only, and the URLs after the first one in an
/// argument list by the second.
pub fn url_query_param_names(text: &str) -> Vec<String> {
    [&WHOLE_URL_QUERY, &URL_QUERY]
        .into_iter()
        .flat_map(|url_query| url_query.captures_iter(text))
        .flat_map(|captures| {
            captures[2]
                .split('&')
                .map(query_param_name)
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
        })
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect()
}

/// `text` without the query parameters, of any URL in it, whose names say
/// they carry a credential ([`is_credential_query_param`]). The other
/// parameters are kept as written, so two URLs that select different things
/// stay different, and stripping twice changes nothing. For identifiers that
/// are stored or used as names; a lockfile refuses such URLs instead.
///
/// `text` is read as a value that is one URL, and then as free text; see
/// [`url_query_param_names`].
pub fn strip_credential_query_params(text: &str) -> Cow<'_, str> {
    if !text.contains("://") || !text.contains('?') {
        return Cow::Borrowed(text);
    }
    let mut out = Cow::Borrowed(text);
    for url_query in [&WHOLE_URL_QUERY, &URL_QUERY] {
        if let Some(stripped) = strip_credential_params_of(url_query, &out) {
            out = Cow::Owned(stripped);
        }
    }
    // The two readings split a text differently, so what one removes can
    // change a name the other reads. Reading again until neither finds one
    // takes quadratic time on a text built for it, so a text that still shows
    // a credential name loses its queries whole: that leaves none, and a
    // second strip changes nothing.
    if url_query_param_names(&out)
        .iter()
        .any(|name| is_credential_query_param(name))
    {
        let whole = WHOLE_URL_QUERY.replace(&out, "$1");
        out = Cow::Owned(URL_QUERY.replace_all(&whole, "$1").into_owned());
    }
    out
}

/// `text` without the credential parameters of the queries `url_query` finds
/// in it, or `None` when it finds none.
fn strip_credential_params_of(url_query: &regex::Regex, text: &str) -> Option<String> {
    let mut changed = false;
    let out = url_query.replace_all(text, |captures: &regex::Captures<'_>| {
        let kept = captures[2]
            .split('&')
            .filter(|pair| {
                let credential = is_credential_query_param(&query_param_name(pair));
                changed |= credential;
                !credential
            })
            .collect::<Vec<_>>();
        if kept.is_empty() {
            captures[1].to_string()
        } else {
            format!("{}?{}", &captures[1], kept.join("&"))
        }
    });
    changed.then(|| out.into_owned())
}

/// Replace the value of every query parameter, of any URL in `text`, whose
/// name says it carries a credential ([`is_credential_query_param`]) with
/// `[redacted]`, and the userinfo as [`redact_url_userinfo`] does.
///
/// `https://host/r.git?ref=main&private_token=t` becomes
/// `https://host/r.git?ref=main&private_token=[redacted]`. A token written
/// into a URL (a tool named by its URL, a config value) is never registered
/// with the [`Redactor`]. Unlike [`strip_credential_query_params`], which is
/// for stored names, this marks where a credential was for a reader.
///
/// A value runs to the next `&`, `#`, quote or whitespace, so what is glued to
/// the URL goes with it (the version of `cargo:https://host/r?token=t@1.0`): an
/// unencoded `@` can be part of a token. A control character cannot be part of
/// a URL, so the terminal style that ends a styled URL is kept.
pub fn redact_url_credentials(text: &str) -> Cow<'_, str> {
    redact_credentials(text, true)
}

/// [`redact_url_credentials`] for one command-line argument, which is one
/// URL: whitespace and quotes are part of it, and a value runs to the next `&`
/// or `#`. Redact each argument before joining them for display.
pub fn redact_url_credentials_in_arg(arg: &str) -> Cow<'_, str> {
    redact_credentials(arg, false)
}

fn redact_credentials(text: &str, whitespace_ends_url: bool) -> Cow<'_, str> {
    let userinfo = redact_userinfo(text, whitespace_ends_url);
    let url_query: &regex::Regex = if whitespace_ends_url {
        &URL_QUERY
    } else {
        &ARG_URL_QUERY
    };
    match redact_query_credentials(url_query, &userinfo) {
        Some(redacted) => Cow::Owned(redacted),
        None => userinfo,
    }
}

/// `text` with the values of the credential parameters of the queries
/// `url_query` finds in it replaced, or `None` when it finds none.
fn redact_query_credentials(url_query: &regex::Regex, text: &str) -> Option<String> {
    if !text.contains("://") || !text.contains('?') {
        return None;
    }
    let mut changed = false;
    let out = url_query.replace_all(text, |captures: &regex::Captures<'_>| {
        let query = &captures[2];
        let (query, rest) = query.split_at(query.find(char::is_control).unwrap_or(query.len()));
        let pairs = query
            .split('&')
            .map(|pair| {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                if matches!(value, "" | "[redacted]")
                    || !is_credential_query_param(&query_param_name(name))
                {
                    return pair.to_string();
                }
                changed = true;
                format!("{name}=[redacted]")
            })
            .collect::<Vec<_>>();
        format!("{}?{}{rest}", &captures[1], pairs.join("&"))
    });
    changed.then(|| out.into_owned())
}

/// Text that mise writes to the terminal or a file without going through the
/// logger, redacted the way the logger redacts every record: registered secrets,
/// then the credentials of every URL ([`redact_url_credentials`]).
pub fn redact_output(text: &str) -> String {
    let text = redact_global(text);
    match redact_url_credentials(&text) {
        Cow::Borrowed(_) => text,
        Cow::Owned(redacted) => redacted,
    }
}

/// `value` with every string in it, keys included, redacted the way the
/// logger redacts a record ([`redact_output`]), for JSON that mise prints.
/// Redacting the strings rather than the serialized text keeps the JSON valid
/// whatever a registered secret is (`false`, `"`, `:`).
pub fn redact_json(value: serde_json::Value) -> serde_json::Value {
    redact_json_with(value, &redact_output)
}

/// [`redact_json`] with `redact` applied to each string.
pub fn redact_json_with(
    value: serde_json::Value,
    redact: &dyn Fn(&str) -> String,
) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => Value::String(redact(&s)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|v| redact_json_with(v, redact))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (redact(&k), redact_json_with(v, redact)))
                .collect(),
        ),
        other => other,
    }
}

fn redact_userinfo(text: &str, whitespace_ends_url: bool) -> Cow<'_, str> {
    rewrite_userinfo(text, whitespace_ends_url, Userinfo::Redact)
}

#[derive(Clone, Copy)]
enum Userinfo {
    /// Replace it with `[redacted]`.
    Redact,
    /// Remove it and its `@`, keeping the user of an `ssh` URL.
    Strip,
}

/// The scheme of the URL whose `://` ends `head`.
fn trailing_scheme(head: &str) -> &str {
    let head = head.strip_suffix("://").unwrap_or(head);
    let start = head
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
        .map_or(0, |i| i + 1);
    &head[start..]
}

fn rewrite_userinfo(text: &str, whitespace_ends_url: bool, mode: Userinfo) -> Cow<'_, str> {
    if !text.contains("://") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut redacted = false;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        rest = tail;
        // The userinfo is everything before the authority's last `@`, so an
        // unencoded `@` in a password is covered.
        let end = tail
            .find(|c: char| {
                matches!(c, '/' | '?' | '#') || (whitespace_ends_url && c.is_whitespace())
            })
            .unwrap_or(tail.len());
        let Some(at) = tail[..end].rfind('@').filter(|&at| at > 0) else {
            continue;
        };
        match mode {
            Userinfo::Redact => {
                out.push_str("[redacted]");
                rest = &tail[at..];
                redacted = true;
            }
            Userinfo::Strip if trailing_scheme(head).to_ascii_lowercase().ends_with("ssh") => {
                if let Some((user, _password)) = tail[..at].split_once(':') {
                    out.push_str(user);
                    rest = &tail[at..];
                    redacted = true;
                }
            }
            Userinfo::Strip => {
                rest = &tail[at + 1..];
                redacted = true;
            }
        }
    }
    if !redacted {
        return Cow::Borrowed(text);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[derive(Default, Clone, Debug, serde::Deserialize)]
pub struct Redactions(pub IndexSet<String>);

/// A redactor that uses Aho-Corasick for efficient multi-pattern string replacement.
///
/// This is more efficient than iterating through patterns and calling `str::replace()`
/// for each one, especially when there are many patterns. Aho-Corasick finds all
/// matches in a single pass through the text - O(n + z) vs O(n * m). Overlapping
/// matches are merged, so no part of any registered value is printed.
#[derive(Clone)]
pub struct Redactor {
    patterns: Arc<IndexSet<String>>,
    automaton: Option<Arc<AhoCorasick>>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self {
            patterns: Arc::new(IndexSet::new()),
            automaton: None,
        }
    }
}

impl Redactor {
    /// Create a new redactor from a set of patterns to redact.
    pub fn new(patterns: impl IntoIterator<Item = String>) -> Self {
        let patterns: IndexSet<String> = patterns.into_iter().filter(|p| !p.is_empty()).collect();
        let automaton = if patterns.is_empty() {
            None
        } else {
            // Build the Aho-Corasick automaton - O(m) where m is total pattern length
            AhoCorasick::new(patterns.iter()).ok().map(Arc::new)
        };
        Self {
            patterns: Arc::new(patterns),
            automaton,
        }
    }

    /// Create a new redactor by adding more patterns to an existing one.
    pub fn with_additional(&self, additional: impl IntoIterator<Item = String>) -> Self {
        let mut patterns = (*self.patterns).clone();
        for p in additional {
            if !p.is_empty() {
                patterns.insert(p);
            }
        }
        Self::new(patterns)
    }

    /// Returns the patterns as an Arc for efficient sharing.
    pub fn patterns_arc(&self) -> Arc<IndexSet<String>> {
        Arc::clone(&self.patterns)
    }

    /// Redact all matching patterns in the input string, replacing them with `[redacted]`.
    ///
    /// Every occurrence of every pattern is found, including ones that overlap each
    /// other, and overlapping matches are merged into one `[redacted]`. Choosing a
    /// single match per position would leak the tail of a secret that crosses
    /// another one. This is O(n + z) where n is the input length and z is the
    /// number of matches.
    pub fn redact(&self, input: &str) -> String {
        if self.patterns.is_empty() {
            return input.to_string();
        }
        replace_spans(input, self.match_spans(input.as_bytes()))
    }

    /// Every occurrence of every pattern in `haystack`, overlapping ones included, as
    /// `(start, end)` byte offsets: exactly the matches [`Self::redact`] replaces. In valid
    /// UTF-8 every offset is a character boundary, since every pattern is a string.
    pub fn match_spans(&self, haystack: &[u8]) -> Vec<(usize, usize)> {
        match &self.automaton {
            Some(ac) => ac
                .find_overlapping_iter(haystack)
                .map(|m| (m.start(), m.end()))
                .collect(),
            // Fallback if the automaton failed to build
            None => naive_spans(haystack, &self.patterns),
        }
    }

    /// The length in bytes of the longest pattern, 0 when there is none.
    pub fn longest_pattern(&self) -> usize {
        self.patterns.iter().map(String::len).max().unwrap_or(0)
    }
}

/// Every occurrence of every pattern, including self-overlapping ones.
fn naive_spans(input: &[u8], patterns: &IndexSet<String>) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    for p in patterns {
        let p = p.as_bytes();
        spans.extend(
            input
                .windows(p.len())
                .enumerate()
                .filter(|(_, window)| *window == p)
                .map(|(start, _)| (start, start + p.len())),
        );
    }
    spans
}

/// Replace each merged span with `[redacted]`. Spans that only touch stay separate.
fn replace_spans(input: &str, mut spans: Vec<(usize, usize)>) -> String {
    spans.sort_unstable();
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    let mut spans = spans.into_iter();
    let Some((mut cur_start, mut cur_end)) = spans.next() else {
        return input.to_string();
    };
    for (start, end) in spans {
        if start < cur_end {
            cur_end = cur_end.max(end);
        } else {
            out.push_str(&input[pos..cur_start]);
            out.push_str("[redacted]");
            pos = cur_end;
            (cur_start, cur_end) = (start, end);
        }
    }
    out.push_str(&input[pos..cur_start]);
    out.push_str("[redacted]");
    out.push_str(&input[cur_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_url_credentials_in_value() {
        // A value that is one URL may hold a raw space in its password.
        assert_eq!(
            strip_url_credentials_in_value("https://user:my password@gems.example.com/"),
            "https://gems.example.com/"
        );
        // An argument list is free text: whitespace ends each URL.
        assert_eq!(
            strip_url_credentials_in_value(
                "--index-url https://u:p@h.example/x --other https://h2/y"
            ),
            "--index-url https://h.example/x --other https://h2/y"
        );
        assert_eq!(strip_url_credentials_in_value("plain value"), "plain value");
    }

    #[test]
    fn test_strip_credential_query_params() {
        let cases = [
            (
                "cargo:https://host/r.git?token=s3cret",
                "cargo:https://host/r.git",
            ),
            (
                "http:https://host/t.tgz?ref=main&private_token=s3cret&page=2",
                "http:https://host/t.tgz?ref=main&page=2",
            ),
            ("https://host/t?api%5Fkey=s3cret&x=1", "https://host/t?x=1"),
            ("https://host/t?ref=main", "https://host/t?ref=main"),
            ("not a url?token=x", "not a url?token=x"),
            (
                "https://host/t.tgz?accessToken=s3cret&v=1",
                "https://host/t.tgz?v=1",
            ),
        ];
        for (input, want) in cases {
            let got = strip_credential_query_params(input);
            assert_eq!(got, want, "{input}");
            assert_eq!(strip_credential_query_params(&got), got, "idempotent");
        }
        assert!(matches!(
            strip_credential_query_params("https://host/t?ref=main"),
            Cow::Borrowed(_)
        ));
        assert_eq!(
            url_query_param_names("https://h/t?api%5Fkey=1&b"),
            ["api_key", "b"]
        );
    }

    /// A value that is one URL keeps raw whitespace and quotes in its path and
    /// query, so a credential after one is still found; an argument list is
    /// still read URL by URL.
    #[test]
    fn test_strip_credential_query_params_after_a_space_or_quote() {
        let cases = [
            ("https://host/my tool?token=s3cret", "https://host/my tool"),
            (
                "cargo:https://host/bob's?token=s3cret",
                "cargo:https://host/bob's",
            ),
            (
                "https://host/t?file=my tool&token=s3cret&page=2",
                "https://host/t?file=my tool&page=2",
            ),
            (
                "https://host/t-{{ version }}.tgz?access_token=s3cret",
                "https://host/t-{{ version }}.tgz",
            ),
            (" https://host/a\"b?to\tken=s3cret", " https://host/a\"b"),
            ("https://host/t?v=1&\ntoken=s3cret", "https://host/t?v=1"),
            (
                "--index-url https://h/s?token=s3cret --pre",
                "--index-url https://h/s --pre",
            ),
            (
                "--a https://host/t?x=1 --b 'https://host/u?token=s3cret' --c",
                "--a https://host/t?x=1 --b 'https://host/u' --c",
            ),
            ("https://host/t?x& a://b_token?key", "https://host/t?x"),
        ];
        for (input, want) in cases {
            let got = strip_credential_query_params(input);
            assert_eq!(got, want, "{input:?}");
            assert_eq!(strip_credential_query_params(&got), got, "idempotent");
        }
        for text in [
            "https://h/my tool?token=1",
            "https://h/bob's?v=1&token=1",
            "https://h/t?file=my tool&token=1",
            "https://h/t?file=a\nb&to\tken=1",
            "--a https://h/t --b \"https://h/u?token=1\"",
        ] {
            let names = url_query_param_names(text);
            assert!(names.contains(&"token".to_string()), "{text:?}: {names:?}");
        }
    }

    #[test]
    fn test_credential_query_params_in_any_naming_style() {
        for name in [
            "token",
            "access_token",
            "access-token",
            "accessToken",
            "ACCESSTOKEN",
            "authtoken",
            "authToken",
            "privateToken",
            "AUTH",
            "Authorization",
            "bearer",
            "API_KEY",
            "apiKey",
            "APIKey",
            "APIkey",
            "x-api-key",
            "key",
            "sig",
            "clientSecret",
            "secretKey",
            "jwt",
            "password",
            "X-Amz-Signature",
            "X-Amz-Credential",
            "X-Amz-Security-Token",
            "x-goog-credential",
            "_token",
            "to\tken",
        ] {
            assert!(is_credential_query_param(name), "{name:?}");
        }
        for name in [
            "", "ref", "page", "version", "keyword", "keys", "monkey", "tokens", "author",
            "passing", "signed",
        ] {
            assert!(!is_credential_query_param(name), "{name:?}");
        }
    }

    #[test]
    fn test_redact_output_hides_url_query_credentials() {
        assert_eq!(
            redact_output("cargo:https://u:p@host/r.git?ref=main&private_token=TOKEN failed"),
            "cargo:https://[redacted]@host/r.git?ref=main&private_token=[redacted] failed"
        );
    }

    #[test]
    fn test_redact_url_credentials() {
        for (input, expected) in [
            (
                "GET https://host/t.tgz?ref=main&token=TOKEN&page=2 failed",
                "GET https://host/t.tgz?ref=main&token=[redacted]&page=2 failed",
            ),
            // The name is matched decoded and kept as written.
            (
                "https://host/t?api%5Fkey=TOKEN&x=1",
                "https://host/t?api%5Fkey=[redacted]&x=1",
            ),
            (
                "https://bucket.example/t.tgz?X-Amz-Signature=TOKEN#frag",
                "https://bucket.example/t.tgz?X-Amz-Signature=[redacted]#frag",
            ),
            // An unencoded `@` can be part of a token, so the version that
            // follows a tool name goes with it.
            (
                "cargo:https://host/r.git?token=TOKEN@tag:1.0.0  installed",
                "cargo:https://host/r.git?token=[redacted]  installed",
            ),
            // A terminal style ends the URL.
            (
                "\u{1b}[34mcargo:https://host/r.git?token=TOKEN\u{1b}[0m@1.0.0",
                "\u{1b}[34mcargo:https://host/r.git?token=[redacted]\u{1b}[0m@1.0.0",
            ),
            (
                r#"{"url":"https://host/t?accessToken=TOKEN","n":1}"#,
                r#"{"url":"https://host/t?accessToken=[redacted]","n":1}"#,
            ),
            (
                "https://a.example/x?token=TOKEN https://b.example/y?access_token=TOKEN",
                "https://a.example/x?token=[redacted] https://b.example/y?access_token=[redacted]",
            ),
            (
                "https://host/t?ref=main&id=2",
                "https://host/t?ref=main&id=2",
            ),
            ("https://host/t?token=&token", "https://host/t?token=&token"),
            ("not a url?token=x", "not a url?token=x"),
        ] {
            let redacted = redact_url_credentials(input);
            assert_eq!(redacted, expected, "{input}");
            assert!(!redacted.contains("TOKEN"));
            assert_eq!(redact_url_credentials(&redacted), redacted, "idempotent");
        }
        assert!(matches!(
            redact_url_credentials("https://host/t?ref=main&id=2"),
            Cow::Borrowed(_)
        ));
        // One argument is one URL: a space does not end the token.
        assert_eq!(
            redact_url_credentials_in_arg("https://u:p w@host/my tool?token=TO KEN&v=1"),
            "https://[redacted]@host/my tool?token=[redacted]&v=1"
        );
    }

    #[test]
    fn test_secret_patterns_cover_shell_quoted_spelling() {
        for secret in ["it's s3cret", "two\nl'ines", r#"{"k":"it's"}"#] {
            let r = Redactor::new(secret_patterns(secret));
            assert_eq!(
                r.redact(&format!("login {}", shell_words::quote(secret))),
                "login '[redacted]'",
                "{secret:?}"
            );
        }
        assert_eq!(secret_patterns("abc&123 %"), vec!["abc&123 %"]);
    }

    #[test]
    fn test_strip_url_credentials() {
        let cases = [
            ("https://user:tok@host/a?b=1", "https://host/a?b=1"),
            ("https://tok@host", "https://host"),
            ("https://u:p@ss@host/x", "https://host/x"),
            (
                "cargo:https://bot:t0ken@host/acme/tool.git",
                "cargo:https://host/acme/tool.git",
            ),
            (
                "ssh://git@github.com/o/r.git",
                "ssh://git@github.com/o/r.git",
            ),
            (
                "git+ssh://git:pw@github.com/o/r",
                "git+ssh://git@github.com/o/r",
            ),
            (
                "--index-url https://u:p@h.example/x --other https://h2/y",
                "--index-url https://h.example/x --other https://h2/y",
            ),
            ("https://host/a@b", "https://host/a@b"),
            ("plain value", "plain value"),
        ];
        for (input, want) in cases {
            assert_eq!(strip_url_credentials(input), want, "{input}");
        }
        assert!(matches!(
            strip_url_credentials("https://host/x"),
            Cow::Borrowed(_)
        ));
        assert_eq!(
            strip_url_credentials_in_arg("cargo:https://bot:my t0ken@host/r"),
            "cargo:https://host/r"
        );
    }

    /// A registered secret that happens to be JSON syntax (`false`) must not
    /// turn into `[redacted]` outside a string.
    #[test]
    fn test_redact_json_redacts_strings_not_syntax() {
        let redactor = Redactor::new(["false".to_string(), "s3cret".to_string()]);
        let redact = |s: &str| redact_url_userinfo(&redactor.redact(s)).into_owned();
        let value = serde_json::json!({
            "activated": false,
            "env": {"TOKEN": "s3cret", "s3cret": "x"},
            "urls": ["https://u:p@host/x"],
        });
        let out = serde_json::to_string(&redact_json_with(value, &redact)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["activated"], false);
        assert_eq!(parsed["env"]["TOKEN"], "[redacted]");
        assert_eq!(parsed["env"]["[redacted]"], "x");
        assert_eq!(parsed["urls"][0], "https://[redacted]@host/x");
    }

    #[test]
    fn test_redact_output_hides_url_userinfo() {
        assert_eq!(
            redact_output("GET https://u:p@host/x failed"),
            "GET https://[redacted]@host/x failed"
        );
    }

    #[test]
    fn test_secret_patterns_single_line() {
        assert_eq!(secret_patterns("abc123"), vec!["abc123"]);
        assert!(secret_patterns("").is_empty());
    }

    #[test]
    fn test_secret_patterns_pem_lines() {
        let pem = "-----BEGIN KEY-----\nMIIEvQIBADAN\nZm9v\nabc\n}\n-----END KEY-----";
        let p = secret_patterns(pem);
        assert!(p.contains(&pem.to_string()));
        assert!(p.contains(&"Zm9v".to_string()));
        assert!(p.contains(&"MIIEvQIBADAN".to_string()));
        assert!(!p.contains(&"abc".to_string()));
        assert!(!p.contains(&"}".to_string()));
        let dashes = secret_patterns("-----\n-----\nxx");
        assert!(!dashes.contains(&"-----".to_string()));
    }

    #[test]
    fn test_secret_patterns_cover_rust_debug_spelling() {
        let secret = "ab\u{1}cd\"e";
        let r = Redactor::new(secret_patterns(secret));
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("\\u{1}"));
        assert_eq!(r.redact(&format!("args={rendered}")), "args=\"[redacted]\"");
    }

    #[test]
    fn test_secret_patterns_json_escaped_form() {
        let p = secret_patterns("a\nb\"c");
        assert!(p.contains(&"a\\nb\\\"c".to_string()));
    }

    #[test]
    fn test_inherited_values_that_overlap_leave_no_residue() {
        let keys: BTreeSet<String> = ["A", "B"].map(String::from).into();
        let get = |k: &str| Some(if k == "A" { "a" } else { "abcdef" }.to_string());
        let r = Redactor::new(inherited_secret_patterns(&keys, &get));
        assert!(!r.redact("x abcdef y").contains("bcdef"));
    }

    #[test]
    fn test_inherited_secret_patterns_use_injected_getter() {
        let keys: BTreeSet<String> = ["A", "B"].map(String::from).into();
        let get = |k: &str| (k == "A").then(|| "value-a".to_string());
        assert_eq!(inherited_secret_patterns(&keys, &get), vec!["value-a"]);
    }

    #[test]
    fn test_empty_redactor() {
        let r = Redactor::default();
        assert_eq!(r.redact("hello world"), "hello world");
    }

    #[test]
    fn test_single_pattern() {
        let r = Redactor::new(["secret".to_string()]);
        assert_eq!(r.redact("my secret value"), "my [redacted] value");
    }

    #[test]
    fn test_multiple_patterns() {
        let r = Redactor::new(["secret".to_string(), "password".to_string()]);
        assert_eq!(
            r.redact("secret and password here"),
            "[redacted] and [redacted] here"
        );
    }

    #[test]
    fn test_overlapping_patterns() {
        let r = Redactor::new(["abc".to_string(), "bc".to_string()]);
        let result = r.redact("abcd");
        // Should replace "abc" first, leaving "d"
        assert_eq!(result, "[redacted]d");
    }

    #[test]
    fn test_multiple_occurrences() {
        let r = Redactor::new(["token".to_string()]);
        assert_eq!(r.redact("token1 and token2"), "[redacted]1 and [redacted]2");
    }

    #[test]
    fn test_with_additional() {
        let r1 = Redactor::new(["secret".to_string()]);
        let r2 = r1.with_additional(["password".to_string()]);

        assert_eq!(r1.redact("secret password"), "[redacted] password");
        assert_eq!(r2.redact("secret password"), "[redacted] [redacted]");
    }

    /// The same patterns through the automaton and through the naive fallback.
    fn redactors(patterns: &[&str]) -> [Redactor; 2] {
        let r = Redactor::new(patterns.iter().map(|p| p.to_string()));
        let fallback = Redactor {
            patterns: r.patterns.clone(),
            automaton: None,
        };
        [r, fallback]
    }

    fn permutations<'a>(items: &[&'a str]) -> Vec<Vec<&'a str>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = vec![];
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let first = rest.remove(i);
            for mut p in permutations(&rest) {
                p.insert(0, first);
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn test_longest_match_wins() {
        for patterns in [["xxabcSECRETPART", "abc"], ["abc", "xxabcSECRETPART"]] {
            for r in redactors(&patterns) {
                assert_eq!(r.redact("token=xxabcSECRETPART"), "token=[redacted]");
                assert_eq!(r.redact("abc alone"), "[redacted] alone");
            }
        }
    }

    #[test]
    fn test_longest_match_prefix_pair() {
        for r in redactors(&["secret", "secret-long"]) {
            assert_eq!(r.redact("secret-long secret"), "[redacted] [redacted]");
        }
    }

    #[test]
    fn test_crossing_values_are_fully_redacted() {
        for patterns in permutations(&["abcdefgh", "cd", "efghSECRET"]) {
            for r in redactors(&patterns) {
                assert_eq!(r.redact("abcdefghSECRET"), "[redacted]", "{patterns:?}");
            }
        }
        for patterns in permutations(&["abc", "bcde"]) {
            for r in redactors(&patterns) {
                assert_eq!(r.redact("abcde"), "[redacted]", "{patterns:?}");
                assert_eq!(r.redact("xabcdey"), "x[redacted]y", "{patterns:?}");
            }
        }
    }

    #[test]
    fn test_self_overlapping_value() {
        for r in redactors(&["aa"]) {
            assert_eq!(r.redact("aaa"), "[redacted]");
        }
    }

    #[test]
    fn test_adjacent_values_stay_separate() {
        for r in redactors(&["abc", "def"]) {
            assert_eq!(r.redact("abcdef"), "[redacted][redacted]");
        }
        for r in redactors(&["secret"]) {
            assert_eq!(r.redact("secretsecret"), "[redacted][redacted]");
        }
    }

    #[test]
    fn test_non_ascii_input() {
        for r in redactors(&["sécret"]) {
            assert_eq!(r.redact("é sécret é"), "é [redacted] é");
        }
        for r in redactors(&["éé"]) {
            assert_eq!(r.redact("ééé"), "[redacted]");
        }
    }

    #[test]
    fn test_value_lines_and_json_escaped_form() {
        let v = "aaaaaaaaaa\nshort";
        for r in redactors(&[v, "aaaaaaaaaa", "short", "aaaaaaaaaa\\nshort"]) {
            assert_eq!(r.redact(v), "[redacted]");
            assert_eq!(
                r.redact(r#"{"k":"aaaaaaaaaa\nshort"}"#),
                r#"{"k":"[redacted]"}"#
            );
        }
    }

    #[test]
    fn test_with_additional_keeps_longest_wins() {
        let r = Redactor::new(["abc".to_string()]).with_additional(["xxabcSECRETPART".to_string()]);
        assert_eq!(r.redact("token=xxabcSECRETPART"), "token=[redacted]");
        let r = r.with_additional(["cSECRETPARTzz".to_string()]);
        assert_eq!(r.redact("xxabcSECRETPARTzz"), "[redacted]");
    }

    #[test]
    fn test_empty_patterns_filtered() {
        let r = Redactor::new(["".to_string(), "secret".to_string(), "".to_string()]);
        assert_eq!(r.patterns_arc().len(), 1);
        assert_eq!(r.redact("my secret"), "my [redacted]");
    }

    #[test]
    fn test_redact_url_userinfo() {
        for (input, expected) in [
            (
                "GET https://user:TOKEN@git.example.com/api/v1",
                "GET https://[redacted]@git.example.com/api/v1",
            ),
            ("https://TOKEN@host?q=1", "https://[redacted]@host?q=1"),
            ("https://u:p@host", "https://[redacted]@host"),
            // An unencoded `@` in the password stays inside the redaction.
            ("https://u:p@ss@host/x", "https://[redacted]@host/x"),
            // An `@` after the authority is not userinfo.
            ("https://host/a@b", "https://host/a@b"),
            ("https://host?email=a@b", "https://host?email=a@b"),
            (
                "ARGS: mise x --url=https://u:p@a.example/x git+ssh://git:k@b.example",
                "ARGS: mise x --url=https://[redacted]@a.example/x git+ssh://[redacted]@b.example",
            ),
            (
                "error for url (https://u:p@host)",
                "error for url (https://[redacted]@host)",
            ),
            // In free text whitespace ends the URL, so a port is not userinfo.
            (
                "ARGS: mise x https://host:8080 --user me@example.com",
                "ARGS: mise x https://host:8080 --user me@example.com",
            ),
            (
                "see https://host and me@example.com",
                "see https://host and me@example.com",
            ),
            ("no url here", "no url here"),
            ("https://@host", "https://@host"),
        ] {
            let redacted = redact_url_userinfo(input);
            assert_eq!(redacted, expected, "{input}");
            assert!(!redacted.contains("TOKEN"));
        }
        assert!(matches!(
            redact_url_userinfo("https://host/path"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_redact_url_userinfo_in_arg() {
        for (input, expected) in [
            // A raw space in userinfo is part of the URL inside one argument.
            ("https://user:my TOKEN@host/x", "https://[redacted]@host/x"),
            ("https://user:my@TOKEN word@host", "https://[redacted]@host"),
            ("https://MY TOKEN@host", "https://[redacted]@host"),
            (
                "--url=https://u:p@host?q=a@b",
                "--url=https://[redacted]@host?q=a@b",
            ),
            ("https://host:8080", "https://host:8080"),
            ("me@example.com", "me@example.com"),
        ] {
            let redacted = redact_url_userinfo_in_arg(input);
            assert_eq!(redacted, expected, "{input}");
            assert!(!redacted.contains("TOKEN"));
        }
    }
}
