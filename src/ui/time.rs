pub fn format_duration(dur: std::time::Duration) -> String {
    if dur < std::time::Duration::from_secs(1) {
        format!("{:.0?}", dur)
    } else {
        format!("{:.2?}", dur)
    }
}
