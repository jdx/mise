use eyre::Result;
use tabled::{Table, Tabled};

use crate::config::Config;
use crate::ui::table;

/// List the skills the active tools declare
///
/// Each line is a skill of a tool that is installed and active in the current
/// directory, with the version it belongs to and the directory holding its
/// `SKILL.md`.
#[derive(Debug, Default, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(super) struct SkillsLs {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "Skill")]
    name: String,
    #[tabled(rename = "Tool")]
    tool: String,
    #[tabled(rename = "Version")]
    version: String,
    #[tabled(rename = "Path")]
    path: String,
}

impl SkillsLs {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let skills = crate::packslip::active_skills(&config).await?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&skills)?);
            return Ok(());
        }
        if skills.is_empty() {
            miseprintln!("no skills declared by the active tools");
            return Ok(());
        }
        let rows = skills.into_iter().map(|s| Row {
            name: s.name,
            tool: s.tool,
            version: s.version,
            path: s.path.display().to_string(),
        });
        let mut table = Table::new(rows);
        table::print(&mut table, false)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise skills ls</bold>
    Skill  Tool                        Version  Path
    mise   packslip:github.com/jdx/mise  2026.9.1  ~/.local/share/mise/installs/.../skills/mise
"#
);
