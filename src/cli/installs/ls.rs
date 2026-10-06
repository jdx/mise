use comfy_table::Cell;
use eyre::Result;

use crate::args::BackendArg;
use crate::file::display_path;
use crate::install_layout::resolver::{self, Installation};
use crate::ui::table::MiseTable;

/// List the installations of the identity install layout
///
/// Every installation is listed separately, so variants of one version (made
/// with different options, or pinned to different artifacts by lockfiles) can
/// be told apart. The status column says which installation requests without
/// a lockfile use (`selected`), which a lockfile adopted (`pinned`), and which
/// live in a read-only shared installs directory (`shared`).
#[derive(Debug, Default, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example(
        r###"mise installs ls"###,
        help = r###"Every installation in the identity layout"###
    ),
    example(
        r###"mise installs ls node --json"###,
        help = r###"The installations of node, as JSON"###
    ),
    verbatim_doc_comment
)]
pub(super) struct InstallsLs {
    /// Only show the installations of these tools
    #[usage(value_name = "TOOL")]
    tool: Vec<BackendArg>,

    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Don't display headers
    #[usage(long)]
    no_header: bool,
}

impl InstallsLs {
    pub(super) async fn run(self) -> Result<()> {
        let mut installations = resolver::installations();
        if !self.tool.is_empty() {
            installations.retain(|i| {
                self.tool
                    .iter()
                    .any(|ba| resolver::installation_belongs_to(ba, i))
            });
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&installations)?);
            return Ok(());
        }
        let mut table = MiseTable::new(
            self.no_header,
            &["Installation", "Tool", "Version", "Platform", "Status"],
        );
        for i in &installations {
            table.add_row(vec![
                Cell::new(&i.name),
                Cell::new(tool_label(i)),
                Cell::new(&i.version),
                Cell::new(&i.platform),
                Cell::new(status(i)),
            ]);
        }
        table.print()?;
        if installations.iter().any(|i| i.shared) {
            for i in installations.iter().filter(|i| i.shared) {
                info!("{} is in {}", i.name, display_path(&i.dir));
            }
        }
        Ok(())
    }
}

/// The tool an installation was requested as, with the options that set it
/// apart from other installations of the same version.
fn tool_label(i: &Installation) -> String {
    let tool = i.requested_as.clone().unwrap_or_else(|| i.backend.clone());
    if i.options.is_empty() {
        return tool;
    }
    let options = i
        .options
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{tool}[{options}]")
}

fn status(i: &Installation) -> String {
    [
        (i.selected, "selected"),
        (i.pinned, "pinned"),
        (i.shared, "shared"),
    ]
    .into_iter()
    .filter_map(|(on, word)| on.then_some(word))
    .collect::<Vec<_>>()
    .join(", ")
}
