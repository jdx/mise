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

fn parse(code: &str, ctx: &Context) -> String {
    if let Some(key) = code.strip_prefix(".") {
        if let Some(val) = ctx.get(key) {
            val.to_string()
        } else {
            warn!("unable to find key in context: {key}");
            "<ERR>".to_string()
        }
    } else if let Some(code) = code.strip_prefix('"') {
        for (end, _) in code.match_indices('"') {
            if end > 0 && code.chars().nth(end - 1) != Some('\\') {
                return code[..end].to_string();
            }
        }
        warn!("unterminated string: {code}");
        "<ERR>".to_string()
    } else if let Some(code) = code.strip_prefix("title ") {
        let code = parse(code, ctx);
        code.to_title_case()
    } else if let Some(code) = code.strip_prefix("trimV ") {
        let code = parse(code, ctx);
        code.trim_start_matches('v').to_string()
    } else if let Some(code) = code.strip_prefix("trimPrefix ") {
        // TODO: this would break on spaces, though spaces are probably unlikely for the sort of strings we're working with
        let prefix = parse(code.split_whitespace().next().unwrap(), ctx);
        let s = parse(code.split_whitespace().nth(1).unwrap(), ctx);
        if s.starts_with(&prefix) {
            s[prefix.len()..].to_string()
        } else {
            s.to_string()
        }
    } else {
        warn!("unable to parse aqua template: {code}");
        "<ERR>".to_string()
    }
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
        test_parse_key: (".OS", "world", hashmap!{"OS" => "world"}),
        test_parse_string: ("\"world\"", "world", hashmap!{}),
        test_parse3: ("XXX", "<ERR>", hashmap!{}),
        test_parse4: (r#"title "world""#, "World", hashmap!{}),
        test_parse5: (r#"trimV "v1.0.0""#, "1.0.0", hashmap!{}),
        test_parse6: (r#"trimPrefix "v" "v1.0.0""#, "1.0.0", hashmap!{}),
        test_parse7: (r#"trimPrefix "v" "1.0.0""#, "1.0.0", hashmap!{}),
    );
}
