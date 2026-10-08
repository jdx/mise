use crate::args::ToolArg;
use crate::config::Config;
use crate::file;
use crate::toolset::ToolsetBuilder;
use eyre::Result;
use serde::Serialize;
use std::path::PathBuf;

/// List the bin directories of the active tools
///
/// Prints the bin directories of the tool versions the current config selects. It
/// does not include `_.path` entries from [env]; `mise doctor path` lists everything
/// mise adds to PATH.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bin-paths",
        help = "Print the bin directory of every active tool"
    ),
    example("mise bin-paths node", help = "Print only node's bin directory"),
    example(
        "mise bin-paths --bin-names",
        help = "List the executables the active tools provide"
    )
)]
pub(crate) struct BinPaths {
    /// Only list these tools, such as `ruby@3`
    #[usage(value_name = "TOOL@VERSION")]
    tool: Option<Vec<ToolArg>>,

    /// Print executable names instead of bin directories
    #[usage(long)]
    bin_names: bool,

    /// Print executables as JSON objects with `name`, `path`, and `symlink` (implies --bin-names)
    #[usage(long, short = 'J')]
    json: bool,
}

impl BinPaths {
    pub(crate) async fn run(self) -> Result<()> {
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
        if self.bin_names || self.json {
            let bins = list_bins(paths)?;
            if self.json {
                miseprintln!("{}", serde_json::to_string_pretty(&bins)?);
            } else {
                for bin in bins {
                    miseprintln!("{}", bin.name);
                }
            }
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
