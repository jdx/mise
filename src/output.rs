#[cfg(test)]
#[macro_export]
macro_rules! rtxprintln {
    () => {
        rtxprint!("\n")
    };
    ($($arg:tt)*) => {{
        let mut stdout = $crate::output::tests::STDOUT.lock().unwrap();
        stdout.push(format!($($arg)*));
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxprintln {
    () => {
        rtxprint!("\n")
    };
    ($($arg:tt)*) => {{
        println!($($arg)*);
    }};
}

#[cfg(test)]
#[macro_export]
macro_rules! rtxprint {
    ($($arg:tt)*) => {{
        let mut stdout = $crate::output::tests::STDOUT.lock().unwrap();
        let cur = stdout.pop().unwrap_or_default();
        stdout.push(cur + &format!($($arg)*));
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxprint {
    ($($arg:tt)*) => {{
        print!($($arg)*);
    }};
}

#[cfg(test)]
#[macro_export]
macro_rules! rtxstatusln {
        ($($arg:tt)*) => {{
            let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
            let rtx = console::style("rtx ").dim().for_stderr();
            stderr.push(format!("{}{}", rtx, format!($($arg)*)));
        }};
    }

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxstatusln {
    ($($arg:tt)*) => {{
        let rtx = console::style("rtx ").dim().for_stderr();
        eprintln!("{}{}", rtx, format!($($arg)*));
    }};
}

#[cfg(test)]
pub mod tests {
    use std::sync::Mutex;

    pub static STDOUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pub static STDERR: Mutex<Vec<String>> = Mutex::new(Vec::new());
}
