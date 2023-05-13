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
        return Err(err);
    }
}
