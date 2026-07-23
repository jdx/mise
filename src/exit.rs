use crate::cmd::CmdLineRunner;
#[cfg(unix)]
use nix::sys::signal::SIGTERM;

pub fn exit(code: i32) -> ! {
    #[cfg(unix)]
    CmdLineRunner::kill_all(SIGTERM);

    #[cfg(windows)]
    CmdLineRunner::kill_all();

    if let Some(config) = crate::config::Config::maybe_get() {
        config.clear_tasks_cache();
    }
    crate::config::clear_remote_task_include_artifacts();
    crate::task::task_fetcher::clear_remote_task_artifacts();
    crate::task::task_file_providers::cleanup_temporary_artifacts();

    debug!("exiting with code: {code}");
    std::process::exit(code)
}
