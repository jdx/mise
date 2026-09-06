// Homebrew-compatible bundle assessment and version comparison, adapted from
// https://github.com/Homebrew/brew/tree/main/Library/Homebrew
//
// BSD 2-Clause License
// Copyright (c) 2009-present, Homebrew contributors
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
// 1. Redistributions of source code must retain the above copyright notice,
//    this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

use std::cmp::Ordering::{self, Equal, Greater, Less};
use std::path::Path;
use std::sync::LazyLock;

use eyre::{bail, eyre};
use regex::Regex;

use super::open_trusted_directory;
use crate::result::Result;

pub(super) struct AppVersion {
    pub(super) short: Option<String>,
    pub(super) build: Option<String>,
}

pub(super) fn read_app_version(app: &Path) -> Result<AppVersion> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::{Mode, SFlag, fstat};

    let contents = app.join("Contents");
    let parent = open_trusted_directory(Path::new("/"), contents.strip_prefix("/")?, true, false)?;
    let fd = openat(
        &parent.fd,
        "Info.plist",
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
        Mode::empty(),
    )?;
    if SFlag::from_bits_truncate(fstat(&fd)?.st_mode) & SFlag::S_IFMT != SFlag::S_IFREG {
        bail!("app Info.plist must be a regular file");
    }
    let plist = plist::Value::from_reader(std::fs::File::from(fd))?;
    let dict = plist
        .as_dictionary()
        .ok_or_else(|| eyre!("app Info.plist must be a dictionary"))?;
    let field = |key| -> Result<Option<String>> {
        dict.get(key)
            .map(|value| {
                value
                    .as_string()
                    .map(str::to_owned)
                    .ok_or_else(|| eyre!("app {key} must be a string"))
            })
            .transpose()
    };
    Ok(AppVersion {
        short: field("CFBundleShortVersionString")?,
        build: field("CFBundleVersion")?,
    })
}

pub(super) fn app_version_outdated(
    current: &str,
    short: Option<&str>,
    build: Option<&str>,
) -> bool {
    if current == "latest" {
        return false;
    }
    let build = build.filter(|value| !value.trim().is_empty());
    let short = short.map(|short| {
        build
            .and_then(|build| short.strip_suffix(&format!("({build})")))
            .map(str::trim_end)
            .unwrap_or(short)
    });
    let usable = |value: &&str| !value.trim().is_empty() && !["0", "0.0"].contains(value);
    let short = short.filter(usable);
    let build = build.filter(usable);
    let candidates: Vec<_> = current.trim_end_matches(',').split(',').collect();
    let tap_short = candidates
        .first()
        .copied()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(current);
    if let (Some(short), Some(build)) = (short, build) {
        let combined = format!("{short}-{build}");
        let comparisons: Vec<_> = candidates
            .iter()
            .filter_map(|candidate| compare_app_versions(&combined, candidate))
            .collect();
        if comparisons.contains(&Equal)
            || (!comparisons.is_empty() && !comparisons.contains(&Less))
            || (comparisons.is_empty()
                && tap_short
                    .rsplit_once('-')
                    .is_some_and(|(prefix, _)| short == prefix))
        {
            return false;
        }
    }
    if [short, build]
        .into_iter()
        .flatten()
        .any(|value| compare_app_versions(value, tap_short) == Some(Equal))
    {
        return false;
    }
    match short.and_then(|short| compare_app_versions(short, tap_short)) {
        Some(Less) => return true,
        Some(Greater) => return false,
        _ => {}
    }
    let comparisons: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| build.and_then(|build| compare_app_versions(build, candidate)))
        .collect();
    !comparisons.contains(&Equal) && comparisons.contains(&Less)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Missing,
    Numeric,
    Alpha,
    Beta,
    Pre,
    Rc,
    Patch,
    Post,
    Text,
}

#[derive(Clone, Copy)]
struct Token<'a> {
    kind: TokenKind,
    text: &'a str,
}

impl Token<'_> {
    fn prerelease(self) -> bool {
        matches!(
            self.kind,
            TokenKind::Alpha | TokenKind::Beta | TokenKind::Pre | TokenKind::Rc
        )
    }

    fn revision(&self) -> &str {
        self.text
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .trim_start_matches('0')
    }

    fn compare(self, other: Self) -> Ordering {
        use TokenKind::*;
        match (self.kind, other.kind) {
            (Missing, Missing) => Equal,
            (Numeric, Numeric) => compare_digits(self.text, other.text),
            (Missing, Numeric) => {
                if other.revision().is_empty() {
                    Equal
                } else {
                    Less
                }
            }
            (Missing, _) => {
                if other.prerelease() {
                    Greater
                } else {
                    Less
                }
            }
            (_, Missing) => other.compare(self).reverse(),
            (Numeric, _) => Greater,
            (_, Numeric) => Less,
            (a, b) if a == b && a != Text => compare_digits(self.revision(), other.revision()),
            _ if self.prerelease() && other.prerelease() => {
                (self.kind as u8).cmp(&(other.kind as u8))
            }
            _ if self.prerelease() && matches!(other.kind, Patch | Post) => Less,
            _ if matches!(self.kind, Patch | Post) && other.prerelease() => Greater,
            _ => self.text.cmp(other.text),
        }
    }
}

fn compare_digits(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn tokens(version: &str) -> Vec<Token<'_>> {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            "(?i:(alpha[0-9]*|a[0-9]+)|(beta[0-9]*|b[0-9]+)|(pre[0-9]*)|",
            "(rc[0-9]*)|(p[0-9]*)|(.post[0-9]+)|([0-9]+)|([a-z]+))"
        ))
        .expect("valid Homebrew version token pattern")
    });
    let kinds = [
        TokenKind::Alpha,
        TokenKind::Beta,
        TokenKind::Pre,
        TokenKind::Rc,
        TokenKind::Patch,
        TokenKind::Post,
        TokenKind::Numeric,
        TokenKind::Text,
    ];
    PATTERN
        .captures_iter(version)
        .map(|captures| {
            let (kind, text) = kinds
                .iter()
                .enumerate()
                .find_map(|(index, kind)| {
                    captures
                        .get(index + 1)
                        .map(|matched| (*kind, matched.as_str()))
                })
                .expect("each version token has one matching kind");
            Token { kind, text }
        })
        .collect()
}

pub(super) fn compare_app_versions(first: &str, second: &str) -> Option<Ordering> {
    let dot_count = |value: &str| {
        let value = value.trim_end_matches('.');
        if value.is_empty() {
            0
        } else {
            value.split('.').count()
        }
    };
    if first.trim().is_empty() || second.trim().is_empty() || dot_count(first) != dot_count(second)
    {
        return None;
    }
    let head = |value: &str| value == "HEAD" || value.starts_with("HEAD-");
    if head(first) || head(second) {
        return Some(head(first).cmp(&head(second)));
    }
    let left = tokens(first);
    let right = tokens(second);
    let missing = Token {
        kind: TokenKind::Missing,
        text: "",
    };
    let (mut l, mut r) = (0, 0);
    while l < left.len() || r < right.len() {
        let a = left.get(l).copied().unwrap_or(missing);
        let b = right.get(r).copied().unwrap_or(missing);
        let ordering = a.compare(b);
        if ordering == Equal {
            l += 1;
            r += 1;
        } else if a.kind == TokenKind::Numeric && b.kind != TokenKind::Numeric {
            if a.compare(missing) == Greater {
                return Some(Greater);
            }
            l += 1;
        } else if a.kind != TokenKind::Numeric && b.kind == TokenKind::Numeric {
            if b.compare(missing) == Greater {
                return Some(Less);
            }
            r += 1;
        } else {
            return Some(ordering);
        }
    }
    Some(Equal)
}
