use crate::cmd::CmdLineRunner;
use nix::sys::signal::SIGTERM;

pub fn exit(code: i32) -> ! {
    CmdLineRunner::kill_all(SIGTERM);
    debug!("exiting with code: {code}");
    std::process::exit(code)
}
