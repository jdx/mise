use clap::builder::ValueParser;
use clap::Arg;
use std::num::ParseIntError;

#[derive(Clone)]
pub struct Jobs(pub usize);

fn parse_jobs(input: &str) -> Result<usize, ParseIntError> {
    input.parse::<usize>()
}

impl Jobs {
    pub fn arg() -> clap::Arg {
        Arg::new("jobs")
            .short('j')
            .long("jobs")
            .help("Number of plugins and runtimes to install in parallel")
            .value_parser(ValueParser::new(parse_jobs))
            .default_value("4")
            .global(true)
    }
}
