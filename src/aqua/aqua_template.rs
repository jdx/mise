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
}

fn lex(code: &str) -> Result<Vec<Token>> {
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
            let end = code.split_whitespace().next().unwrap().len();
            tokens.push(Token::Key(&code[1..end]));
            code = &code[end..];
        } else if code.starts_with("|") {
            tokens.push(Token::Pipe);
            code = &code[1..];
        } else {
            let func = code.split_whitespace().next().unwrap();
            tokens.push(Token::Func(func));
            code = &code[func.len()..];
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
        while let Some(token) = tokens.next() {
            match token {
                Token::Key(key) => {
                    if let Some(val) = self.ctx.get(*key) {
                        s = val.to_string()
                    } else {
                        bail!("unable to find key in context: {key}");
                    }
                }
                Token::String(str) => s = str.to_string(),
                Token::Func(func) => match *func {
                    "title" => {
                        let arg = next_arg(&mut tokens)?;
                        s = arg.to_title_case();
                    }
                    "trimV" => {
                        let arg = next_arg(&mut tokens)?;
                        s = arg.trim_start_matches('v').to_string();
                    }
                    "trimPrefix" => {
                        let prefix = next_arg(&mut tokens)?;
                        let str = next_arg(&mut tokens)?;
                        if let Some(str) = str.strip_prefix(&prefix) {
                            s = str.to_string();
                        } else {
                            s = str.to_string();
                        }
                    }
                    "trimSuffix" => {
                        let suffix = next_arg(&mut tokens)?;
                        let str = next_arg(&mut tokens)?;
                        if let Some(str) = str.strip_suffix(&suffix) {
                            s = str.to_string();
                        } else {
                            s = str.to_string();
                        }
                    }
                    "replace" => {
                        let from = next_arg(&mut tokens)?;
                        let to = next_arg(&mut tokens)?;
                        let str = next_arg(&mut tokens)?;
                        s = str.replace(&from, &to);
                    }
                    _ => bail!("unexpected function: {func}"),
                },
                Token::Whitespace(_) => {}
                Token::Pipe => {
                    let mut tokens = tokens.cloned().collect_vec();
                    let whitespace = Token::Whitespace(" ");
                    let str = Token::String(&s);
                    tokens.push(&whitespace);
                    tokens.push(&str);
                    return self.parse(tokens);
                }
            }
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;
    use crate::hashmap;

    #[tokio::test]
    async fn test_render() {
        let _config = Config::get().await.unwrap();
        let tmpl = "Hello, {{.OS}}!";
        let mut ctx = HashMap::new();
        ctx.insert("OS".to_string(), "world".to_string());
        assert_eq!(render(tmpl, &ctx).unwrap(), "Hello, world!");
    }

    macro_rules! parse_tests {
    ($($name:ident: $value:expr,)*) => {
        $(
            #[tokio::test]
            async fn $name() {
                let _config = Config::get().await.unwrap();
                let (input, expected, ctx): (&str, &str, HashMap<&str, &str>) = $value;
                let ctx = ctx.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                let parser = Parser { ctx: &ctx };
                let tokens = lex(input).unwrap();
                assert_eq!(expected, parser.parse(tokens.iter().collect()).unwrap());
            }
        )*
    }}

    parse_tests!(
        test_parse_key: (".OS", "world", hashmap!{"OS" => "world"}),
        test_parse_string: ("\"world\"", "world", hashmap!{}),
        test_parse_title: (r#"title "world""#, "World", hashmap!{}),
        test_parse_trimv: (r#"trimV "v1.0.0""#, "1.0.0", hashmap!{}),
        test_parse_trim_prefix: (r#"trimPrefix "v" "v1.0.0""#, "1.0.0", hashmap!{}),
        test_parse_trim_prefix2: (r#"trimPrefix "v" "1.0.0""#, "1.0.0", hashmap!{}),
        test_parse_trim_suffix: (r#"trimSuffix "-v1.0.0" "foo-v1.0.0""#, "foo", hashmap!{}),
        test_parse_pipe: (r#"trimPrefix "foo-" "foo-v1.0.0" | trimV"#, "1.0.0", hashmap!{}),
        test_parse_replace: (r#"replace "foo" "bar" "foo-bar""#, "bar-bar", hashmap!{}),
    );

    #[tokio::test]
    async fn test_parse_err() {
        let _config = Config::get().await.unwrap();
        let parser = Parser {
            ctx: &HashMap::new(),
        };
        let tokens = lex("foo").unwrap();
        assert!(parser.parse(tokens.iter().collect()).is_err());
    }

    #[tokio::test]
    async fn test_lex() {
        let _config = Config::get().await.unwrap();
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
