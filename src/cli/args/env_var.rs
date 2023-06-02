use clap::{
    error::{ContextKind, ContextValue, ErrorKind},
    Arg, Command, Error,
};
use std::ffi::OsStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvVarArg {
    pub key: String,
    pub value: String,
}

impl EnvVarArg {
    pub fn parse(input: &str) -> Option<Self> {
        input.split_once('=').map(|(k, v)| Self {
            key: k.to_string(),
            value: v.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct EnvVarArgParser;

impl clap::builder::TypedValueParser for EnvVarArgParser {
    type Value = EnvVarArg;

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, Error> {
        if let Some(parsed) = EnvVarArg::parse(&value.to_string_lossy()) {
            return Ok(parsed);
        }

        let mut err = clap::Error::new(ErrorKind::ValueValidation).with_cmd(cmd);
        if let Some(arg) = arg {
            err.insert(
                ContextKind::InvalidArg,
                ContextValue::String(arg.to_string()),
            );
        }
        err.insert(
            ContextKind::InvalidValue,
            ContextValue::String(value.to_string_lossy().into()),
        );
        Err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::EnvVarArg;

    #[test]
    fn invalid_value() {
        let res = EnvVarArg::parse("NO_EQUAL_SIGN");
        assert!(res.is_none());
    }

    #[test]
    fn valid_values() {
        let values = [
            ("FOO=", new_arg("FOO", "")),
            ("FOO=bar", new_arg("FOO", "bar")),
        ];

        for (input, want) in values {
            let got = EnvVarArg::parse(input);
            assert_eq!(got, Some(want));
        }
    }

    fn new_arg(key: &str, value: &str) -> EnvVarArg {
        EnvVarArg {
            key: key.to_string(),
            value: value.to_string(),
        }
    }
}
