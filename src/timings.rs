use crate::env;
use crate::ui::{style, time};
use std::time::{Duration, Instant};

pub fn start(module: &str) -> impl FnOnce() {
    let start = Instant::now();
    let module = module.to_string();
    move || {
        let diff = start.elapsed();
        eprintln!("{}", render(module.as_str(), diff));
    }
}

static START: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

pub fn get_time_diff(module: &str) -> String {
    if *env::MISE_TIMINGS == 0 {
        return "".to_string();
    }
    static PREV: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let now = Instant::now();
    if PREV.lock().unwrap().is_none() {
        *START.lock().unwrap() = Some(now);
        *PREV.lock().unwrap() = Some(now);
    }
    let mut prev = PREV.lock().unwrap();
    let diff = now.duration_since(prev.unwrap());
    *prev = Some(now);
    render(module, diff)
}

fn render(module: &str, diff: Duration) -> String {
    let diff_str = if *env::MISE_TIMINGS == 2 {
        let relative = time::format_duration(diff);
        let from_start =
            time::format_duration(Instant::now().duration_since(START.lock().unwrap().unwrap()));
        format!("{relative} {from_start}")
    } else {
        time::format_duration(diff)
    };
    let thread_id = crate::logger::thread_id();
    let out = format!("[TIME] {thread_id} {module} {diff_str}")
        .trim()
        .to_string();
    if diff.as_micros() > 8000 {
        style::eblack(out).on_red().bold()
    } else if diff.as_micros() > 4000 {
        style::eblack(out).on_red()
    } else if diff.as_micros() > 2000 {
        style::ered(out).bright()
    } else if diff.as_micros() > 1000 {
        style::eyellow(out).bright()
    } else if diff.as_micros() > 500 {
        style::eyellow(out).dim()
    } else if diff.as_micros() > 100 {
        style::ecyan(out).dim()
    } else {
        style::edim(out)
    }
    .to_string()
}

#[macro_export]
macro_rules! time {
    ($fn:expr) => {{
        if *$crate::env::MISE_TIMINGS > 1 {
            let module = format!("{}::{}", module_path!(), format!($fn));
            eprintln!("{}", $crate::timings::get_time_diff(&module));
        } else {
            trace!($fn);
        }
    }};
    ($fn:expr, $($arg:tt)+) => {{
        if *$crate::env::MISE_TIMINGS > 1 {
            let module = format!("{}::{}", module_path!(), format!($fn, $($arg)+));
            eprintln!("{}", $crate::timings::get_time_diff(&module));
        } else {
            trace!($fn, $($arg)+);
        }
    }};
}

#[macro_export]
macro_rules! measure {
    ($fmt:expr, $block:block) => {{
        if *$crate::env::MISE_TIMINGS > 0 {
            let module = format!("{}::{}", module_path!(), format!($fmt));
            let end = $crate::timings::start(&module);
            let result = $block;
            end();
            result
        } else if log::log_enabled!(log::Level::Trace) {
            let msg = format!($fmt);
            trace!("{msg} start");
            let result = $block;
            trace!("{msg} done");
            result
        } else {
            $block
        }
    }};
}
