use eyre::{ContextCompat, Result, bail, eyre};
use heck::ToTitleCase;
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::LazyLock;
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

/// Runtime value trait - implemented by different value types
trait Value: Debug {
    fn as_string(&self) -> String;
    fn get_property(&self, prop: &str) -> Result<String>;
}

/// String value type
#[derive(Debug, Clone)]
struct StringValue(String);

impl Value for StringValue {
    fn as_string(&self) -> String {
        self.0.clone()
    }

    fn get_property(&self, _prop: &str) -> Result<String> {
        Err(eyre!("cannot access property on string"))
    }
}

/// Semantic version value type
#[derive(Debug, Clone)]
struct SemVerValue {
    major: u32,
    minor: u32,
    patch: u32,
    original: String,
}

impl Value for SemVerValue {
    fn as_string(&self) -> String {
        self.original.clone()
    }

    fn get_property(&self, prop: &str) -> Result<String> {
        Ok(match prop {
            "Major" => self.major.to_string(),
            "Minor" => self.minor.to_string(),
            "Patch" => self.patch.to_string(),
            _ => return Err(eyre!("unknown semver property: {prop}")),
        })
    }
}

pub(crate) fn render(tmpl: &str, ctx: &Context) -> Result<String> {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    let chars = tmpl.chars().collect_vec();
    let mut i = 0;
    let evaluator = Evaluator::new(ctx);
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
            if let Some(first_end) = dotted_identifier_end(code) {
                // This could be .Key or .Property, and can also include chained fields
                // like .Vars.channel.
                if tokens.last().is_some_and(|t| t.is_r_paren()) {
                    tokens.push(Token::Dot);
                    tokens.push(Token::Ident(&code[1..first_end]));
                } else {
                    tokens.push(Token::Key(&code[1..first_end]));
                }
                code = &code[first_end..];

                while let Some(end) = dotted_identifier_end(code) {
                    tokens.push(Token::Dot);
                    tokens.push(Token::Ident(&code[1..end]));
                    code = &code[end..];
                }
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

fn dotted_identifier_end(code: &str) -> Option<usize> {
    let rest = code.strip_prefix('.')?;
    let first = rest.chars().next()?;
    if !first.is_alphabetic() {
        return None;
    }
    Some(1 + identifier_len(rest))
}

fn identifier_len(code: &str) -> usize {
    code.char_indices()
        .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
        .map(|(i, _)| i)
        .unwrap_or(code.len())
}

/// Parse tokens into an AST
fn parse_tokens(tokens: &[Token]) -> Result<Expr> {
    let mut tokens = tokens.iter().peekable();
    parse_pipe(&mut tokens)
}

/// Parse pipe expressions (lowest precedence)
fn parse_pipe(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) -> Result<Expr> {
    let mut left = parse_primary(tokens)?;

    // Skip whitespace before checking for pipe
    skip_whitespace(tokens);
    while matches!(tokens.peek(), Some(Token::Pipe)) {
        tokens.next(); // consume pipe
        skip_whitespace(tokens);
        let right = parse_primary(tokens)?;
        left = Expr::Pipe(Box::new(left), Box::new(right));
        // Skip whitespace before checking for next pipe
        skip_whitespace(tokens);
    }

    Ok(left)
}

/// Parse primary expressions
fn parse_primary(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) -> Result<Expr> {
    skip_whitespace(tokens);

    let token = tokens.next().wrap_err("unexpected end of expression")?;

    let expr = match token {
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
        _ => return Err(eyre!("unexpected token: {token:?}")),
    };

    parse_property_chain(tokens, expr)
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
            parse_property_chain(tokens, expr)
        }
        Some(Token::Key(k)) => {
            tokens.next();
            parse_property_chain(tokens, Expr::Var(k.to_string()))
        }
        Some(Token::String(s)) => {
            tokens.next();
            Ok(Expr::Literal(s.to_string()))
        }
        _ => Err(eyre!("expected argument")),
    }
}

fn parse_property_chain(
    tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>,
    mut expr: Expr,
) -> Result<Expr> {
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

fn skip_whitespace(tokens: &mut std::iter::Peekable<std::slice::Iter<Token>>) {
    while matches!(tokens.peek(), Some(Token::Whitespace(_))) {
        tokens.next();
    }
}

/// Function signature for template functions that return Value trait objects
type TemplateFn = fn(&[Box<dyn Value>]) -> Result<Box<dyn Value>>;

/// Static registry of available template functions
static FUNCTION_REGISTRY: LazyLock<HashMap<&'static str, TemplateFn>> = LazyLock::new(|| {
    let mut registry: HashMap<&'static str, TemplateFn> = HashMap::new();

    registry.insert("semver", |args| {
        if args.len() != 1 {
            bail!("semver requires exactly 1 argument");
        }
        let input = args[0].as_string();
        let clean_version = input.strip_prefix('v').unwrap_or(&input);
        let version = Versioning::new(clean_version)
            .wrap_err_with(|| format!("invalid semver version: {input}"))?;

        Ok(Box::new(SemVerValue {
            major: version.nth(0).unwrap_or(0),
            minor: version.nth(1).unwrap_or(0),
            patch: version.nth(2).unwrap_or(0),
            original: clean_version.to_string(),
        }) as Box<dyn Value>)
    });

    registry.insert("title", |args| {
        if args.len() != 1 {
            bail!("title requires exactly 1 argument");
        }
        Ok(Box::new(StringValue(args[0].as_string().to_title_case())) as Box<dyn Value>)
    });

    registry.insert("trimV", |args| {
        if args.len() != 1 {
            bail!("trimV requires exactly 1 argument");
        }
        Ok(Box::new(StringValue(
            args[0].as_string().trim_start_matches('v').to_string(),
        )) as Box<dyn Value>)
    });

    registry.insert("trimPrefix", |args| {
        if args.len() != 2 {
            bail!("trimPrefix requires exactly 2 arguments");
        }
        let prefix = args[0].as_string();
        let text = args[1].as_string();
        Ok(Box::new(StringValue(
            text.strip_prefix(&prefix).unwrap_or(&text).to_string(),
        )) as Box<dyn Value>)
    });

    registry.insert("trimSuffix", |args| {
        if args.len() != 2 {
            bail!("trimSuffix requires exactly 2 arguments");
        }
        let suffix = args[0].as_string();
        let text = args[1].as_string();
        Ok(Box::new(StringValue(
            text.strip_suffix(&suffix).unwrap_or(&text).to_string(),
        )) as Box<dyn Value>)
    });

    registry.insert("replace", |args| {
        if args.len() != 3 {
            bail!("replace requires exactly 3 arguments");
        }
        let from = args[0].as_string();
        let to = args[1].as_string();
        let text = args[2].as_string();
        Ok(Box::new(StringValue(text.replace(&from, &to))) as Box<dyn Value>)
    });

    registry
});

/// Evaluator walks the AST and produces results
struct Evaluator<'a> {
    ctx: &'a Context,
}

impl<'a> Evaluator<'a> {
    fn new(ctx: &'a Context) -> Self {
        Self { ctx }
    }

    /// Evaluate an AST node and return a string (public interface)
    fn eval(&self, expr: &Expr) -> Result<String> {
        let value = self.eval_value(expr)?;
        Ok(value.as_string())
    }

    /// Evaluate an AST node and return a Value trait object (internal)
    fn eval_value(&self, expr: &Expr) -> Result<Box<dyn Value>> {
        match expr {
            Expr::Var(name) => {
                let s = self
                    .ctx
                    .get(name)
                    .wrap_err_with(|| format!("variable not found: {name}"))?;
                Ok(Box::new(StringValue(s.clone())) as Box<dyn Value>)
            }
            Expr::Literal(s) => Ok(Box::new(StringValue(s.clone())) as Box<dyn Value>),
            Expr::FuncCall(func, args) => self.eval_func(func, args),
            Expr::PropertyAccess(expr, prop) => self.eval_property(expr, prop),
            Expr::Pipe(left, right) => {
                let left_val = self.eval_value(left)?;
                self.eval_with_input(right, left_val)
            }
        }
    }

    /// Evaluate an expression with a piped input value
    fn eval_with_input(&self, expr: &Expr, input: Box<dyn Value>) -> Result<Box<dyn Value>> {
        match expr {
            Expr::FuncCall(func, args) => {
                // For piped functions, append the input as last argument
                let mut full_args = args.clone();
                full_args.push(Expr::Literal(input.as_string()));
                self.eval_func(func, &full_args)
            }
            _ => Err(eyre!("can only pipe to function calls")),
        }
    }

    /// Evaluate property access
    fn eval_property(&self, expr: &Expr, prop: &str) -> Result<Box<dyn Value>> {
        if let Expr::Var(name) = expr {
            let key = format!("{name}.{prop}");
            if let Some(value) = self.ctx.get(&key) {
                return Ok(Box::new(StringValue(value.clone())) as Box<dyn Value>);
            }
        }
        let value = self.eval_value(expr)?;
        let prop_value = value.get_property(prop)?;
        Ok(Box::new(StringValue(prop_value)) as Box<dyn Value>)
    }

    /// Evaluate a function call
    fn eval_func(&self, func: &str, args: &[Expr]) -> Result<Box<dyn Value>> {
        // Evaluate all arguments first
        let evaluated_args: Result<Vec<Box<dyn Value>>> =
            args.iter().map(|arg| self.eval_value(arg)).collect();
        let evaluated_args = evaluated_args?;

        // Look up function in registry
        if let Some(func_impl) = FUNCTION_REGISTRY.get(func) {
            func_impl(&evaluated_args)
        } else {
            Err(eyre!("unknown function: {func}"))
        }
    }
}

#[cfg(test)]
mod tests;
