use std::ffi::{OsStr, OsString};
use std::fmt::Display;

use clap::{Arg, Command, Error};
use color_eyre::eyre::Result;
use regex::Regex;

use crate::plugins::PluginName;
use crate::toolset::ToolVersionRequest;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeArg {
    pub plugin: PluginName,
    pub tvr: Option<ToolVersionRequest>,
}

impl RuntimeArg {
    pub fn parse(input: &str) -> Self {
        match input.split_once('@') {
            Some((plugin, version)) => Self {
                plugin: plugin.to_string(),
                tvr: Some(ToolVersionRequest::new(plugin.to_string(), version)),
            },
            None => Self {
                plugin: input.into(),
                tvr: None,
            },
        }
    }

    /// this handles the case where the user typed in:
    /// rtx local nodejs 18.0.0
    /// instead of
    /// rtx local nodejs@18.0.0
    ///
    /// We can detect this, and we know what they meant, so make it work the way
    /// they expected.
    pub fn double_runtime_condition(runtimes: &[RuntimeArg]) -> Vec<RuntimeArg> {
        let mut runtimes = runtimes.to_vec();
        if runtimes.len() == 2 {
            let re: &Regex = regex!(r"^\d+(\.\d+)?(\.\d+)?$");
            let a = runtimes[0].clone();
            let b = runtimes[1].clone();
            if matches!(a.tvr, None) && matches!(b.tvr, None) && re.is_match(&b.plugin) {
                runtimes[1].tvr = Some(ToolVersionRequest::new(a.plugin.clone(), &b.plugin));
                runtimes[1].plugin = a.plugin;
                runtimes.remove(0);
            }
        }
        runtimes
    }

    pub fn with_version(self, version: &str) -> Self {
        Self {
            tvr: Some(ToolVersionRequest::new(self.plugin.clone(), version)),
            ..self
        }
    }
}

impl Display for RuntimeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tvr {
            Some(tvr) => write!(f, "{}", tvr),
            _ => write!(f, "{}", self.plugin),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeArgParser;

impl clap::builder::TypedValueParser for RuntimeArgParser {
    type Value = RuntimeArg;

    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, Error> {
        self.parse(cmd, arg, value.to_os_string())
    }

    fn parse(
        &self,
        _cmd: &Command,
        _arg: Option<&Arg>,
        value: OsString,
    ) -> Result<Self::Value, Error> {
        Ok(RuntimeArg::parse(&value.to_string_lossy()))
    }
}
