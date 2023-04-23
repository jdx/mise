use std::ffi::{OsStr, OsString};
use std::fmt::Display;

use clap::{Arg, Command, Error};
use color_eyre::eyre::Result;
use regex::Regex;

use crate::plugins::PluginName;
use crate::toolset::ToolVersionRequest;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolArg {
    pub plugin: PluginName,
    pub tvr: Option<ToolVersionRequest>,
}

impl ToolArg {
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
    /// rtx local node 20.0.0
    /// instead of
    /// rtx local node@20.0.0
    ///
    /// We can detect this, and we know what they meant, so make it work the way
    /// they expected.
    pub fn double_tool_condition(tools: &[ToolArg]) -> Vec<ToolArg> {
        let mut tools = tools.to_vec();
        if tools.len() == 2 {
            let re: &Regex = regex!(r"^\d+(\.\d+)?(\.\d+)?$");
            let a = tools[0].clone();
            let b = tools[1].clone();
            if matches!(a.tvr, None) && matches!(b.tvr, None) && re.is_match(&b.plugin) {
                tools[1].tvr = Some(ToolVersionRequest::new(a.plugin.clone(), &b.plugin));
                tools[1].plugin = a.plugin;
                tools.remove(0);
            }
        }
        tools
    }

    pub fn with_version(self, version: &str) -> Self {
        Self {
            tvr: Some(ToolVersionRequest::new(self.plugin.clone(), version)),
            ..self
        }
    }
}

impl Display for ToolArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tvr {
            Some(tvr) => write!(f, "{}", tvr),
            _ => write!(f, "{}", self.plugin),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolArgParser;

impl clap::builder::TypedValueParser for ToolArgParser {
    type Value = ToolArg;

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
        Ok(ToolArg::parse(&value.to_string_lossy()))
    }
}
