use std::ffi::{OsStr, OsString};
use std::fmt::Display;

use clap::{Arg, Command, Error};
use regex::Regex;

use crate::plugins::PluginName;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RuntimeArg {
    pub plugin: PluginName,
    pub version: String,
}

impl RuntimeArg {
    pub fn parse(input: &str) -> Self {
        let (plugin, version) = input.split_once('@').unwrap_or((input, "latest"));
        Self {
            plugin: plugin.into(),
            version: version.into(),
        }
    }

    /// this handles the case where the user typed in:
    /// rtx local nodejs 20.0.0
    /// instead of
    /// rtx local nodejs 20.0.0
    ///
    /// We can detect this, and we know what they meant, so make it work the way
    /// they expected.
    pub fn double_runtime_condition(runtimes: &[RuntimeArg]) -> Vec<RuntimeArg> {
        let mut runtimes = runtimes.to_vec();
        if runtimes.len() == 2 {
            let re: &Regex = regex!(r"^\d+(\.\d+)?(\.\d+)?$");
            let a = runtimes[0].clone();
            let b = runtimes[1].clone();
            if a.version == "latest" && b.version == "latest" && re.is_match(&b.plugin) {
                runtimes[1].version = b.plugin;
                runtimes[1].plugin = a.plugin;
                runtimes.remove(0);
            }
        }
        runtimes
    }
}

impl Display for RuntimeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.plugin, self.version)
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
