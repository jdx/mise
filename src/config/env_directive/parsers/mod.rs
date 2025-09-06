use eyre::bail;
use indexmap::IndexMap;
// no std::path imports needed

pub type EnvMap = IndexMap<String, String>;

pub fn parse_json_env(raw: &str) -> eyre::Result<EnvMap> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let mut out = EnvMap::new();
    if let serde_json::Value::Object(map) = v {
        for (k, v) in map {
            if k == "sops" {
                continue;
            }
            let s = match v {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => bail!("unsupported json value: {v:?}"),
            };
            out.insert(k, s);
        }
    }
    Ok(out)
}

pub fn parse_yaml_env(raw: &str) -> eyre::Result<EnvMap> {
    let v: serde_yaml::Value = serde_yaml::from_str(raw)?;
    let mut out = EnvMap::new();
    if let serde_yaml::Value::Mapping(map) = v {
        for (k, v) in map {
            let k = match k {
                serde_yaml::Value::String(s) => s,
                _ => continue,
            };
            if k == "sops" {
                continue;
            }
            let s = match v {
                serde_yaml::Value::String(s) => s,
                serde_yaml::Value::Number(n) => n.to_string(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                _ => bail!("unsupported yaml value: {v:?}"),
            };
            out.insert(k, s);
        }
    }
    Ok(out)
}

pub fn parse_toml_env(raw: &str) -> eyre::Result<EnvMap> {
    let v: toml::Value = toml::from_str(raw)?;
    let mut out = EnvMap::new();
    if let toml::Value::Table(map) = v {
        for (k, v) in map {
            if k == "sops" {
                continue;
            }
            let s = match v {
                toml::Value::String(s) => s,
                toml::Value::Integer(n) => n.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                _ => bail!("unsupported toml value: {v:?}"),
            };
            out.insert(k, s);
        }
    }
    Ok(out)
}

pub fn parse_dotenv_env(raw: &str) -> eyre::Result<EnvMap> {
    let mut out = EnvMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = if let Some(rest) = line.strip_prefix("export ") {
            rest.trim()
        } else {
            line
        };
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let mut val = line[eq + 1..].trim().to_string();
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                val = val[1..val.len() - 1].to_string();
            }
            out.insert(key, val);
        }
    }
    Ok(out)
}

pub fn parse_env_by_ext(ext: &str, raw: &str) -> eyre::Result<EnvMap> {
    match ext {
        "json" => parse_json_env(raw),
        "yaml" | "yml" => parse_yaml_env(raw),
        "toml" => parse_toml_env(raw),
        _ => parse_dotenv_env(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_env() {
        let raw = r#"{"FOO":"bar","N":1,"B":true}"#;
        let env = parse_json_env(raw).unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("N").unwrap(), "1");
        assert_eq!(env.get("B").unwrap(), "true");
    }

    #[test]
    fn test_parse_yaml_env() {
        let raw = "FOO: bar\nN: 1\nB: true\n";
        let env = parse_yaml_env(raw).unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("N").unwrap(), "1");
        assert_eq!(env.get("B").unwrap(), "true");
    }

    #[test]
    fn test_parse_toml_env() {
        let raw = "FOO=\"bar\"\nN=1\nB=true\n";
        let env = parse_toml_env(raw).unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("N").unwrap(), "1");
        assert_eq!(env.get("B").unwrap(), "true");
    }

    #[test]
    fn test_parse_dotenv_env() {
        let raw = "FOO=bar\nN=1\nB=true\nexport X=42\n# comment\n";
        let env = parse_dotenv_env(raw).unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("N").unwrap(), "1");
        assert_eq!(env.get("B").unwrap(), "true");
        assert_eq!(env.get("X").unwrap(), "42");
    }

    #[test]
    fn test_parse_env_by_ext() {
        assert!(parse_env_by_ext("json", "{}").is_ok());
        assert!(parse_env_by_ext("yaml", "").is_ok());
        assert!(parse_env_by_ext("toml", "").is_ok());
        assert!(parse_env_by_ext("dotenv", "").is_ok());
    }
}
