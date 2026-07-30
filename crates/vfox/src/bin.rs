#[cfg(feature = "cli")]
#[macro_use]
extern crate log;

#[cfg(feature = "cli")]
mod cli;

#[cfg(feature = "cli")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    env_logger::init_from_env(env_logger::Env::default().filter_or("VFOX_LOG", "info"));
    if let Err(err) = cli::run().await {
        error!("{err}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(not(feature = "cli"))]
fn main() {
    panic!("cli feature is not enabled");
}
