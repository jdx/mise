//! Locating the subcommand in mise's raw arguments without building the clap
//! tree, for code that runs before arguments are parsed.

/// Long and short forms of the top-level flags that consume a following argument.
///
/// Hardcoded rather than derived because `env.rs` needs it from `Lazy` statics
/// during startup — before anything has parsed arguments — and deriving it means
/// building the entire clap tree, which costs ~3.1M instructions. Doing that
/// there is what made every mise command ~6.3M instructions more expensive.
///
/// mise's `test_global_flags_with_values_matches_clap` asserts this equals what clap
/// reports, so adding a value-taking flag to its `Cli` without updating this list
/// fails CI rather than silently mis-parsing arguments.
pub const GLOBAL_FLAGS_WITH_VALUES: &[&str] = &[
    "--cd",
    "-C",
    "--env",
    "-E",
    "--jobs",
    "-j",
    "--profile",
    "-P",
    "--shell",
    "-s",
    "--tool",
    "-t",
    "--log-level",
    "--output",
];

/// The index of the first argument that is not a global flag or one of its
/// values, against [`GLOBAL_FLAGS_WITH_VALUES`].
///
/// For callers with no `Command` to hand, which would otherwise build the whole
/// tree just to read its top-level arguments.
pub fn first_non_global_arg_idx_cached(args: &[String]) -> Option<usize> {
    first_non_global_arg_idx_with(|f| GLOBAL_FLAGS_WITH_VALUES.contains(&f), args)
}

/// As [`first_non_global_arg_idx_cached`], with `takes_value` deciding which
/// flags consume the next argument.
pub fn first_non_global_arg_idx_with(
    takes_value: impl Fn(&str) -> bool,
    args: &[String],
) -> Option<usize> {
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            return None;
        }

        if !arg.starts_with('-') {
            return Some(i);
        }

        let flag_takes_separate_value = if arg.starts_with("--") {
            if arg.contains('=') {
                false
            } else {
                let flag_name = arg.split('=').next().unwrap();
                takes_value(flag_name)
            }
        } else if let Some(flag_name) = arg.get(..2) {
            // `arg.get(..2)` (not `&arg[..2]`) avoids panicking when the arg is
            // not valid UTF-8 in the first place: args are read lossily, so a
            // malformed byte becomes a multi-byte U+FFFD and byte index 2 may not
            // be a char boundary. A short flag is always ASCII, so a non-ASCII
            // prefix simply matches no value-taking flag.
            arg.len() == 2 && takes_value(flag_name)
        } else {
            false
        };

        if flag_takes_separate_value && i + 1 < args.len() {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}
