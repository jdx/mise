use eyre::Result;

use crate::system::history::checkpoint::annotate;
use crate::system::history::store::{Annotation, DescriptionSource};

/// Set the description of a checkpoint
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct HistoryDescribe {
    /// Checkpoint id, `latest`, `latest~N`, or a uuid prefix
    #[usage(value_name = "REF")]
    reference: String,

    /// The new description
    #[usage(value_name = "TEXT")]
    text: String,
}

impl HistoryDescribe {
    pub(crate) async fn run(self) -> Result<()> {
        let (store, _tracked, entries) = super::open().await?;
        let entry = super::resolve(&self.reference, &entries, None)?;
        let annotation = Annotation {
            description: Some(self.text.trim().to_string()),
            description_source: Some(DescriptionSource::User),
            pinned: None,
            labels: None,
            updated_at: crate::system::history::store::now_rfc3339(),
        };
        annotate(&store, &entry, annotation)?;
        info!("history: checkpoint {} described", entry.id);
        Ok(())
    }
}
