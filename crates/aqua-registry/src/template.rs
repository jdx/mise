use eyre::{ContextCompat, Result, bail};
use heck::ToTitleCase;
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Debug;

type Context = HashMap<String, String>;

pub fn render(tmpl: &str, ctx: &Context) -> Result<String> {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    let chars = tmpl.chars().collect_vec();
    let mut i = 0;
    let parser = Parser { ctx };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).cloned().unwrap_or(' ');
        if !in_tag && c == '{' && next == '{' {
            in_tag = true;
            i += 1;
        } else if in_tag && c == '}' && next == '}' {
            in_tag = false;
            let tokens = lex(&tag)?;
            result += &parser.parse(tokens.iter().collect())?;
            tag.clear();
            i += 1;
        } else if in_tag {
            tag.push(c);
        } else {
            result.push(c);
        }
        i += 1;
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, strum::EnumIs)]
enum Token<'a> {
    Key(&'a str),
    String(&'a str),
    Func(&'a str),
    Whitespace(&'a str),
    Pipe,
    LParen,
    RParen,
    Dot,
}

fn lex(code: &str) -> Result<Vec<Token<'_>>> {
    let mut tokens = vec![];
    let mut code = code.trim();
    while !code.is_empty() {
        if code.starts_with(" ") {
            let end = code
                .chars()
                .enumerate()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(i, _)| i);
            if let Some(end) = end {
                tokens.push(Token::Whitespace(&code[..end]));
                code = &code[end..];
            } else {
                break;
            }
        } else if code.starts_with("(") {
            tokens.push(Token::LParen);
            code = &code[1..];
        } else if code.starts_with(")") {
            tokens.push(Token::RParen);
            code = &code[1..];
        } else if code.starts_with("|") {
            tokens.push(Token::Pipe);
            code = &code[1..];
        } else if code.starts_with('"') {
            for (end, _) in code[1..].match_indices('"') {
                if code.chars().nth(end) != Some('\\') {
                    tokens.push(Token::String(&code[1..end + 1]));
                    code = &code[end + 2..];
                    break;
                }
            }
        } else if code.starts_with(".") {
            // Dot followed by an identifier
            let end = code[1..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .count();
            if end > 0 {
                let name = &code[1..=end];
                // Check if previous token was RParen - that means this is field access
                let is_field_access = tokens.last().map(|t| t.is_r_paren()).unwrap_or(false);

                if is_field_access {
                    // Field access like ).Major
                    tokens.push(Token::Dot);
                    tokens.push(Token::Func(name));
                } else {
                    // Context variable like .Version or .OS
                    tokens.push(Token::Key(name));
                }
                code = &code[1 + end..];
            } else {
                bail!("unexpected . at end of input");
            }
        } else {
            let end = code
                .chars()
                .position(|c| c.is_whitespace() || c == '(' || c == ')' || c == '.')
                .unwrap_or(code.len());
            tokens.push(Token::Func(&code[..end]));
            code = &code[end..];
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    ctx: &'a Context,
}

impl Parser<'_> {
    fn parse(&self, tokens: Vec<&Token>) -> Result<String> {
        let mut s = String::new();
        let mut tokens = tokens.iter();
        let expect_whitespace = |t: Option<&&Token>| {
            if let Some(token) = t {
                if let Token::Whitespace(_) = token {
                    Ok(())
                } else {
                    bail!("expected whitespace, found: {token:?}");
                }
            } else {
                bail!("expected whitespace, found: end of input");
            }
        };
        let next_arg = |tokens: &mut std::slice::Iter<&Token>| -> Result<String> {
            expect_whitespace(tokens.next())?;
            let arg = tokens.next().wrap_err("missing argument")?;
            self.parse(vec![arg])
        };

        let mut in_pipe = false;
        while let Some(token) = tokens.next() {
            match token {
                Token::Key(key) => {
                    if in_pipe {
                        bail!("unexpected key token in pipe");
                    }
                    if let Some(val) = self.ctx.get(*key) {
                        s = val.to_string()
                    } else {
                        bail!("unable to find key in context: {key}");
                    }
                }
                Token::String(str) => {
                    if in_pipe {
                        bail!("unexpected string token in pipe");
                    }
                    s = str.to_string()
                }
                Token::Func(func) => {
                    match *func {
                        "title" | "trimV" => {
                            let arg = if in_pipe {
                                s.clone()
                            } else {
                                next_arg(&mut tokens)?
                            };
                            s = match *func {
                                "title" => arg.to_title_case(),
                                "trimV" => arg.trim_start_matches('v').to_string(),
                                _ => unreachable!(),
                            };
                        }
                        "trimPrefix" | "trimSuffix" => {
                            let param = next_arg(&mut tokens)?;
                            let input = if in_pipe {
                                s.clone()
                            } else {
                                next_arg(&mut tokens)?
                            };
                            s = match *func {
                                "trimPrefix" => {
                                    if let Some(str) = input.strip_prefix(&param) {
                                        str.to_string()
                                    } else {
                                        input.to_string()
                                    }
                                }
                                "trimSuffix" => {
                                    if let Some(str) = input.strip_suffix(&param) {
                                        str.to_string()
                                    } else {
                                        input.to_string()
                                    }
                                }
                                _ => unreachable!(),
                            };
                        }
                        "replace" => {
                            let from = next_arg(&mut tokens)?;
                            let to = next_arg(&mut tokens)?;
                            let str = if in_pipe {
                                s.clone()
                            } else {
                                next_arg(&mut tokens)?
                            };
                            s = str.replace(&from, &to);
                        }
                        "semver" => {
                            // Parse semver function matching sprig's semver behavior
                            // https://masterminds.github.io/sprig/semver.html
                            expect_whitespace(tokens.next())?;
                            let version_str = self
                                .parse(vec![tokens.next().wrap_err("semver missing argument")?])?;

                            // Remove 'v' prefix if present (semver crate doesn't handle it)
                            let version = version_str.trim_start_matches('v');
                            let ver = semver::Version::parse(version)
                                .map_err(|e| eyre::eyre!("invalid semver '{}': {}", version, e))?;

                            // Store parsed version components as "major|minor|patch"
                            // This will be used by field access operations (.Major, .Minor, .Patch)
                            s = format!("{}|{}|{}", ver.major, ver.minor, ver.patch);
                        }
                        "Major" | "Minor" | "Patch" => {
                            // Field access on semver result
                            // s should contain "major|minor|patch" from semver
                            let parts: Vec<&str> = s.split('|').collect();
                            if parts.len() == 3 {
                                s = match *func {
                                    "Major" => parts[0].to_string(),
                                    "Minor" => parts[1].to_string(),
                                    "Patch" => parts[2].to_string(),
                                    _ => unreachable!(),
                                };
                            } else {
                                bail!("field access .{func} requires semver result");
                            }
                        }
                        _ => bail!("unexpected function: {func}"),
                    }
                    in_pipe = false
                }
                Token::LParen => {
                    // Start of grouped expression - continue parsing
                }
                Token::RParen => {
                    // End of grouped expression - continue parsing
                }
                Token::Dot => {
                    // Dot for field access - the field name will come next as a Func token
                }
                Token::Whitespace(_) => {}
                Token::Pipe => {
                    if in_pipe {
                        bail!("unexpected pipe token");
                    }
                    in_pipe = true;
                }
            }
        }
        if in_pipe {
            bail!("unexpected end of input in pipe");
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashmap(data: Vec<(&str, &str)>) -> HashMap<String, String> {
        data.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_render() {
        let tmpl = "Hello, {{.OS}}!";
        let ctx = hashmap(vec![("OS", "world")]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "Hello, world!");
    }

    macro_rules! parse_tests {
    ($($name:ident: $value:expr,)*) => {
        $(
            #[test]
            fn $name() {
                let (input, expected, ctx_data): (&str, &str, Vec<(&str, &str)>) = $value;
                let ctx = hashmap(ctx_data);
                let parser = Parser { ctx: &ctx };
                let tokens = lex(input).unwrap();
                assert_eq!(expected, parser.parse(tokens.iter().collect()).unwrap());
            }
        )*
    }}

    parse_tests!(
        test_parse_key: (".OS", "world", vec![("OS", "world")]),
        test_parse_string: ("\"world\"", "world", vec![]),
        test_parse_title: (r#"title "world""#, "World", vec![]),
        test_parse_trimv: (r#"trimV "v1.0.0""#, "1.0.0", vec![]),
        test_parse_trim_prefix: (r#"trimPrefix "v" "v1.0.0""#, "1.0.0", vec![]),
        test_parse_trim_prefix2: (r#"trimPrefix "v" "1.0.0""#, "1.0.0", vec![]),
        test_parse_trim_suffix: (r#"trimSuffix "-v1.0.0" "foo-v1.0.0""#, "foo", vec![]),
        test_parse_pipe: (r#"trimPrefix "foo-" "foo-v1.0.0" | trimV"#, "1.0.0", vec![]),
        test_parse_multiple_pipes: (
            r#"trimPrefix "foo-" "foo-v1.0.0-beta" | trimSuffix "-beta" | trimV"#,
            "1.0.0",
            vec![],
        ),
        test_parse_replace: (r#"replace "foo" "bar" "foo-bar""#, "bar-bar", vec![]),

        // Semver tests matching sprig behavior
        test_semver_major: (r#"(semver "1.2.3").Major"#, "1", vec![]),
        test_semver_minor: (r#"(semver "1.2.3").Minor"#, "2", vec![]),
        test_semver_patch: (r#"(semver "1.2.3").Patch"#, "3", vec![]),
        test_semver_with_v_prefix: (r#"(semver "v1.2.3").Major"#, "1", vec![]),
        test_semver_with_prerelease: (r#"(semver "1.2.3-alpha").Major"#, "1", vec![]),
        test_semver_from_context: (r#"(semver .Version).Major"#, "2", vec![("Version", "2.5.8")]),
        test_semver_from_semver_var: (r#"(semver .SemVer).Major"#, "3", vec![("SemVer", "3.9.0")]),

        // Additional semver edge cases
        test_semver_major_version_10: (r#"(semver "10.5.2").Major"#, "10", vec![]),
        test_semver_with_build_metadata: (r#"(semver "1.2.3+build123").Major"#, "1", vec![]),
        test_semver_prerelease_complex: (r#"(semver "2.0.0-rc.1+build").Minor"#, "0", vec![]),
        test_semver_all_fields: (r#"(semver "4.5.6").Minor"#, "5", vec![]),
        test_semver_zero_major: (r#"(semver "0.1.0").Major"#, "0", vec![]),
        test_semver_zero_minor: (r#"(semver "1.0.5").Minor"#, "0", vec![]),
        test_semver_large_numbers: (r#"(semver "99.88.77").Patch"#, "77", vec![]),
    );

    // Real-world template patterns (like maven URL) - need render() for multiple {{...}} blocks
    #[test]
    fn test_semver_in_url_pattern() {
        let tmpl = r#"maven-{{(semver "3.9.0").Major}}/{{trimV "3.9.0"}}"#;
        let ctx = hashmap(vec![]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "maven-3/3.9.0");
    }

    #[test]
    fn test_semver_with_context_in_url() {
        let tmpl = r#"maven-{{(semver .Ver).Major}}/{{.Ver}}"#;
        let ctx = hashmap(vec![("Ver", "4.0.0-rc-5")]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "maven-4/4.0.0-rc-5");
    }

    #[test]
    fn test_parse_err() {
        let parser = Parser {
            ctx: &HashMap::new(),
        };
        let tokens = lex("foo").unwrap();
        assert!(parser.parse(tokens.iter().collect()).is_err());
    }

    #[test]
    fn test_lex() {
        assert_eq!(
            lex(r#"trimPrefix "foo-" "foo-v1.0.0" | trimV"#).unwrap(),
            vec![
                Token::Func("trimPrefix"),
                Token::Whitespace(" "),
                Token::String("foo-"),
                Token::Whitespace(" "),
                Token::String("foo-v1.0.0"),
                Token::Whitespace(" "),
                Token::Pipe,
                Token::Whitespace(" "),
                Token::Func("trimV"),
            ]
        );
    }
}
