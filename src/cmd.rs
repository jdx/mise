/// Create a command with any number of of positional arguments
///
/// may be different types (anything that implements [`Into<OsString>`](https://doc.rust-lang.org/std/convert/trait.From.html)).
/// See also the [`cmd`](fn.cmd.html) function, which takes a collection of arguments.
///
/// # Example
///
/// ```
///     use std::path::Path;
///     use mise::cmd;
///
///     let arg1 = "--version".to_owned();
///     let arg2 = Path::new("--verbose");
///
///     let output = cmd!("rustc", arg1, arg2).read();
///
///     assert!(output.unwrap().starts_with("rustc "));
/// ```
#[macro_export]
macro_rules! cmd {
    ( $program:expr $(, $arg:expr )* $(,)? ) => {
        {
            use std::ffi::OsString;
            let args: std::vec::Vec<OsString> = std::vec![$( Into::<OsString>::into($arg) ),*];
            $crate::cmd::cmd($program, args)
        }
    };
}

pub use mise_util::cmd::*;

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[tokio::test]
    async fn test_cmd() {
        let _config = Config::get().await.unwrap();
        let output = cmd!("echo", "foo", "bar").read().unwrap();
        assert_eq!("foo bar", output);
    }
}
