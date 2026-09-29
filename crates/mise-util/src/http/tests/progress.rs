use super::*;

#[test]
fn slow_download_detector_reports_a_crawling_window_once_it_is_full() {
    let start = Instant::now();
    let mut detector = SlowDownloadDetector::new(start);
    // ~3 kB/s, like a throttled CDN edge.
    assert_eq!(
        detector.observe(start + Duration::from_secs(30), 90_000),
        None
    );
    assert_eq!(
        detector.observe(start + Duration::from_secs(60), 180_000),
        Some(3_000)
    );
}

#[test]
fn slow_download_detector_ignores_healthy_windows() {
    let start = Instant::now();
    let mut detector = SlowDownloadDetector::new(start);
    assert_eq!(
        detector.observe(start + Duration::from_secs(60), 60 * 1024 * 1024),
        None
    );
}

#[test]
fn slow_download_detector_measures_each_window_from_its_own_start() {
    let start = Instant::now();
    let mut detector = SlowDownloadDetector::new(start);
    // A healthy first minute does not hide a crawling second minute.
    assert_eq!(
        detector.observe(start + Duration::from_secs(60), 60 * 1024 * 1024),
        None
    );
    assert_eq!(
        detector.observe(start + Duration::from_secs(120), 60 * 1024 * 1024 + 60_000),
        Some(1_000)
    );
}

#[test]
fn download_bytes_total_survives_retries() {
    let bytes = DownloadProgress::default();
    bytes.start_attempt();
    bytes.attempt.fetch_add(1_500, Ordering::Relaxed);
    assert_eq!(bytes.total(), 1_500);
    // A retry restarts the per-attempt count but keeps the total, even once
    // the new attempt passes the old attempt's count.
    bytes.start_attempt();
    assert_eq!(bytes.attempt.load(Ordering::Relaxed), 0);
    assert_eq!(bytes.total(), 1_500);
    bytes.attempt.fetch_add(2_000, Ordering::Relaxed);
    assert_eq!(bytes.attempt.load(Ordering::Relaxed), 2_000);
    assert_eq!(bytes.total(), 3_500);
}

#[test]
fn slow_download_detector_restart_drops_the_previous_window() {
    let start = Instant::now();
    let mut detector = SlowDownloadDetector::new(start);
    // 50 s at 1 kB/s from one host, then a retry reaches another host.
    detector.restart(start + Duration::from_secs(50), 50_000);
    // Without the restart this minute would already be judged.
    assert_eq!(
        detector.observe(start + Duration::from_secs(60), 60_000),
        None
    );
    // The new host's own full minute is what gets reported.
    assert_eq!(
        detector.observe(start + Duration::from_secs(110), 110_000),
        Some(1_000)
    );
}

fn served(host: &str, since: Instant, total_at_start: u64) -> ServedBy {
    ServedBy {
        host: host.to_string(),
        since,
        total_at_start,
    }
}

#[test]
fn slow_download_watch_blames_the_host_that_was_slow() {
    let start = Instant::now();
    let mut watch = SlowDownloadWatch::new(start);
    let a = served("a.example", start, 0);
    // 55 s healthy from a, then a retry reaches b, which stalls.
    let b = served("b.example", start + Duration::from_secs(55), 55 << 20);
    assert_eq!(
        watch.sample(start + Duration::from_secs(5), 5 << 20, Some(&a)),
        None
    );
    assert_eq!(
        watch.sample(start + Duration::from_secs(60), 55 << 20, Some(&b)),
        None,
        "a was healthy and b has not had a full minute"
    );
    assert_eq!(
        watch.sample(start + Duration::from_secs(115), 55 << 20, Some(&b)),
        Some((Some("b.example".to_string()), 0))
    );
}

#[test]
fn slow_download_watch_reports_a_slow_minute_that_ends_in_a_host_switch() {
    let start = Instant::now();
    let mut watch = SlowDownloadWatch::new(start);
    let a = served("a.example", start, 0);
    assert_eq!(
        watch.sample(start + Duration::from_secs(5), 5_000, Some(&a)),
        None
    );
    // a crawled at 1 kB/s for 62 s; b took over between samples.
    let b = served("b.example", start + Duration::from_secs(62), 62_000);
    assert_eq!(
        watch.sample(start + Duration::from_secs(65), 10 << 20, Some(&b)),
        Some((Some("a.example".to_string()), 1_000))
    );
}

#[test]
fn download_progress_keeps_the_start_of_a_host_across_same_host_retries() {
    let progress = DownloadProgress::default();
    let url = Url::parse("https://mirror.example/node.tar.gz").unwrap();
    progress.served_by(&url);
    let first = progress.served_by.lock().unwrap().clone();
    progress.start_attempt();
    progress.attempt.fetch_add(10, Ordering::Relaxed);
    progress.served_by(&url);
    assert_eq!(*progress.served_by.lock().unwrap(), first);
}

#[test]
fn download_progress_names_the_host_that_served_the_response() {
    let progress = DownloadProgress::default();
    progress.served_by(&Url::parse("https://mirror.example/node.tar.gz").unwrap());
    assert_eq!(
        progress
            .served_by
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.host.as_str()),
        Some("mirror.example")
    );
}
