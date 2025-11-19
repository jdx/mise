use eyre::{bail, ContextCompat, Result};
use heck::ToTitleCase;
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Debug;
use versions::Versioning;

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
    Ident(&'a str),
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
            // Check if this is a property access (after ) or identifier)
            let next_char = code.chars().nth(1);
            if next_char.is_some_and(|c| c.is_alphabetic()) {
                // This could be .Key or .Property
                let end = code[1..]
                    .chars()
                    .enumerate()
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map(|(i, _)| i + 1)
                    .unwrap_or(code.len());

                // If preceded by RParen, it's a property access
                if tokens.last().is_some_and(|t| t.is_r_paren()) {
                    tokens.push(Token::Dot);
                    tokens.push(Token::Ident(&code[1..end]));
                } else {
                    // Otherwise it's a key reference
                    tokens.push(Token::Key(&code[1..end]));
                }
                code = &code[end..];
            } else {
                tokens.push(Token::Dot);
                code = &code[1..];
            }
        } else {
            // Check if it's an identifier (alphanumeric starting with letter)
            let end = code
                .chars()
                .enumerate()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_' && *c != '-')
                .map(|(i, _)| i)
                .unwrap_or(code.len());

            if end > 0 {
                let token_str = &code[..end];
                // Determine if this is a function or identifier based on context
                tokens.push(Token::Func(token_str));
                code = &code[end..];
            } else {
                bail!("unexpected character: {}", code.chars().next().unwrap());
            }
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
        let mut tokens = tokens.iter().peekable();
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
        let next_arg =
            |tokens: &mut std::iter::Peekable<std::slice::Iter<&Token>>| -> Result<String> {
                expect_whitespace(tokens.next())?;
                let arg = tokens.next().wrap_err("missing argument")?;
                match arg {
                    Token::Key(key) => {
                        if let Some(val) = self.ctx.get(*key) {
                            Ok(val.to_string())
                        } else {
                            bail!("unable to find key in context: {key}");
                        }
                    }
                    Token::String(str) => Ok(str.to_string()),
                    _ => bail!("expected key or string, found: {arg:?}"),
                }
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
                Token::LParen => {
                    // Handle (semver .Key).Property syntax
                    let func_token = tokens.next().wrap_err("expected function after (")?;
                    if let Token::Func(func) = func_token {
                        if *func == "semver" {
                            let arg = next_arg(&mut tokens)?;

                            // Expect )
                            if !matches!(tokens.next(), Some(Token::RParen)) {
                                bail!("expected ) after semver argument");
                            }

                            // Check for property access
                            if matches!(tokens.peek(), Some(Token::Dot)) {
                                tokens.next(); // consume dot
                                if let Some(Token::Ident(prop)) = tokens.next() {
                                    // Strip 'v' prefix if present before parsing
                                    let clean_version = arg.strip_prefix('v').unwrap_or(&arg);

                                    // Parse version and extract property
                                    let version =
                                        Versioning::new(clean_version).wrap_err_with(|| {
                                            format!("invalid semver version: {arg}")
                                        })?;

                                    s = match *prop {
                                        "Major" => version.nth(0).unwrap_or(0).to_string(),
                                        "Minor" => version.nth(1).unwrap_or(0).to_string(),
                                        "Patch" => version.nth(2).unwrap_or(0).to_string(),
                                        _ => bail!("unknown semver property: {prop}"),
                                    };
                                } else {
                                    bail!("expected identifier after dot");
                                }
                            } else {
                                // Return the whole version string
                                s = arg;
                            }
                        } else {
                            bail!("unexpected function in parentheses: {func}");
                        }
                    } else {
                        bail!("expected function after (");
                    }
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
                        _ => bail!("unexpected function: {func}"),
                    }
                    in_pipe = false
                }
                Token::Whitespace(_) => {}
                Token::Pipe => {
                    if in_pipe {
                        bail!("unexpected pipe token");
                    }
                    in_pipe = true;
                }
                Token::RParen | Token::Dot | Token::Ident(_) => {
                    bail!("unexpected token: {token:?}");
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

    #[test]
    fn test_render_semver_maven() {
        let tmpl = "https://archive.apache.org/dist/maven/maven-{{(semver .SemVer).Major}}/{{.SemVer}}/binaries/apache-maven-{{.SemVer}}-bin.tar.gz";
        let ctx = hashmap(vec![("SemVer", "3.9.11")]);
        assert_eq!(
            render(tmpl, &ctx).unwrap(),
            "https://archive.apache.org/dist/maven/maven-3/3.9.11/binaries/apache-maven-3.9.11-bin.tar.gz"
        );
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
        test_parse_semver_major: (r#"(semver .Version).Major"#, "3", vec![("Version", "3.9.11")]),
        test_parse_semver_minor: (r#"(semver .Version).Minor"#, "9", vec![("Version", "3.9.11")]),
        test_parse_semver_patch: (r#"(semver .Version).Patch"#, "11", vec![("Version", "3.9.11")]),
        test_parse_semver_major_v_prefix: (r#"(semver .Version).Major"#, "1", vec![("Version", "v1.2.3")]),
        test_parse_semver_no_property: (r#"(semver .Version)"#, "1.2.3", vec![("Version", "1.2.3")]),
    );

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
