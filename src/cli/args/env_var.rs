use std::ffi::OsStr;

use clap::{Arg, Command, Error};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvVarArg {
    pub key: String,
    pub value: Option<String>,
}

impl EnvVarArg {
    pub fn parse(input: &str) -> Self {
        input
            .split_once('=')
            .map(|(k, v)| Self {
                key: k.to_string(),
                value: Some(v.to_string()),
            })
            .unwrap_or_else(|| Self {
                key: input.to_string(),
                value: None,
            })
    }
}

#[derive(Debug, Clone)]
pub struct EnvVarArgParser;

impl clap::builder::TypedValueParser for EnvVarArgParser {
    type Value = EnvVarArg;

    fn parse_ref(
        &self,
        _cmd: &Command,
        _arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, Error> {
        Ok(EnvVarArg::parse(&value.to_string_lossy()))
    }
}

#[cfg(test)]
mod tests {
    use super::EnvVarArg;

    #[test]
    fn valid_values() {
        let values = [
            ("FOO", new_arg("FOO", None)),
            ("FOO=", new_arg("FOO", Some(""))),
            ("FOO=bar", new_arg("FOO", Some("bar"))),
        ];

        for (input, want) in values {
            let got = EnvVarArg::parse(input);
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
