use heck::ToTitleCase;
use itertools::Itertools;
use std::collections::HashMap;

type Context = HashMap<String, String>;

pub fn render(tmpl: &str, ctx: &Context) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    let chars = tmpl.chars().collect_vec();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).cloned().unwrap_or(' ');
        if !in_tag && c == '{' && next == '{' {
            in_tag = true;
            i += 1;
        } else if in_tag && c == '}' && next == '}' {
            in_tag = false;
            result += &parse(&tag, ctx);
            tag.clear();
            i += 1;
        } else if in_tag {
            tag.push(c);
        } else {
            result.push(c);
        }
        i += 1;
    }
    result
}

fn parse(mut code: &str, ctx: &Context) -> String {
    type Op = Box<dyn Fn(&str) -> String>;
    let mut ops: Vec<Op> = Vec::new();
    if code.starts_with("title ") {
        code = &code[6..];
        ops.push(Box::new(|s: &str| s.to_title_case()));
    }
    if code.starts_with("trimV ") {
        code = &code[6..];
        ops.push(Box::new(|s: &str| s.trim_start_matches('v').to_string()));
    }
    let mut val = if let Some(key) = code.strip_prefix(".") {
        ctx.get(key).unwrap().clone()
    } else if code.starts_with('"') && code.ends_with('"') {
        // TODO: handle quotes in the middle of code
        code[1..code.len() - 1].to_string()
    } else {
        warn!("unable to parse aqua template: {code}");
        "<ERR>".to_string()
    };

    for op in ops.into_iter().rev() {
        val = op(&val);
    }

    val
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashmap;

    #[test]
    fn test_render() {
        let tmpl = "Hello, {{.OS}}!";
        let mut ctx = HashMap::new();
        ctx.insert("OS".to_string(), "world".to_string());
        assert_eq!(render(tmpl, &ctx), "Hello, world!");
    }

    macro_rules! parse_tests {
    ($($name:ident: $value:expr,)*) => {
        $(
            #[test]
            fn $name() {
                let (input, expected, ctx): (&str, &str, HashMap<&str, &str>) = $value;
                let ctx = ctx.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                assert_eq!(expected, parse(input, &ctx));
            }
        )*
    }}

    parse_tests!(
        test_parse1: (".OS", "world", hashmap!{"OS" => "world"}),
        test_parse2: ("\"world\"", "world", hashmap!{}),
        test_parse3: ("XXX", "<ERR>", hashmap!{}),
        test_parse4: (r#"title "world""#, "World", hashmap!{}),
        test_parse5: (r#"trimV "v1.0.0""#, "1.0.0", hashmap!{}),
    );
}
