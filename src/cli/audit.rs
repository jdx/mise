use std::collections::HashSet;

use console::style;
use eyre::Result;
use itertools::Itertools;
use serde::Serialize;

use crate::args::ToolArg;
use crate::audit::{self, Finding, Severity, Unchecked};
use crate::config::Config;
use crate::exit;
use crate::toolset::ToolsetBuilder;
use crate::ui::table::MiseTable;

/// Check tool versions for known vulnerabilities
///
/// Looks up each tool version the current config uses in the OSV.dev
/// vulnerability database and lists the advisories that affect it. Exits
/// with status 1 when it finds any. Run `mise upgrade --vulnerable` to
/// upgrade only the affected tools.
///
/// Package registry tools (npm, pipx, cargo, gem, go, dotnet) are looked up
/// by package name. Tools released from GitHub (github, aqua, packslip) are
/// looked up by repository and release tag, and Go programs also as Go
/// modules. Of the core tools, go, node, python, deno, bun and erlang are
/// covered. Other tools are listed as not checked.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment,
    example(r###"mise audit
Tool        Version  ID                   Severity  Fixed    Summary
npm:lodash  4.17.15  GHSA-35jh-r3h4-6jhm  high      4.17.21  Command Injection in lodash
go          1.22.0   GO-2024-2887         -         1.22.4   Unexpected behavior from Is methods for IPv4-mapped IPv6 addresses in net/netip"###,
        help = "Check every tool in the current config"),
    example(r###"mise audit node python"###,
        help = "Check only some tools"),
    example(r###"mise audit --json"###,
        help = "Print the advisories as JSON"))]
pub(crate) struct Audit {
    /// Tools to check, such as `node` or `node@20`
    ///
    /// Checks every tool in the current config when omitted.
    #[usage(value_name = "TOOL@VERSION")]
    tool: Vec<ToolArg>,

    /// Output in JSON format
    #[usage(short = 'J', long)]
    json: bool,

    /// Do not print the table header
    #[usage(long)]
    no_header: bool,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    vulnerabilities: &'a [Finding],
    unchecked: &'a [Unchecked],
}

impl Audit {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&config)
            .await?;
        let tool_set = self
            .tool
            .iter()
            .map(|t| t.ba.clone())
            .collect::<HashSet<_>>();
        let items = ts
            .list_current_versions()
            .into_iter()
            .filter(|(_, tv)| tool_set.is_empty() || tool_set.contains(tv.ba()))
            .map(|(backend, tv)| (backend, tv.version))
            .unique_by(|(backend, version)| (backend.id().to_string(), version.clone()))
            .collect_vec();
        let checked = items.len();
        let report = audit::scan(items).await?;
        let checked = checked - report.unchecked.len();

        if self.json {
            miseprintln!(
                "{}",
                serde_json::to_string_pretty(&JsonReport {
                    vulnerabilities: &report.findings,
                    unchecked: &report.unchecked,
                })?
            );
        } else {
            self.display_table(&report.findings)?;
            if !report.unchecked.is_empty() {
                info!(
                    "not checked, no known advisory source: {}",
                    report
                        .unchecked
                        .iter()
                        .map(|u| format!("{}@{}", u.tool, u.version))
                        .join(", ")
                );
            }
            if report.findings.is_empty() {
                if checked > 0 {
                    info!(
                        "no known vulnerabilities in {checked} tool version{}",
                        if checked == 1 { "" } else { "s" }
                    );
                }
            } else {
                let tools = report.findings.iter().map(|f| &f.tool).unique().count();
                let n = report.findings.len();
                info!(
                    "{n} known vulnerabilit{} in {tools} tool{}. Run `mise upgrade --vulnerable` to upgrade them.",
                    if n == 1 { "y" } else { "ies" },
                    if tools == 1 { "" } else { "s" },
                );
            }
        }
        if !report.findings.is_empty() {
            return Err(exit::request(1));
        }
        Ok(())
    }

    fn display_table(&self, findings: &[Finding]) -> Result<()> {
        if findings.is_empty() {
            return Ok(());
        }
        let mut table = MiseTable::new(
            self.no_header,
            &["Tool", "Version", "ID", "Severity", "Fixed", "Summary"],
        );
        for f in findings {
            table.add_row(vec![
                f.tool.clone(),
                f.version.clone(),
                f.id.clone(),
                display_severity(f.severity),
                if f.fixed.is_empty() {
                    "-".to_string()
                } else {
                    f.fixed.join(", ")
                },
                f.summary.clone(),
            ]);
        }
        table.print()
    }
}

fn display_severity(severity: Option<Severity>) -> String {
    match severity {
        None => "-".to_string(),
        Some(s @ (Severity::Critical | Severity::High)) => style(s).red().to_string(),
        Some(s @ Severity::Medium) => style(s).yellow().to_string(),
        Some(s @ Severity::Low) => style(s).dim().to_string(),
    }
}
