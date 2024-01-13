use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;

use console::style;
use regex::Regex;

use crate::cli::args::ForgeArg;
use crate::forge::ForgeType;
use crate::toolset::ToolVersionRequest;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolArg {
    pub forge: String,
    pub forge_type: ForgeType,
    pub version: Option<String>,
    pub version_type: ToolVersionType,
    pub tvr: Option<ToolVersionRequest>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ToolVersionType {
    Path(PathBuf),
    Prefix(String),
    Ref(String),
    Sub { sub: String, orig_version: String },
    System,
    Version(String),
}

impl FromStr for ToolArg {
    type Err = eyre::Error;

    fn from_str(input: &str) -> eyre::Result<Self> {
        let (forge_input, version) = input
            .split_once('@')
            .map(|(f, v)| (f, Some(v.to_string())))
            .unwrap_or((input, None));
        let forge: ForgeArg = forge_input.parse()?;
        let tvr = version
            .as_ref()
            .map(|v| ToolVersionRequest::new(forge.name.clone(), v));
        let version_type = match version.as_ref() {
            Some(version) => match version.split_once(':') {
                Some(("ref", r)) => ToolVersionType::Ref(r.to_string()),
                Some(("prefix", p)) => ToolVersionType::Prefix(p.to_string()),
                Some(("path", p)) => ToolVersionType::Path(PathBuf::from(p)),
                Some((p, v)) if p.starts_with("sub-") => ToolVersionType::Sub {
                    sub: p.split_once('-').unwrap().1.to_string(),
                    orig_version: v.to_string(),
                },
                None if version == "system" => ToolVersionType::System,
                None => ToolVersionType::Version(version.to_string()),
                _ => bail!("invalid tool version request: {version}"),
            },
            None => ToolVersionType::Version(String::from("latest")),
        };
        Ok(Self {
            tvr,
            version,
            version_type,
            forge: forge.name,
            forge_type: forge.forge_type,
        })
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
            if a.tvr.is_none() && b.tvr.is_none() && re.is_match(&b.forge) {
                tools[1].tvr = Some(ToolVersionRequest::new(a.forge.clone(), &b.forge));
                tools[1].forge = a.forge;
                tools.remove(0);
            }
        }
        tools
    }

    pub fn with_version(self, version: &str) -> Self {
        Self {
            tvr: Some(ToolVersionRequest::new(self.forge.clone(), version)),
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
            style(&self.forge).blue().for_stderr(),
            style(&format!("@{version}")).for_stderr()
        )
    }
}

impl Display for ToolArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tvr {
            Some(tvr) => write!(f, "{}", tvr),
            _ => write!(f, "{}", self.forge),
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
                forge: "node".into(),
                version: None,
                version_type: ToolVersionType::Version("latest".into()),
                forge_type: ForgeType::Asdf,
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
                forge: "node".into(),
                forge_type: ForgeType::Asdf,
                version: Some("20".into()),
                version_type: ToolVersionType::Version("20".into()),
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
                forge: "node".into(),
                forge_type: ForgeType::Asdf,
                version: Some("lts".into()),
                version_type: ToolVersionType::Version("lts".into()),
                tvr: Some(ToolVersionRequest::new("node".into(), "lts")),
            }
        );
    }
}
