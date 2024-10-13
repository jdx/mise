use crate::cmd::CmdLineRunner;
#[cfg(unix)]
use nix::sys::signal::SIGTERM;

pub fn exit(code: i32) -> ! {
    #[cfg(unix)]
    CmdLineRunner::kill_all(SIGTERM);

    #[cfg(windows)]
    CmdLineRunner::kill_all();

    debug!("exiting with code: {code}");
    std::process::exit(code)
}
