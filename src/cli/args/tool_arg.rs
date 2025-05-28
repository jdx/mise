use std::path::PathBuf;
use std::str::FromStr;
use std::{fmt::Display, sync::Arc};

use crate::cli::args::BackendArg;
use crate::toolset::{ToolRequest, ToolSource};
use crate::ui::style;
use console::style;
use eyre::bail;
use xx::regex;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolArg {
    pub short: String,
    pub ba: Arc<BackendArg>,
    pub version: Option<String>,
    pub version_type: ToolVersionType,
    pub tvr: Option<ToolRequest>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ToolVersionType {
    Path(PathBuf),
    Prefix(String),
    Ref(String, String),
    Sub { sub: String, orig_version: String },
    System,
    Version(String),
}

impl FromStr for ToolArg {
    type Err = eyre::Error;

    fn from_str(input: &str) -> eyre::Result<Self> {
        let (backend_input, version) = parse_input(input);

        let ba: Arc<BackendArg> = Arc::new(backend_input.into());
        let version_type = match version.as_ref() {
            Some(version) => version.parse()?,
            None => ToolVersionType::Version(String::from("latest")),
        };
        let tvr = version
            .as_ref()
            .map(|v| ToolRequest::new(ba.clone(), v, ToolSource::Argument))
            .transpose()?;
        Ok(Self {
            short: ba.short.clone(),
            tvr,
            version: version.map(|v| v.to_string()),
            version_type,
            ba,
        })
    }
}

impl FromStr for ToolVersionType {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        trace!("parsing ToolVersionType from: {}", s);
        Ok(match s.split_once(':') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => {
                Self::Ref(ref_type.to_string(), r.to_string())
            }
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
            Self::Prefix(p) => write!(f, "prefix:{p}"),
            Self::Ref(rt, r) => write!(f, "{rt}:{r}"),
            Self::Sub { sub, orig_version } => write!(f, "sub-{sub}:{orig_version}"),
            Self::System => write!(f, "system"),
            Self::Version(v) => write!(f, "{v}"),
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
            let re = regex!(r"^\d+(\.\d+)*$");
            let a = tools[0].clone();
            let b = tools[1].clone();
            if a.tvr.is_none() && b.tvr.is_none() && re.is_match(&b.ba.tool_name) {
                tools[1].short = a.short.clone();
                tools[1].tvr = Some(ToolRequest::new(
                    a.ba.clone(),
                    &b.ba.tool_name,
                    ToolSource::Argument,
                )?);
                tools[1].ba = a.ba;
                tools[1].version_type = b.ba.tool_name.parse()?;
                tools[1].version = Some(b.ba.tool_name.clone());
                tools.remove(0);
            }
        }
        Ok(tools)
    }

    pub fn with_version(self, version: &str) -> Self {
        Self {
            tvr: Some(ToolRequest::new(self.ba.clone(), version, ToolSource::Argument).unwrap()),
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
            style(&self.short).blue().for_stderr(),
            style(&format!("@{version}")).for_stderr()
        )
    }
}

impl Display for ToolArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tvr {
            Some(tvr) => write!(f, "{tvr}"),
            _ => write!(f, "{}", self.ba.tool_name),
        }
    }
}

fn parse_input(s: &str) -> (&str, Option<&str>) {
    let (backend, version) = s
        .split_once('@')
        .map(|(f, v)| (f, if v.is_empty() { None } else { Some(v) }))
        .unwrap_or((s, None));

    // special case for packages with npm scopes like "npm:@antfu/ni"
    if backend == "npm:" {
        if let Some(v) = version {
            return if let Some(i) = v.find('@') {
                let ver = &v[i + 1..];
                (
                    &s[..backend.len() + i + 1],
                    if ver.is_empty() { None } else { Some(ver) },
                )
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

    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_tool_arg() {
        let _config = Config::get().await.unwrap();
        let tool = ToolArg::from_str("node").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                short: "node".into(),
                ba: Arc::new("node".into()),
                version: None,
                version_type: ToolVersionType::Version("latest".into()),
                tvr: None,
            }
        );
    }

    #[tokio::test]
    async fn test_tool_arg_with_version() {
        let _config = Config::get().await.unwrap();
        let tool = ToolArg::from_str("node@20").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                short: "node".into(),
                ba: Arc::new("node".into()),
                version: Some("20".into()),
                version_type: ToolVersionType::Version("20".into()),
                tvr: Some(
                    ToolRequest::new(Arc::new("node".into()), "20", ToolSource::Argument).unwrap()
                ),
            }
        );
    }

    #[tokio::test]
    async fn test_tool_arg_with_version_and_alias() {
        let _config = Config::get().await.unwrap();
        let tool = ToolArg::from_str("nodejs@lts").unwrap();
        assert_eq!(
            tool,
            ToolArg {
                short: "node".into(),
                ba: Arc::new("node".into()),
                version: Some("lts".into()),
                version_type: ToolVersionType::Version("lts".into()),
                tvr: Some(
                    ToolRequest::new(Arc::new("node".into()), "lts", ToolSource::Argument).unwrap()
                ),
            }
        );
    }

    #[tokio::test]
    async fn test_tool_arg_parse_input() {
        let _config = Config::get().await.unwrap();
        let t = |input, f, v| {
            let (backend, version) = parse_input(input);
            assert_eq!(backend, f);
            assert_eq!(version, v);
        };
        t("erlang", "erlang", None);
        t("erlang@", "erlang", None);
        t("erlang@27.2", "erlang", Some("27.2"));
        t("npm:@antfu/ni", "npm:@antfu/ni", None);
        t("npm:@antfu/ni@", "npm:@antfu/ni", None);
        t("npm:@antfu/ni@1.0.0", "npm:@antfu/ni", Some("1.0.0"));
        t("npm:@antfu/ni@1.0.0@1", "npm:@antfu/ni", Some("1.0.0@1"));
        t("npm:", "npm:", None);
        t("npm:prettier", "npm:prettier", None);
        t("npm:prettier@1.0.0", "npm:prettier", Some("1.0.0"));
        t(
            "ubi:BurntSushi/ripgrep[exe=rg]",
            "ubi:BurntSushi/ripgrep[exe=rg]",
            None,
        );
        t(
            "ubi:BurntSushi/ripgrep[exe=rg,match=musl]",
            "ubi:BurntSushi/ripgrep[exe=rg,match=musl]",
            None,
        );
        t(
            "ubi:BurntSushi/ripgrep[exe=rg,match=musl]@1.0.0",
            "ubi:BurntSushi/ripgrep[exe=rg,match=musl]",
            Some("1.0.0"),
        );
    }
}
