use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvVarArg {
    pub key: String,
    pub value: Option<String>,
}

impl FromStr for EnvVarArg {
    type Err = eyre::Error;

    fn from_str(input: &str) -> eyre::Result<Self> {
        let ev = match input.split_once('=') {
            Some((k, v)) => Self {
                key: k.to_string(),
                value: Some(v.to_string()),
            },
            None => Self {
                key: input.to_string(),
                value: None,
            },
        };
        Ok(ev)
    }
}

#[cfg(test)]
mod tests {
    use super::EnvVarArg;
    use crate::test::reset;
    use test_log::test;

    #[test(tokio::test)]
    async fn valid_values() {
        reset().await;
        let values = [
            ("FOO", new_arg("FOO", None)),
            ("FOO=", new_arg("FOO", Some(""))),
            ("FOO=bar", new_arg("FOO", Some("bar"))),
        ];

        for (input, want) in values {
            let got: EnvVarArg = input.parse().unwrap();
            assert_eq!(got, want);
        }
    }

    fn new_arg(key: &str, value: Option<&str>) -> EnvVarArg {
        EnvVarArg {
            key: key.to_string(),
            value: value.map(|s| s.to_string()),
        }
    }
}
