use chrono::{DateTime, FixedOffset, Months, Utc};
use console::style;
use lazy_static::lazy_static;

use crate::env::RTX_HIDE_UPDATE_WARNING;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

lazy_static! {
    pub static ref BUILD_TIME: DateTime<FixedOffset> =
        DateTime::parse_from_rfc2822(built_info::BUILT_TIME_UTC).unwrap();
}

#[ctor::ctor]
fn init() {
    if !*RTX_HIDE_UPDATE_WARNING
        && BUILD_TIME.checked_add_months(Months::new(12)).unwrap() < Utc::now()
    {
        eprintln!("{}", render_outdated_message());
    }
}

fn render_outdated_message() -> String {
    let rtx = style("rtx").dim().for_stderr();
    let mut output = vec![];
    output.push(format!(
        "{rtx} rtx has not been updated in over a year. Please update to the latest version."
    ));
    if cfg!(any(
        feature = "self_update",
        feature = "alpine",
        feature = "brew",
        feature = "deb",
        feature = "rpm",
    )) {
        output.push(format!("{rtx} update with: `rtx self-update`"));
    }
    output.push(format!(
        "{rtx} To hide this warning, set RTX_HIDE_OUTDATED_BUILD=1."
    ));

    output.join("\n")
}
