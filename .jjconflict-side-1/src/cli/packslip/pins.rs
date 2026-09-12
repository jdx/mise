use eyre::Result;
use tabled::{Table, Tabled};

use crate::packslip_pins;
use crate::ui::table;

/// List the signers mise has accepted packslips from
#[derive(Debug, Default, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment)]
pub(super) struct PackslipPins {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "Project")]
    project: String,
    #[tabled(rename = "Scheme")]
    scheme: String,
    #[tabled(rename = "Signer")]
    signer: String,
    #[tabled(rename = "Attested by")]
    attested_by: String,
    #[tabled(rename = "Provenance")]
    provenance: String,
}

impl PackslipPins {
    pub(super) fn run(self) -> Result<()> {
        let pins = packslip_pins::list()?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&pins)?);
            return Ok(());
        }
        if pins.is_empty() {
            miseprintln!("no packslip signers pinned yet");
            return Ok(());
        }
        let rows = pins.into_iter().map(|(project, pin)| Row {
            project,
            scheme: pin.scheme,
            signer: pin.signer,
            attested_by: pin.attested_by,
            provenance: if pin.provenance { "yes" } else { "no" }.to_string(),
        });
        let mut table = Table::new(rows);
        table::print(&mut table, false)
    }
}
