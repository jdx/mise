#[cfg(feature = "cli")]
#[macro_use]
extern crate log;

#[cfg(feature = "cli")]
mod cli;

#[allow(clippy::needless_return)]
#[cfg(feature = "cli")]
#[tokio::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::default().filter_or("VFOX_LOG", "info"));
    if let Err(err) = cli::run().await {
        error!("{err}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "cli"))]
fn main() {
    panic!("cli feature is not enabled");
}
