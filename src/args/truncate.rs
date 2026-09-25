/// Control whether long terminal output is shortened to fit the available
/// width.
#[derive(Debug, usage_rs::Args)]
pub(crate) struct TruncateOptions {
    /// Truncate long terminal output to fit the available width
    #[usage(
        long,
        negate = "no-truncate",
        default = "true",
        setting = "truncate",
        verbatim_doc_comment
    )]
    pub(crate) truncate: bool,
}
