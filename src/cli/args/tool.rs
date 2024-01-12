use std::fmt::Display;
use std::str::FromStr;

use console::style;
use regex::Regex;

use crate::plugins::{unalias_plugin, PluginName};
use crate::toolset::ToolVersionRequest;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolArg {
    pub plugin: PluginName,
    pub tvr: Option<ToolVersionRequest>,
}

impl FromStr for ToolArg {
    type Err = eyre::Error;

    fn from_str(input: &str) -> eyre::Result<Self> {
        let arg = match input.split_once('@') {
            Some((plugin, version)) => {
                let plugin = unalias_plugin(plugin).to_string();
                Self {
                    plugin: plugin.clone(),
                    tvr: Some(ToolVersionRequest::new(plugin, version)),
                }
            }
            None => Self {
                plugin: unalias_plugin(input).into(),
                tvr: None,
            },
        };
        Ok(arg)
    }
}

impl ToolArg {
    /// this handles the case where the user typed in:
    /// mise local node 20.0.0
    /// instead of
    /// mise local node@20.0.0
    ///
    /// We can detect this, and we know what they meant, so make it work the way
    /// they expected.
    pub fn double_tool_condition(tools: &[ToolArg]) -> Vec<ToolArg> {
        let mut tools = tools.to_vec();
        if tools.len() == 2 {
            let re: &Regex = regex!(r"^\d+(\.\d+)?(\.\d+)?$");
            let a = tools[0].clone();
            let b = tools[1].clone();
            if a.tvr.is_none() && b.tvr.is_none() && re.is_match(&b.plugin) {
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

    pub fn style(&self) -> String {
        let version = self
            .tvr
            .as_ref()
            .map(|t| t.version())
            .unwrap_or(String::from("latest"));
        format!(
            "{}{}",
            style(&self.plugin).blue().for_stderr(),
            style(&format!("@{version}")).for_stderr()
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_arg() {
        let tool = ToolArg::from_str("node").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                plugin: "node".into(),
                tvr: None,
            }
        );
    }

    #[test]
    fn test_tool_arg_with_version() {
        let tool = ToolArg::from_str("node@20").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                plugin: "node".into(),
                tvr: Some(ToolVersionRequest::new("node".into(), "20")),
            }
        );
    }

    #[test]
    fn test_tool_arg_with_version_and_alias() {
        let tool = ToolArg::from_str("nodejs@lts").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                plugin: "node".into(),
                tvr: Some(ToolVersionRequest::new("node".into(), "lts")),
            }
        );
    }
}
