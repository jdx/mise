use std::time::Duration;

pub fn format_duration(dur: Duration) -> String {
    if dur < Duration::from_millis(1) {
        format!("{dur:.0?}")
    } else if dur < Duration::from_secs(1) {
        format!("{dur:.1?}")
    } else {
        format!("{dur:.2?}")
    }
}
