use crate::env::RTX_HIDE_OUTDATED_BUILD;
use build_time::build_time_utc;
use chrono::{DateTime, FixedOffset, Months, Utc};
use console::style;

use lazy_static::lazy_static;

lazy_static! {
    pub static ref BUILD_TIME: DateTime<FixedOffset> =
        DateTime::parse_from_rfc3339(build_time_utc!()).unwrap();
}

#[ctor::ctor]
fn init() {
    if !*RTX_HIDE_OUTDATED_BUILD
        && BUILD_TIME.checked_add_months(Months::new(12)).unwrap() < Utc::now()
    {
        let rtx = style("rtx").dim().for_stderr();
        eprintln!(
            "{rtx} rtx has not been updated in over a year. Please update to the latest version"
        );
        if cfg!(feature = "self_update") {
            eprintln!("{rtx} update with: `rtx self-update`");
        }
        eprintln!("{rtx} To hide this warning, set RTX_HIDE_OUTDATED_BUILD=1.");
    }
}
