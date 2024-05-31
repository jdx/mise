use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;

use console::style;
use eyre::bail;
use regex::Regex;

use crate::cli::args::BackendArg;
use crate::toolset::ToolRequest;
use crate::ui::style;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolArg {
    pub backend: BackendArg,
    pub version: Option<String>,
    pub version_type: ToolVersionType,
    pub tvr: Option<ToolRequest>,
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
        let (backend_input, version) = parse_input(input);

        let backend: BackendArg = backend_input.into();
        let version_type = match version.as_ref() {
            Some(version) => version.parse()?,
            None => ToolVersionType::Version(String::from("latest")),
        };
        let tvr = version
            .as_ref()
            .map(|v| ToolRequest::new(backend.clone(), v))
            .transpose()?;
        Ok(Self {
            tvr,
            version: version.map(|v| v.to_string()),
            version_type,
            backend,
        })
    }
}

impl FromStr for ToolVersionType {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.split_once(':') {
            Some(("ref", r)) => Self::Ref(r.to_string()),
            Some(("prefix", p)) => Self::Prefix(p.to_string()),
            Some(("path", p)) => Self::Path(PathBuf::from(p)),
            Some((p, v)) if p.starts_with("sub-") => Self::Sub {
                sub: p.split_once('-').unwrap().1.to_string(),
                orig_version: v.to_string(),
            },
            Some((p, _)) => bail!("invalid prefix: {}", style::ered(p)),
            None if s == "system" => Self::System,
            None => Self::Version(s.to_string()),
        })
    }
}

impl Display for ToolVersionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(p) => write!(f, "path:{}", p.to_string_lossy()),
            Self::Prefix(p) => write!(f, "prefix:{}", p),
            Self::Ref(r) => write!(f, "ref:{}", r),
            Self::Sub { sub, orig_version } => write!(f, "sub-{}:{}", sub, orig_version),
            Self::System => write!(f, "system"),
            Self::Version(v) => write!(f, "{}", v),
        }
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
    pub fn double_tool_condition(tools: &[ToolArg]) -> eyre::Result<Vec<ToolArg>> {
        let mut tools = tools.to_vec();
        if tools.len() == 2 {
            let re: &Regex = regex!(r"^\d+(\.\d+)*$");
            let a = tools[0].clone();
            let b = tools[1].clone();
            if a.tvr.is_none() && b.tvr.is_none() && re.is_match(&b.backend.name) {
                tools[1].tvr = Some(ToolRequest::new(a.backend.clone(), &b.backend.name)?);
                tools[1].backend = a.backend;
                tools[1].version_type = b.backend.name.parse()?;
                tools[1].version = Some(b.backend.name);
                tools.remove(0);
            }
        }
        Ok(tools)
    }

    pub fn with_version(self, version: &str) -> Self {
        Self {
            tvr: Some(ToolRequest::new(self.backend.clone(), version).unwrap()),
            version: Some(version.into()),
            version_type: version.parse().unwrap(),
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
            style(&self.backend.name).blue().for_stderr(),
            style(&format!("@{version}")).for_stderr()
        )
    }
}

impl Display for ToolArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tvr {
            Some(tvr) => write!(f, "{}", tvr),
            _ => write!(f, "{}", self.backend.name),
        }
    }
}

fn parse_input(s: &str) -> (&str, Option<&str>) {
    let (backend, version) = s
        .split_once('@')
        .map(|(f, v)| (f, Some(v)))
        .unwrap_or((s, None));

    // special case for packages with npm scopes like "npm:@antfu/ni"
    if backend == "npm:" {
        if let Some(v) = version {
            return if let Some(i) = v.find('@') {
                (&s[..backend.len() + i + 1], Some(&v[i + 1..]))
            } else {
                (&s[..backend.len() + v.len() + 1], None)
            };
        }
    }

    (backend, version)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_tool_arg() {
        let tool = ToolArg::from_str("node").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                backend: "node".into(),
                version: None,
                version_type: ToolVersionType::Version("latest".into()),
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
                backend: "node".into(),
                version: Some("20".into()),
                version_type: ToolVersionType::Version("20".into()),
                tvr: Some(ToolRequest::new("node".into(), "20").unwrap()),
            }
        );
    }

    #[test]
    fn test_tool_arg_with_version_and_alias() {
        let tool = ToolArg::from_str("nodejs@lts").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                backend: "node".into(),
                version: Some("lts".into()),
                version_type: ToolVersionType::Version("lts".into()),
                tvr: Some(ToolRequest::new("node".into(), "lts").unwrap()),
            }
        );
    }

    #[test]
    fn test_tool_arg_parse_input() {
        let t = |input, f, v| {
            let (backend, version) = parse_input(input);
            assert_eq!(backend, f);
            assert_eq!(version, v);
        };
        t("npm:@antfu/ni", "npm:@antfu/ni", None);
        t("npm:@antfu/ni@1.0.0", "npm:@antfu/ni", Some("1.0.0"));
        t("npm:@antfu/ni@1.0.0@1", "npm:@antfu/ni", Some("1.0.0@1"));
        t("npm:", "npm:", None);
        t("npm:prettier", "npm:prettier", None);
        t("npm:prettier@1.0.0", "npm:prettier", Some("1.0.0"));
    }
}
