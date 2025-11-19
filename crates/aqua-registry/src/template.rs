use eyre::{ContextCompat, Result, bail};
use heck::ToTitleCase;
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Debug;
use versions::Versioning;

type Context = HashMap<String, String>;

/// AST node representing an expression in the template
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    /// Variable reference: .Version
    Var(String),
    /// String literal: "foo"
    Literal(String),
    /// Function call: func arg1 arg2
    FuncCall(String, Vec<Expr>),
    /// Property access: expr.Property
    PropertyAccess(Box<Expr>, String),
    /// Pipe: expr | func
    Pipe(Box<Expr>, Box<Expr>),
}

pub fn render(tmpl: &str, ctx: &Context) -> Result<String> {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    let chars = tmpl.chars().collect_vec();
    let mut i = 0;
    let evaluator = Evaluator { ctx };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).cloned().unwrap_or(' ');
        if !in_tag && c == '{' && next == '{' {
            in_tag = true;
            i += 1;
        } else if in_tag && c == '}' && next == '}' {
            in_tag = false;
            let tokens = lex(&tag)?;
            let ast = parse_tokens(&tokens)?;
            result += &evaluator.eval(&ast)?;
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

/// Parse tokens into an AST
fn parse_tokens(tokens: &[Token]) -> Result<Expr> {
    let mut tokens = tokens.iter().peekable();
    parse_pipe(&mut tokens)
}

/// Parse pipe expressions (lowest precedence)
fn parse_pipe(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) -> Result<Expr> {
    let mut left = parse_primary(tokens)?;

    while matches!(tokens.peek(), Some(Token::Pipe)) {
        tokens.next(); // consume pipe
        skip_whitespace(tokens);
        let right = parse_primary(tokens)?;
        left = Expr::Pipe(Box::new(left), Box::new(right));
    }

    Ok(left)
}

/// Parse primary expressions
fn parse_primary(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) -> Result<Expr> {
    skip_whitespace(tokens);

    let token = tokens.next().wrap_err("unexpected end of expression")?;

    let mut expr = match token {
        Token::Key(k) => Expr::Var(k.to_string()),
        Token::String(s) => Expr::Literal(s.to_string()),
        Token::LParen => {
            // Parenthesized expression: (func arg)
            skip_whitespace(tokens);
            let inner = parse_pipe(tokens)?;
            skip_whitespace(tokens);
            if !matches!(tokens.next(), Some(Token::RParen)) {
                bail!("expected closing parenthesis");
            }
            inner
        }
        Token::Func(f) => {
            // Function call: func arg1 arg2
            let func_name = f.to_string();
            let mut args = Vec::new();

            // Collect arguments until we hit pipe, rparen, or end
            loop {
                skip_whitespace(tokens);

                match tokens.peek() {
                    None | Some(Token::Pipe) | Some(Token::RParen) => break,
                    Some(Token::Dot) | Some(Token::Ident(_)) => break, // Stop before property access
                    _ => {
                        args.push(parse_arg(tokens)?);
                    }
                }
            }

            Expr::FuncCall(func_name, args)
        }
        _ => bail!("unexpected token: {token:?}"),
    };

    // Handle property access: expr.Property
    while matches!(tokens.peek(), Some(Token::Dot)) {
        tokens.next(); // consume dot
        skip_whitespace(tokens);

        if let Some(Token::Ident(prop)) = tokens.next() {
            expr = Expr::PropertyAccess(Box::new(expr), prop.to_string());
        } else {
            bail!("expected identifier after dot");
        }
    }

    Ok(expr)
}

/// Parse a function argument
fn parse_arg(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) -> Result<Expr> {
    skip_whitespace(tokens);

    match tokens.peek() {
        Some(Token::LParen) => {
            tokens.next(); // consume lparen
            skip_whitespace(tokens);
            let expr = parse_pipe(tokens)?;
            skip_whitespace(tokens);
            if !matches!(tokens.next(), Some(Token::RParen)) {
                bail!("expected closing parenthesis");
            }

            // Check for property access after paren
            let mut result = expr;
            while matches!(tokens.peek(), Some(Token::Dot)) {
                tokens.next(); // consume dot
                skip_whitespace(tokens);
                if let Some(Token::Ident(prop)) = tokens.next() {
                    result = Expr::PropertyAccess(Box::new(result), prop.to_string());
                } else {
                    bail!("expected identifier after dot");
                }
            }
            Ok(result)
        }
        Some(Token::Key(k)) => {
            tokens.next();
            Ok(Expr::Var(k.to_string()))
        }
        Some(Token::String(s)) => {
            tokens.next();
            Ok(Expr::Literal(s.to_string()))
        }
        _ => bail!("expected argument"),
    }
}

fn skip_whitespace(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) {
    while matches!(tokens.peek(), Some(Token::Whitespace(_))) {
        tokens.next();
    }
}

/// Evaluator walks the AST and produces results
struct Evaluator<'a> {
    ctx: &'a Context,
}

impl Evaluator<'_> {
    /// Evaluate an AST node
    fn eval(&self, expr: &Expr) -> Result<String> {
        match expr {
            Expr::Var(name) => self
                .ctx
                .get(name)
                .map(|s| s.to_string())
                .wrap_err_with(|| format!("variable not found: {name}")),
            Expr::Literal(s) => Ok(s.clone()),
            Expr::FuncCall(func, args) => self.eval_func(func, args),
            Expr::PropertyAccess(expr, prop) => self.eval_property(expr, prop),
            Expr::Pipe(left, right) => {
                let left_val = self.eval(left)?;
                self.eval_with_input(right, &left_val)
            }
        }
    }

    /// Evaluate an expression with a piped input value
    fn eval_with_input(&self, expr: &Expr, input: &str) -> Result<String> {
        match expr {
            Expr::FuncCall(func, args) => {
                // For piped functions, prepend the input as first argument
                let mut full_args = vec![Expr::Literal(input.to_string())];
                full_args.extend(args.iter().cloned());
                self.eval_func(func, &full_args)
            }
            _ => bail!("can only pipe to function calls"),
        }
    }

    /// Evaluate property access
    fn eval_property(&self, expr: &Expr, prop: &str) -> Result<String> {
        let base = self.eval(expr)?;

        // Check if this is semver property access
        let clean_version = base.strip_prefix('v').unwrap_or(&base);
        let version = Versioning::new(clean_version)
            .wrap_err_with(|| format!("invalid semver version: {base}"))?;

        Ok(match prop {
            "Major" => version.nth(0).unwrap_or(0).to_string(),
            "Minor" => version.nth(1).unwrap_or(0).to_string(),
            "Patch" => version.nth(2).unwrap_or(0).to_string(),
            _ => bail!("unknown property: {prop}"),
        })
    }

    /// Evaluate a function call
    fn eval_func(&self, func: &str, args: &[Expr]) -> Result<String> {
        match func {
            "semver" => {
                if args.len() != 1 {
                    bail!("semver requires exactly 1 argument");
                }
                let version = self.eval(&args[0])?;
                // Strip 'v' prefix if present
                Ok(version.strip_prefix('v').unwrap_or(&version).to_string())
            }
            "title" => {
                if args.len() != 1 {
                    bail!("title requires exactly 1 argument");
                }
                let s = self.eval(&args[0])?;
                Ok(s.to_title_case())
            }
            "trimV" => {
                if args.len() != 1 {
                    bail!("trimV requires exactly 1 argument");
                }
                let s = self.eval(&args[0])?;
                Ok(s.trim_start_matches('v').to_string())
            }
            "trimPrefix" => {
                if args.len() != 2 {
                    bail!("trimPrefix requires exactly 2 arguments");
                }
                let prefix = self.eval(&args[0])?;
                let s = self.eval(&args[1])?;
                Ok(s.strip_prefix(&prefix).unwrap_or(&s).to_string())
            }
            "trimSuffix" => {
                if args.len() != 2 {
                    bail!("trimSuffix requires exactly 2 arguments");
                }
                let suffix = self.eval(&args[0])?;
                let s = self.eval(&args[1])?;
                Ok(s.strip_suffix(&suffix).unwrap_or(&s).to_string())
            }
            "replace" => {
                if args.len() != 3 {
                    bail!("replace requires exactly 3 arguments");
                }
                let from = self.eval(&args[0])?;
                let to = self.eval(&args[1])?;
                let s = self.eval(&args[2])?;
                Ok(s.replace(&from, &to))
            }
            _ => bail!("unknown function: {func}"),
        }
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

    #[test]
    fn test_render_nested_semver_in_function() {
        // The semver function handles 'v' prefix internally, so (semver .Version).Major
        // correctly extracts "3" from "v3.9.11". Then trimV is called on "3" (no-op).
        let tmpl = "{{trimV (semver .Version).Major}}";
        let ctx = hashmap(vec![("Version", "v3.9.11")]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "3");
    }

    #[test]
    fn test_render_semver_handles_v_prefix() {
        // semver function automatically strips 'v' prefix - no need for trimV
        let tmpl = "{{semver .Version}}";
        let ctx = hashmap(vec![("Version", "v3.9.11")]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "3.9.11");
    }

    #[test]
    fn test_render_blender_url() {
        // Exact pattern from blender registry
        let tmpl = "https://download.blender.org/release/Blender{{(semver .Version).Major}}.{{(semver .Version).Minor}}/blender-{{trimV .Version}}-linux-x64.tar.xz";
        let ctx = hashmap(vec![
            ("Version", "4.3.2"),
            ("OS", "linux"),
            ("Arch", "x64"),
            ("Format", "tar.xz"),
        ]);
        let result = render(tmpl, &ctx).unwrap();
        assert_eq!(
            result,
            "https://download.blender.org/release/Blender4.3/blender-4.3.2-linux-x64.tar.xz"
        );
    }

    #[test]
    fn test_render_semver_as_function_arg() {
        let tmpl = "{{title (semver .Version).Major}}";
        let ctx = hashmap(vec![("Version", "3.9.11")]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "3");
    }

    #[test]
    fn test_lex_semver_with_property() {
        let tokens = lex("(semver .Version).Major").unwrap();
        // Should be: LParen, Func(semver), Whitespace, Key(Version), RParen, Dot, Ident(Major)
        assert!(
            tokens.len() >= 6,
            "Expected at least 6 tokens, got {}: {:?}",
            tokens.len(),
            tokens
        );
    }

    #[test]
    fn test_render_just_semver_paren() {
        let tmpl = "{{(semver .Version)}}";
        let ctx = hashmap(vec![("Version", "1.2.3")]);
        assert_eq!(render(tmpl, &ctx).unwrap(), "1.2.3");
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
        test_parse_nested_semver_in_trimv: (r#"trimV (semver .Version).Major"#, "3", vec![("Version", "v3.9.11")]),
        test_parse_nested_semver_in_title: (r#"title (semver .Version).Minor"#, "9", vec![("Version", "3.9.11")]),
        test_parse_semver_standalone: (r#"semver .Version"#, "1.2.3", vec![("Version", "v1.2.3")]),
        test_parse_semver_standalone_no_v: (r#"semver .Version"#, "1.2.3", vec![("Version", "1.2.3")]),
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
