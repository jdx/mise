//! Known vulnerabilities in tool versions, looked up in [OSV.dev](https://osv.dev).
//!
//! OSV collects advisories from GitHub, PyPA, RustSec, the Go vulnerability
//! database, NVD and more under one schema. A package registry tool (`npm:`,
//! `pipx:`, `cargo:`, ...) is looked up by its package name; a tool released
//! from a git repository (`github:`, `aqua:`, node, python, ...) by the
//! repository and the release tag. OSV decides whether a version is affected,
//! so mise never has to order versions itself.

use std::collections::HashSet;
use std::sync::Arc;

use eyre::{Result, WrapErr, ensure};
use itertools::Itertools;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::args::BackendArg;
use crate::backend::ABackend;
use crate::backend::backend_type::BackendType;
use crate::config::Settings;

const OSV_API: &str = "https://api.osv.dev/v1";
/// The most queries `/v1/querybatch` takes in one request.
const OSV_BATCH_LIMIT: usize = 1000;

/// A package as OSV names it: an ecosystem and a name within it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OsvPackage {
    pub ecosystem: String,
    pub name: String,
}

impl OsvPackage {
    fn new(ecosystem: &str, name: impl Into<String>) -> Self {
        Self {
            ecosystem: ecosystem.to_string(),
            name: name.into(),
        }
    }

    /// A git repository, which OSV matches against release tags.
    fn git(url: &str) -> Self {
        Self::new("GIT", url)
    }
}

/// One lookup for a tool version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryQuery {
    /// Names the tool may be published under, best first. With more than
    /// one, the first that OSV has any record of is used.
    pub packages: Vec<OsvPackage>,
    pub version: String,
}

impl AdvisoryQuery {
    fn new(package: OsvPackage, version: impl Into<String>) -> Self {
        Self {
            packages: vec![package],
            version: version.into(),
        }
    }
}

/// The OSV lookups for `version` of a tool, given its backend type and tool
/// name. Empty when mise knows no advisory source for the tool.
pub fn queries_for(backend: &BackendType, tool_name: &str, version: &str) -> Vec<AdvisoryQuery> {
    match backend {
        BackendType::Npm => vec![AdvisoryQuery::new(
            OsvPackage::new("npm", tool_name),
            version,
        )],
        BackendType::Gem => vec![AdvisoryQuery::new(
            OsvPackage::new("RubyGems", tool_name),
            version,
        )],
        BackendType::Dotnet => vec![AdvisoryQuery::new(
            OsvPackage::new("NuGet", tool_name),
            version,
        )],
        BackendType::Pipx => pipx_queries(tool_name, version),
        BackendType::Cargo => cargo_queries(tool_name, version),
        BackendType::Go => go_module_query(tool_name, version).into_iter().collect(),
        BackendType::Github | BackendType::Aqua => match tool_name.split_once('/') {
            Some((owner, repo)) if !repo.contains('/') => {
                github_queries(owner, repo, version, version)
            }
            _ => vec![],
        },
        BackendType::Gitlab => vec![AdvisoryQuery::new(
            OsvPackage::git(&format!("https://gitlab.com/{tool_name}")),
            version,
        )],
        BackendType::Packslip => crate::backend::packslip::project_name(tool_name)
            .ok()
            .and_then(|project| {
                let path = project.strip_prefix("github.com/")?;
                let (owner, repo) = path.split_once('/')?;
                Some(github_queries(owner, repo, version, version))
            })
            .unwrap_or_default(),
        BackendType::Core => core_queries(tool_name, version),
        _ => vec![],
    }
}

/// The lookups for a tool released from `github.com/<owner>/<repo>`, whose
/// release `version` carries the tag `tag`. Go programs are also looked up
/// as Go modules, where the Go vulnerability database has fixed versions.
pub fn github_queries(owner: &str, repo: &str, version: &str, tag: &str) -> Vec<AdvisoryQuery> {
    let repo = repo.trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return vec![];
    }
    let module = format!("github.com/{owner}/{repo}");
    let mut modules = vec![];
    if let Some(major) = major_version(version).filter(|major| *major >= 2) {
        modules.push(OsvPackage::new("Go", format!("{module}/v{major}")));
    }
    modules.push(OsvPackage::new("Go", module));
    vec![
        AdvisoryQuery {
            packages: modules,
            version: version.to_string(),
        },
        // OSV stores GitHub repository URLs in lowercase and matches them exactly.
        AdvisoryQuery::new(
            OsvPackage::git(&format!("https://github.com/{owner}/{repo}").to_lowercase()),
            tag,
        ),
    ]
}

fn core_queries(tool_name: &str, version: &str) -> Vec<AdvisoryQuery> {
    // Only plain release numbers: `python = "pypy3.10-7.3.15"` is not CPython.
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return vec![];
    }
    let repo = match tool_name {
        "go" => {
            return vec![AdvisoryQuery::new(OsvPackage::new("Go", "stdlib"), version)];
        }
        "node" => "nodejs/node",
        "python" => "python/cpython",
        "deno" => "denoland/deno",
        "bun" => "oven-sh/bun",
        "erlang" => "erlang/otp",
        _ => return vec![],
    };
    vec![AdvisoryQuery::new(
        OsvPackage::git(&format!("https://github.com/{repo}")),
        version,
    )]
}

fn pipx_queries(tool_name: &str, version: &str) -> Vec<AdvisoryQuery> {
    let source = tool_name.strip_prefix("git+").unwrap_or(tool_name);
    let github_path = source
        .strip_prefix("https://github.com/")
        .or_else(|| (!source.contains(':') && source.contains('/')).then_some(source));
    if let Some(path) = github_path {
        let path = path.split(['@', '#']).next().unwrap_or_default();
        return match path.split('/').collect_vec().as_slice() {
            [owner, repo] => vec![AdvisoryQuery::new(
                OsvPackage::git(
                    &format!(
                        "https://github.com/{owner}/{}",
                        repo.trim_end_matches(".git")
                    )
                    .to_lowercase(),
                ),
                version,
            )],
            _ => vec![],
        };
    }
    if source.contains(':') {
        return vec![];
    }
    let name = source.split('[').next().unwrap_or(source);
    vec![AdvisoryQuery::new(
        OsvPackage::new("PyPI", normalize_pypi_name(name)),
        version,
    )]
}

fn cargo_queries(tool_name: &str, version: &str) -> Vec<AdvisoryQuery> {
    if let Some(path) = tool_name.strip_prefix("https://github.com/") {
        return match path
            .trim_end_matches('/')
            .split('/')
            .collect_vec()
            .as_slice()
        {
            [owner, repo] => github_queries(owner, repo, version, version)
                .into_iter()
                .filter(|q| q.packages.iter().all(|p| p.ecosystem == "GIT"))
                .collect(),
            _ => vec![],
        };
    }
    if tool_name.contains(['/', ':']) {
        return vec![];
    }
    vec![AdvisoryQuery::new(
        OsvPackage::new("crates.io", tool_name),
        version,
    )]
}

/// A `go:` tool names a package, and the module holding it is one of the
/// package path's prefixes, longest first.
fn go_module_query(tool_name: &str, version: &str) -> Option<AdvisoryQuery> {
    let parts = tool_name.split('/').collect_vec();
    let packages = (2..=parts.len())
        .rev()
        .map(|i| OsvPackage::new("Go", parts[..i].join("/")))
        .collect_vec();
    (!packages.is_empty()).then(|| AdvisoryQuery {
        packages,
        version: version.to_string(),
    })
}

/// PyPI compares names after lowercasing and folding runs of `-`, `_` and
/// `.` into one `-` (PEP 503).
fn normalize_pypi_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.trim().chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// The leading number of a version such as `2.40.1` or `v2.40.1`.
fn major_version(version: &str) -> Option<u64> {
    let digits = version
        .trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .next()?;
    digits.parse().ok()
}

/// Whether `version` names a release that OSV could know about, rather
/// than a request mise resolves elsewhere (`ref:main`, `path:...`).
fn is_release(version: &str) -> bool {
    !version.is_empty()
        && !matches!(version, "latest" | "system")
        && ![
            "ref:", "path:", "sub-", "prefix:", "tag:", "rev:", "branch:",
        ]
        .iter()
        .any(|p| version.starts_with(p))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// One advisory that affects one tool version.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    #[serde(skip)]
    pub ba: Arc<BackendArg>,
    pub tool: String,
    pub version: String,
    pub id: String,
    pub aliases: Vec<String>,
    pub severity: Option<Severity>,
    pub summary: String,
    /// Versions that fix the advisory, as the advisory lists them. Empty when
    /// it names fixes only as commits.
    pub fixed: Vec<String>,
    pub url: String,
}

/// A tool version that was not looked up.
#[derive(Debug, Clone, Serialize)]
pub struct Unchecked {
    #[serde(skip)]
    pub ba: Arc<BackendArg>,
    pub tool: String,
    pub version: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub unchecked: Vec<Unchecked>,
}

/// Looks up every tool version in `items` in OSV.
pub async fn scan(items: Vec<(ABackend, String)>) -> Result<Report> {
    ensure!(
        !mise_util::network::offline(&Settings::get()),
        "checking for vulnerabilities needs OSV.dev, but offline mode is enabled"
    );
    let mut report = Report::default();
    let mut planned = vec![];
    for (backend, version) in items {
        let queries = if is_release(&version) {
            backend.advisory_queries(&version).await
        } else {
            vec![]
        };
        if queries.is_empty() {
            report.unchecked.push(Unchecked {
                ba: backend.ba().clone(),
                tool: backend.id().to_string(),
                version,
            });
        } else {
            planned.push((backend, version, queries));
        }
    }

    let alternatives = planned
        .iter()
        .flat_map(|(_, _, queries)| queries)
        .filter(|q| q.packages.len() > 1)
        .flat_map(|q| q.packages.iter().cloned())
        .unique()
        .collect_vec();
    let known = known_packages(alternatives).await?;

    let mut lookups = vec![];
    for (i, (_, _, queries)) in planned.iter().enumerate() {
        for query in queries {
            let package = match query.packages.as_slice() {
                [only] => Some(only),
                many => many.iter().find(|p| known.contains(*p)),
            };
            if let Some(package) = package {
                lookups.push((i, package.clone(), query.version.clone()));
            }
        }
    }
    let results = crate::parallel::parallel(lookups, |(i, package, version)| async move {
        let vulns = query(&package, &version).await?;
        Ok((i, package, vulns))
    })
    .await?;

    let mut seen: Vec<HashSet<String>> = vec![HashSet::new(); planned.len()];
    for (i, package, vulns) in results {
        let (backend, version, _) = &planned[i];
        for vuln in vulns {
            let names = std::iter::once(&vuln.id)
                .chain(&vuln.aliases)
                .cloned()
                .collect_vec();
            let fixed = vuln.fixed_versions(&package);
            if names.iter().any(|n| seen[i].contains(n)) {
                // The same advisory under another id: keep the first, but
                // take the fixed versions if only this one lists them.
                if let Some(existing) = report.findings.iter_mut().find(|f| {
                    Arc::ptr_eq(&f.ba, backend.ba())
                        && f.version == *version
                        && std::iter::once(&f.id)
                            .chain(&f.aliases)
                            .any(|n| names.contains(n))
                }) {
                    if existing.fixed.is_empty() {
                        existing.fixed = fixed;
                    }
                    if existing.severity.is_none() {
                        existing.severity = vuln.severity();
                    }
                }
                seen[i].extend(names);
                continue;
            }
            seen[i].extend(names);
            report.findings.push(Finding {
                ba: backend.ba().clone(),
                tool: backend.id().to_string(),
                version: version.clone(),
                severity: vuln.severity(),
                summary: vuln.summary(),
                url: format!("https://osv.dev/vulnerability/{}", vuln.id),
                fixed,
                aliases: vuln.aliases,
                id: vuln.id,
            });
        }
    }
    Ok(report)
}

#[derive(Debug, Serialize)]
struct QueryRequest<'a> {
    package: &'a OsvPackage,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct QueryResponse {
    #[serde(default)]
    vulns: Vec<OsvVuln>,
    next_page_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct BatchRequest<'a> {
    queries: Vec<QueryRequest<'a>>,
}

#[derive(Debug, Deserialize)]
struct BatchResponse {
    #[serde(default)]
    results: Vec<BatchResult>,
}

#[derive(Debug, Default, Deserialize)]
struct BatchResult {
    #[serde(default)]
    vulns: Vec<serde_json::Value>,
}

/// The advisories OSV has for `version` of `package`.
async fn query(package: &OsvPackage, version: &str) -> Result<Vec<OsvVuln>> {
    let mut vulns = vec![];
    let mut page_token = None;
    loop {
        let res: QueryResponse = post(
            "query",
            &QueryRequest {
                package,
                version: Some(version),
                page_token,
            },
        )
        .await
        .wrap_err_with(|| {
            format!(
                "looking up {} {} {version} on OSV.dev",
                package.ecosystem, package.name
            )
        })?;
        vulns.extend(res.vulns);
        match res.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => return Ok(vulns),
        }
    }
}

/// Which of `packages` OSV has any advisory for, in any version.
async fn known_packages(packages: Vec<OsvPackage>) -> Result<HashSet<OsvPackage>> {
    let mut known = HashSet::new();
    for chunk in packages.chunks(OSV_BATCH_LIMIT) {
        let res: BatchResponse = post(
            "querybatch",
            &BatchRequest {
                queries: chunk
                    .iter()
                    .map(|package| QueryRequest {
                        package,
                        version: None,
                        page_token: None,
                    })
                    .collect(),
            },
        )
        .await
        .wrap_err("looking up packages on OSV.dev")?;
        for (package, result) in chunk.iter().zip(res.results) {
            if !result.vulns.is_empty() {
                known.insert(package.clone());
            }
        }
    }
    Ok(known)
}

async fn post<T: DeserializeOwned, B: Serialize>(path: &str, body: &B) -> Result<T> {
    let url = Url::parse(&format!("{OSV_API}/{path}"))?;
    let client = crate::http::HTTP.reqwest()?;
    crate::http::retry_async("POST", &url, || async {
        Ok(client
            .post(url.clone())
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json::<T>()
            .await?)
    })
    .await
}

/// The parts of an OSV record mise reads.
/// <https://ossf.github.io/osv-schema/>
#[derive(Debug, Default, Deserialize)]
struct OsvVuln {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    summary: Option<String>,
    details: Option<String>,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
    #[serde(default)]
    affected: Vec<OsvAffected>,
    database_specific: Option<OsvDatabaseSpecific>,
}

#[derive(Debug, Deserialize)]
struct OsvSeverity {
    r#type: String,
    score: String,
}

#[derive(Debug, Deserialize)]
struct OsvAffected {
    package: Option<OsvPackage>,
    #[serde(default)]
    ranges: Vec<OsvRange>,
}

#[derive(Debug, Deserialize)]
struct OsvRange {
    r#type: String,
    #[serde(default)]
    events: Vec<OsvEvent>,
}

#[derive(Debug, Deserialize)]
struct OsvEvent {
    fixed: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OsvDatabaseSpecific {
    severity: Option<String>,
}

impl OsvVuln {
    fn summary(&self) -> String {
        self.summary
            .as_deref()
            .or(self.details.as_deref())
            .and_then(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
            .unwrap_or_default()
            .to_string()
    }

    /// The rating the advisory database gives, or else the one its CVSS v3
    /// vector scores.
    fn severity(&self) -> Option<Severity> {
        let rated = self
            .database_specific
            .as_ref()
            .and_then(|d| d.severity.as_deref())
            .and_then(|s| match s.to_ascii_uppercase().as_str() {
                "LOW" => Some(Severity::Low),
                "MODERATE" | "MEDIUM" => Some(Severity::Medium),
                "HIGH" => Some(Severity::High),
                "CRITICAL" => Some(Severity::Critical),
                _ => None,
            });
        rated.or_else(|| {
            self.severity
                .iter()
                .filter(|s| s.r#type == "CVSS_V3")
                .find_map(|s| cvss3_base_score(&s.score))
                .and_then(rating)
        })
    }

    /// The fixed versions the advisory lists for `package`. Fixes in a
    /// `GIT` range are commits, not versions, so those are left out.
    fn fixed_versions(&self, package: &OsvPackage) -> Vec<String> {
        self.affected
            .iter()
            .filter(|a| {
                a.package.as_ref().is_some_and(|p| {
                    p.ecosystem == package.ecosystem && p.name.eq_ignore_ascii_case(&package.name)
                })
            })
            .flat_map(|a| &a.ranges)
            .filter(|r| r.r#type != "GIT")
            .flat_map(|r| &r.events)
            .filter_map(|e| e.fixed.clone())
            .unique()
            .collect()
    }
}

fn rating(score: f64) -> Option<Severity> {
    match score {
        s if s >= 9.0 => Some(Severity::Critical),
        s if s >= 7.0 => Some(Severity::High),
        s if s >= 4.0 => Some(Severity::Medium),
        s if s > 0.0 => Some(Severity::Low),
        _ => None,
    }
}

/// The CVSS v3.x base score of a vector such as
/// `CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H`.
/// <https://www.first.org/cvss/v3.1/specification-document#7-1-Base-Metrics-Equations>
fn cvss3_base_score(vector: &str) -> Option<f64> {
    let mut parts = vector.split('/');
    if !parts.next()?.starts_with("CVSS:3") {
        return None;
    }
    let metrics = parts
        .filter_map(|p| p.split_once(':'))
        .collect::<std::collections::HashMap<_, _>>();
    let metric = |name: &str| metrics.get(name).copied();
    let changed = match metric("S")? {
        "U" => false,
        "C" => true,
        _ => return None,
    };
    let av = match metric("AV")? {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.2,
        _ => return None,
    };
    let ac = match metric("AC")? {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let pr = match (metric("PR")?, changed) {
        ("N", _) => 0.85,
        ("L", false) => 0.62,
        ("L", true) => 0.68,
        ("H", false) => 0.27,
        ("H", true) => 0.5,
        _ => return None,
    };
    let ui = match metric("UI")? {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let cia = |name: &str| match metric(name)? {
        "H" => Some(0.56),
        "L" => Some(0.22),
        "N" => Some(0.0),
        _ => None,
    };
    let iss = 1.0 - (1.0 - cia("C")?) * (1.0 - cia("I")?) * (1.0 - cia("A")?);
    let impact = if changed {
        7.52 * (iss - 0.029) - 3.25 * (iss - 0.02f64).powi(15)
    } else {
        6.42 * iss
    };
    if impact <= 0.0 {
        return Some(0.0);
    }
    let exploitability = 8.22 * av * ac * pr * ui;
    let base = if changed {
        1.08 * (impact + exploitability)
    } else {
        impact + exploitability
    };
    Some(roundup(base.min(10.0)))
}

/// CVSS v3.1's round-up to one decimal, which avoids floating-point error.
fn roundup(value: f64) -> f64 {
    let int = (value * 100_000.0).round() as i64;
    if int % 10_000 == 0 {
        int as f64 / 100_000.0
    } else {
        ((int / 10_000) + 1) as f64 / 10.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn names(queries: &[AdvisoryQuery]) -> Vec<Vec<(String, String)>> {
        queries
            .iter()
            .map(|q| {
                q.packages
                    .iter()
                    .map(|p| (p.ecosystem.clone(), p.name.clone()))
                    .collect()
            })
            .collect()
    }

    fn pkg(ecosystem: &str, name: &str) -> (String, String) {
        (ecosystem.to_string(), name.to_string())
    }

    #[test]
    fn package_registry_tools_use_their_package_name() {
        assert_eq!(
            names(&queries_for(&BackendType::Npm, "@scope/pkg", "1.0.0")),
            vec![vec![pkg("npm", "@scope/pkg")]]
        );
        assert_eq!(
            names(&queries_for(
                &BackendType::Pipx,
                "Black_Formatter[d]",
                "1.0.0"
            )),
            vec![vec![pkg("PyPI", "black-formatter")]]
        );
        assert_eq!(
            names(&queries_for(&BackendType::Cargo, "ripgrep", "14.0.0")),
            vec![vec![pkg("crates.io", "ripgrep")]]
        );
        assert_eq!(
            names(&queries_for(&BackendType::Gem, "rails", "7.0.0")),
            vec![vec![pkg("RubyGems", "rails")]]
        );
        assert_eq!(
            names(&queries_for(
                &BackendType::Dotnet,
                "GitVersion.Tool",
                "5.0.0"
            )),
            vec![vec![pkg("NuGet", "GitVersion.Tool")]]
        );
    }

    #[test]
    fn pipx_git_sources_use_the_repository() {
        for name in [
            "git+https://github.com/PSF/black.git@main",
            "psf/black",
            "https://github.com/psf/black",
        ] {
            assert_eq!(
                names(&queries_for(&BackendType::Pipx, name, "24.1.0")),
                vec![vec![pkg("GIT", "https://github.com/psf/black")]],
                "{name}"
            );
        }
        assert!(queries_for(&BackendType::Pipx, "git+https://example.com/a/b", "1").is_empty());
    }

    #[test]
    fn go_tools_try_each_module_prefix() {
        assert_eq!(
            names(&queries_for(
                &BackendType::Go,
                "github.com/golangci/golangci-lint/v2/cmd/golangci-lint",
                "2.1.0"
            )),
            vec![vec![
                pkg(
                    "Go",
                    "github.com/golangci/golangci-lint/v2/cmd/golangci-lint"
                ),
                pkg("Go", "github.com/golangci/golangci-lint/v2/cmd"),
                pkg("Go", "github.com/golangci/golangci-lint/v2"),
                pkg("Go", "github.com/golangci/golangci-lint"),
                pkg("Go", "github.com/golangci"),
            ]]
        );
    }

    #[test]
    fn github_tools_use_the_lowercased_repository_and_go_modules() {
        let queries = queries_for(&BackendType::Github, "Cli/Cli", "2.40.0");
        assert_eq!(
            names(&queries),
            vec![
                vec![
                    pkg("Go", "github.com/Cli/Cli/v2"),
                    pkg("Go", "github.com/Cli/Cli")
                ],
                vec![pkg("GIT", "https://github.com/cli/cli")],
            ]
        );
        assert_eq!(
            names(&queries_for(
                &BackendType::Github,
                "BurntSushi/ripgrep",
                "1.0.0"
            )),
            vec![
                vec![pkg("Go", "github.com/BurntSushi/ripgrep")],
                vec![pkg("GIT", "https://github.com/burntsushi/ripgrep")],
            ]
        );
        assert_eq!(
            github_queries("jqlang", "jq", "1.7.1", "jq-1.7.1")[1].version,
            "jq-1.7.1"
        );
    }

    #[test]
    fn core_tools_with_advisory_sources() {
        assert_eq!(
            names(&queries_for(&BackendType::Core, "go", "1.22.0")),
            vec![vec![pkg("Go", "stdlib")]]
        );
        assert_eq!(
            names(&queries_for(&BackendType::Core, "node", "20.0.0")),
            vec![vec![pkg("GIT", "https://github.com/nodejs/node")]]
        );
        assert!(queries_for(&BackendType::Core, "python", "pypy3.10-7.3.15").is_empty());
        assert!(queries_for(&BackendType::Core, "zig", "0.13.0").is_empty());
        assert!(queries_for(&BackendType::Asdf, "node", "20.0.0").is_empty());
    }

    #[test]
    fn non_release_versions_are_not_looked_up() {
        assert!(is_release("1.2.3"));
        assert!(is_release("2024.01.15"));
        assert!(!is_release("ref:main"));
        assert!(!is_release("path:/opt/node"));
        assert!(!is_release("latest"));
        assert!(!is_release("system"));
    }

    #[test]
    fn cvss3_scores() {
        let score = |v| cvss3_base_score(v).unwrap();
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"), 9.8);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:H/A:N"), 7.5);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:R/S:C/C:L/I:L/A:N"), 6.1);
        assert_eq!(score("CVSS:3.0/AV:L/AC:H/PR:H/UI:R/S:U/C:L/I:N/A:N"), 1.8);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H"), 10.0);
        assert_eq!(cvss3_base_score("CVSS:4.0/AV:N/AC:L/AT:N"), None);
    }

    #[test]
    fn severity_prefers_the_database_rating() {
        let vuln: OsvVuln = serde_json::from_value(serde_json::json!({
            "id": "GHSA-x",
            "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"}],
            "database_specific": {"severity": "MODERATE"},
        }))
        .unwrap();
        assert_eq!(vuln.severity(), Some(Severity::Medium));
        let vuln: OsvVuln = serde_json::from_value(serde_json::json!({
            "id": "CVE-x",
            "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:H/A:N"}],
        }))
        .unwrap();
        assert_eq!(vuln.severity(), Some(Severity::High));
    }

    #[test]
    fn fixed_versions_come_from_the_queried_package() {
        let vuln: OsvVuln = serde_json::from_value(serde_json::json!({
            "id": "GHSA-x",
            "details": "\nFirst line.\nSecond line.",
            "affected": [
                {
                    "package": {"ecosystem": "npm", "name": "lodash"},
                    "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}],
                },
                {
                    "package": {"ecosystem": "npm", "name": "lodash-es"},
                    "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.22"}]}],
                },
                {
                    "package": {"ecosystem": "npm", "name": "lodash"},
                    "ranges": [{"type": "GIT", "events": [{"introduced": "0"}, {"fixed": "abc123"}]}],
                },
            ],
        }))
        .unwrap();
        assert_eq!(
            vuln.fixed_versions(&OsvPackage::new("npm", "lodash")),
            vec!["4.17.21".to_string()]
        );
        assert_eq!(vuln.summary(), "First line.");
    }
}
