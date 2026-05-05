use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::file;
use crate::toolset::ToolsetBuilder;
use eyre::Result;
use serde_derive::Serialize;
use std::path::PathBuf;

/// List all the active runtime bin paths
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct BinPaths {
    /// Tool(s) to look up
    /// e.g.: ruby@3
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Option<Vec<ToolArg>>,

    /// Output executable entries in JSON format
    #[clap(long, short = 'J')]
    json: bool,
}

impl BinPaths {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut tsb = ToolsetBuilder::new();
        if let Some(tool) = &self.tool {
            tsb = tsb.with_args(tool);
        }
        let mut ts = tsb.build(&config).await?;
        if let Some(tool) = &self.tool {
            ts.versions.retain(|k, _| tool.iter().any(|t| *t.ba == **k));
        }
        ts.notify_if_versions_missing(&config).await;
        let paths = ts.list_paths(&config).await;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&list_bins(paths)?)?);
            return Ok(());
        }
        for p in paths {
            miseprintln!("{}", p.display());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct BinPathEntry {
    name: String,
    path: PathBuf,
    symlink: bool,
}

fn list_bins(paths: Vec<PathBuf>) -> Result<Vec<BinPathEntry>> {
    let mut bins = vec![];
    for dir in paths.into_iter().filter(|path| path.is_dir()) {
        let Ok(entries) = dir.read_dir() else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() && !file_type.is_symlink() {
                continue;
            }

            let path = entry.path();
            if !path.is_file() || !file::is_executable(&path) {
                continue;
            }

            bins.push(BinPathEntry {
                name: entry
                    .file_name()
                    .into_string()
                    .unwrap_or_else(|name| name.to_string_lossy().into_owned()),
                path,
                symlink: file_type.is_symlink(),
            });
        }
    }
    bins.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    bins.dedup_by(|a, b| a.name == b.name && a.path == b.path);
    Ok(bins)
}
